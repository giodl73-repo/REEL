use std::{fs, path::Path};

use anyhow::{Context, Result, bail};
use reel_music::{
    edl::{self, EditDecisionList},
    evidence::{self, WriteReport},
    source::RawPcmFormat,
};
use tempfile::NamedTempFile;

use crate::adapters::ffmpeg::FfmpegAdapter;

pub fn render(
    edl_path: &Path,
    repair_path: &Path,
    output_pcm: &Path,
    evidence_path: &Path,
) -> Result<WriteReport> {
    if output_pcm.exists() {
        bail!(
            "music repair output already exists: {}",
            output_pcm.display()
        );
    }
    if evidence_path.exists() {
        bail!(
            "music repair evidence output already exists: {}",
            evidence_path.display()
        );
    }
    edl::validate(edl_path, repair_path)?;
    let decisions = edl::load(edl_path)?;
    let output_parent = output_pcm.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(output_parent)?;
    let output_parent = fs::canonicalize(output_parent)?;
    let output_name = output_pcm
        .file_name()
        .ok_or_else(|| anyhow::anyhow!("music repair output must name a file"))?;
    let published_output = output_parent.join(output_name);

    let reservation = NamedTempFile::new_in(&output_parent)?;
    let temporary_output = reservation.path().to_path_buf();
    reservation.close()?;

    let result = render_then_publish(&decisions, &temporary_output, &published_output);
    if result.is_err() {
        let _ = fs::remove_file(&temporary_output);
    }
    let adapter_version = result?;
    let report = evidence::analyze(
        edl_path,
        repair_path,
        &published_output,
        "ffmpeg",
        &adapter_version,
    )?;
    let written = evidence::write(evidence_path, &report)?;
    if !written.passed {
        bail!("music repair evidence failed; candidate and evidence were retained for review");
    }
    Ok(written)
}

pub fn check(
    evidence_path: &Path,
    edl_path: &Path,
    repair_path: &Path,
    candidate_pcm: &Path,
) -> Result<WriteReport> {
    let report = evidence::check(evidence_path, edl_path, repair_path, candidate_pcm)?;
    if !report.passed {
        bail!("music repair evidence records a failed candidate");
    }
    Ok(report)
}

fn render_then_publish(
    decisions: &EditDecisionList,
    temporary_output: &Path,
    published_output: &Path,
) -> Result<String> {
    let adapter = FfmpegAdapter;
    let (demuxer, codec) = pcm_names(decisions.format);
    let source = adapter.path_argument(&decisions.source_pcm)?;
    let temporary = adapter.path_argument(temporary_output)?;
    let filter = filter_graph(decisions);
    let args = vec![
        "-hide_banner".to_string(),
        "-loglevel".to_string(),
        "error".to_string(),
        "-f".to_string(),
        demuxer.to_string(),
        "-ar".to_string(),
        decisions.timebase.sample_rate_hz.to_string(),
        "-ac".to_string(),
        decisions.timebase.channels.to_string(),
        "-i".to_string(),
        source,
        "-filter_complex".to_string(),
        filter,
        "-map".to_string(),
        "[out]".to_string(),
        "-f".to_string(),
        demuxer.to_string(),
        "-c:a".to_string(),
        codec.to_string(),
        "-ar".to_string(),
        decisions.timebase.sample_rate_hz.to_string(),
        "-ac".to_string(),
        decisions.timebase.channels.to_string(),
        temporary,
    ];
    adapter.run_ffmpeg(&args, &[])?;
    fs::rename(temporary_output, published_output).with_context(|| {
        format!(
            "failed to publish rendered PCM {}",
            published_output.display()
        )
    })?;
    let version = adapter.run_ffmpeg(&["-version".to_string()], &[])?;
    Ok(version
        .lines()
        .next()
        .unwrap_or("unknown")
        .trim()
        .to_string())
}

fn filter_graph(decisions: &EditDecisionList) -> String {
    let count = decisions.segments.len();
    let split_outputs = (0..count)
        .map(|index| format!("[s{index}]"))
        .collect::<String>();
    let mut filters = vec![format!("[0:a]asplit={count}{split_outputs}")];
    for (index, segment) in decisions.segments.iter().enumerate() {
        filters.push(format!(
            "[s{index}]atrim=start_sample={}:end_sample={},asetpts=PTS-STARTPTS[k{index}]",
            segment.source.start, segment.source.end
        ));
    }
    let inputs = (0..count)
        .map(|index| format!("[k{index}]"))
        .collect::<String>();
    filters.push(format!("{inputs}concat=n={count}:v=0:a=1[out]"));
    filters.join(";")
}

fn pcm_names(format: RawPcmFormat) -> (&'static str, &'static str) {
    match format {
        RawPcmFormat::RawPcmU8 => ("u8", "pcm_u8"),
        RawPcmFormat::RawPcmS16le => ("s16le", "pcm_s16le"),
        RawPcmFormat::RawPcmS24le => ("s24le", "pcm_s24le"),
        RawPcmFormat::RawPcmS32le => ("s32le", "pcm_s32le"),
        RawPcmFormat::RawPcmF32le => ("f32le", "pcm_f32le"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reel_music::{
        edl::{EvidencePolicy, KeepSegment},
        time::{AudioTimebase, SampleRange},
    };
    use std::path::PathBuf;

    #[test]
    fn emits_sample_exact_cut_filter_graph() {
        let decisions = EditDecisionList {
            schema: edl::SCHEMA.into(),
            repair_manifest: PathBuf::from("repair.json"),
            repair_manifest_sha256: "0".repeat(64),
            repair_contract_sha256: "1".repeat(64),
            source_manifest: PathBuf::from("source.json"),
            source_manifest_sha256: "2".repeat(64),
            source_contract_sha256: "3".repeat(64),
            source_id: "source".into(),
            source_pcm: PathBuf::from("source.u8"),
            source_pcm_sha256: "4".repeat(64),
            format: RawPcmFormat::RawPcmU8,
            timebase: AudioTimebase {
                sample_rate_hz: 8_000,
                channels: 1,
                samples_per_channel: 12,
            },
            output_samples_per_channel: 8,
            segments: vec![
                KeepSegment {
                    id: "keep-001".into(),
                    source: SampleRange { start: 0, end: 4 },
                    output: SampleRange { start: 0, end: 4 },
                },
                KeepSegment {
                    id: "keep-002".into(),
                    source: SampleRange { start: 8, end: 12 },
                    output: SampleRange { start: 4, end: 8 },
                },
            ],
            cuts: vec![],
            evidence_policy: EvidencePolicy::default(),
            shareable: false,
        };

        assert_eq!(
            filter_graph(&decisions),
            "[0:a]asplit=2[s0][s1];[s0]atrim=start_sample=0:end_sample=4,asetpts=PTS-STARTPTS[k0];[s1]atrim=start_sample=8:end_sample=12,asetpts=PTS-STARTPTS[k1];[k0][k1]concat=n=2:v=0:a=1[out]"
        );
    }
}
