//! Ephemeral bounded filter controls keep large picture jobs off the OS argv.
//! Only already verified physical media paths enter the source graph.
use super::*;
use std::io::Write;

const INLINE_BYTES: usize = 8 * 1024;
const CONTROL_BYTES: usize = 4 * 1024 * 1024;

// AVOption tokenization and filter-graph tokenization each consume one escape.
fn option(value: &str) -> Result<String> {
    if value.chars().any(char::is_control) {
        bail!("picture source contains unsupported control characters");
    }
    let mut first = String::new();
    for c in value.chars() {
        if matches!(c, '\\' | ':' | '\'') {
            first.push('\\');
        }
        first.push(c);
    }
    let mut result = String::new();
    for c in first.chars() {
        if matches!(c, '\\' | '\'' | '[' | ']' | ',' | ';') {
            result.push('\\');
        }
        result.push(c);
    }
    Ok(result)
}

fn output_format(program: &Path) -> Result<String> {
    let help = Command::new(program)
        .args(["-hide_banner", "-h", "filter=format"])
        .output()?;
    if !help.status.success() || help.stdout.len() + help.stderr.len() > 65536 {
        bail!("native format capability query failed or exceeded budget");
    }
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&help.stdout),
        String::from_utf8_lossy(&help.stderr)
    );
    if !text.contains("pix_fmts") {
        bail!("native format capability missing");
    }
    let mut value = "format=pix_fmts=yuv444p".to_owned();
    // Match the normal CLI encoder sink. Newer FFmpeg negotiates color link
    // properties too; inheriting the RGB source's range at a YUV lavfi sink
    // would change pixels before the outer encoder sees them.
    if text.contains("color_spaces") {
        value.push_str(":color_spaces=unknown");
    }
    if text.contains("color_ranges") {
        value.push_str(":color_ranges=unknown");
    }
    Ok(value)
}

fn external(
    args: &[String],
    root: &Path,
    output_format: &str,
    clock: &str,
) -> Result<(Vec<String>, tempfile::NamedTempFile)> {
    let (n, d) = clock
        .split_once('/')
        .context("picture output clock missing")?;
    if n.parse::<u32>()? == 0 || d.parse::<u32>()? == 0 {
        bail!("picture output clock must be positive");
    }
    let filter = args
        .iter()
        .position(|a| a == "-filter_complex")
        .context("picture graph missing")?;
    if args
        .get(filter + 2..)
        .map(|tail| tail.iter().map(String::as_str).collect::<Vec<_>>())
        != Some(vec!["-map", "[v]", "-an"])
    {
        bail!("picture graph command shape differs");
    }
    let mut sources = Vec::new();
    let mut graph = args
        .get(filter + 1)
        .context("picture graph missing")?
        .clone();
    let mut i = 0;
    while i < filter {
        let fps = if args[i] == "-loop" {
            if args.get(i + 1).map(String::as_str) != Some("1")
                || args.get(i + 2).map(String::as_str) != Some("-framerate")
            {
                bail!("picture loop controls differ");
            }
            let fps = args
                .get(i + 3)
                .context("picture frame clock missing")?
                .clone();
            if fps.is_empty() || !fps.bytes().all(|c| c.is_ascii_digit() || c == b'/') {
                bail!("picture frame clock differs");
            }
            i += 4;
            Some(fps)
        } else {
            None
        };
        if args.get(i).map(String::as_str) != Some("-i") || i + 1 >= filter {
            bail!("picture input controls differ");
        }
        let path = Path::new(&args[i + 1]).canonicalize()?;
        let path = path
            .to_str()
            .context("picture source path is not Unicode")?;
        let index = sources.len();
        let mut source = format!(
            "movie=filename={}:dec_threads=1:format_opts=protocol_whitelist=file",
            option(path)?
        );
        if let Some(fps) = fps {
            let (n, d) = fps
                .split_once('/')
                .context("picture rational clock missing")?;
            if n.parse::<u32>()? == 0 || d.parse::<u32>()? == 0 {
                bail!("picture rational clock must be positive");
            }
            source.push_str(&format!(
                ",loop=loop=-1:size=1:start=0,settb=expr={d}/{n},setpts=N"
            ));
        } else {
            // Normal CLI demuxing subtracts the input stream start timestamp.
            source.push_str(",setpts=PTS-STARTPTS");
        }
        source.push_str(&format!("[src{index}]"));
        sources.push(source);
        graph = graph.replace(&format!("[{index}:v]"), &format!("[src{index}]"));
        i += 2;
    }
    if sources.is_empty() {
        bail!("picture source graph is empty");
    }
    let text = format!(
        "{};{};[v]settb=expr={d}/{n},setpts=N,{output_format}[out0]",
        sources.join(";"),
        graph
    );
    if text.len() > CONTROL_BYTES {
        bail!("picture filter control exceeds byte budget");
    }
    let mut file = tempfile::Builder::new()
        .prefix(".picture-filter-")
        .tempfile_in(root)?;
    file.write_all(text.as_bytes())?;
    file.flush()?;
    let args = vec![
        "-f".into(),
        "lavfi".into(),
        "-graph_file".into(),
        arg(&file.path().canonicalize()?),
        "-i".into(),
        "unused".into(),
        "-map".into(),
        "0:v:0".into(),
        "-an".into(),
    ];
    Ok((args, file))
}

pub(crate) fn prepare(
    job: &Job,
    plan: &Plan,
    inputs: &(impl InputFiles + ?Sized),
    root: &Path,
) -> Result<(Vec<String>, Option<tempfile::NamedTempFile>)> {
    let args = picture_filter_command(job, plan, inputs)?;
    if args
        .iter()
        .try_fold(0usize, |n, a| n.checked_add(a.len() + 1))
        .is_some_and(|n| n <= INLINE_BYTES)
    {
        return Ok((args, None));
    }
    let format = output_format(&inputs.media_program(MediaProgram::Ffmpeg)?)?;
    let clock = format!("{}/{}", plan.fps_numerator, plan.fps_denominator);
    let (args, file) = external(&args, root, &format, &clock)?;
    Ok((args, Some(file)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_options_reject_control_injection_and_escape_both_parsers() {
        for c in ['\n', '\r', '\0', '\t'] {
            assert!(option(&format!("file{c}foreign")).is_err());
        }
        assert_eq!(
            option("D:/a,b;[c]'d").unwrap(),
            "D\\\\:/a\\,b\\;\\[c\\]\\\\\\'d"
        );
    }

    fn command(paths: &[(PathBuf, bool)], counts: &[usize]) -> Vec<String> {
        command_at(paths, counts, "25/1")
    }
    fn command_at(paths: &[(PathBuf, bool)], counts: &[usize], fps: &str) -> Vec<String> {
        let mut args = Vec::new();
        let mut filters = Vec::new();
        for (i, ((path, still), count)) in paths.iter().zip(counts).enumerate() {
            if *still {
                args.extend(["-loop".into(), "1".into(), "-framerate".into(), fps.into()]);
            }
            args.extend(["-i".into(), arg(path)]);
            let start = if *still { 0 } else { 2 };
            filters.push(format!("[{i}:v]scale=8:8,setsar=1,fps={fps},trim=start_frame={start}:end_frame={},setpts=PTS-STARTPTS[v{i}]",start+count));
        }
        filters.push(format!(
            "{}concat=n={}:v=1:a=0[v]",
            (0..paths.len())
                .map(|i| format!("[v{i}]"))
                .collect::<String>(),
            paths.len()
        ));
        args.extend([
            "-filter_complex".into(),
            filters.join(";"),
            "-map".into(),
            "[v]".into(),
            "-an".into(),
        ]);
        args
    }

    fn program() -> std::ffi::OsString {
        std::env::var_os("REEL_TEST_FFMPEG").unwrap_or_else(|| "ffmpeg".into())
    }
    fn frames(args: &[String]) -> Result<Vec<u8>> {
        frames_at(args, "25/1")
    }
    fn frames_at(args: &[String], fps: &str) -> Result<Vec<u8>> {
        let result = Command::new(program())
            .args([
                "-hide_banner",
                "-v",
                "error",
                "-nostdin",
                "-threads",
                "1",
                "-filter_complex_threads",
                "1",
                "-filter_threads",
                "1",
            ])
            .args(args)
            .args(["-r", fps])
            .args([
                "-c:v",
                "rawvideo",
                "-threads:v",
                "1",
                "-pix_fmt",
                "yuv444p",
                "-f",
                "framemd5",
                "pipe:1",
            ])
            .output()?;
        if !result.status.success() {
            bail!(
                "native picture fixture failed: {}",
                String::from_utf8_lossy(&result.stderr)
            );
        }
        Ok(result.stdout)
    }
    #[test]
    #[ignore = "requires native FFmpeg; exact inline/file picture, clock and long-path comparison"]
    fn native_file_controls_preserve_frames_clocks_and_large_picture_jobs() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let root = temp.path();
        let format = output_format(Path::new(&program()))?;
        let a = root.join("input ',;[] a.png");
        let b = root.join("input ',;[] b.png");
        image::RgbImage::from_pixel(8, 8, image::Rgb([255, 0, 0])).save(&a)?;
        image::RgbImage::from_pixel(8, 8, image::Rgb([0, 0, 255])).save(&b)?;
        let video = root.join("offset-video.mkv");
        let made = Command::new(program())
            .args([
                "-hide_banner",
                "-v",
                "error",
                "-nostdin",
                "-f",
                "lavfi",
                "-i",
                "testsrc=size=8x8:rate=25",
                "-vf",
                "setpts=PTS+0.4/TB",
                "-frames:v",
                "8",
                "-c:v",
                "ffv1",
            ])
            .arg(&video)
            .output()?;
        anyhow::ensure!(
            made.status.success(),
            "native offset fixture failed: {}",
            String::from_utf8_lossy(&made.stderr)
        );
        for fps in ["25/1", "24/1", "30000/1001"] {
            let args = command_at(
                &[(a.clone(), true), (b.clone(), true), (video.clone(), false)],
                &[2, 3, 3],
                fps,
            );
            let baseline = frames_at(&args, fps)?;
            assert_eq!(
                String::from_utf8_lossy(&baseline)
                    .lines()
                    .filter(|line| !line.starts_with('#'))
                    .count(),
                8
            );
            let (bounded, file) = external(&args, root, &format, fps)?;
            assert_eq!(
                frames_at(&bounded, fps)?,
                baseline,
                "all decoded frames and timestamps must match at {fps}"
            );
            let control = file.path().to_owned();
            drop(file);
            assert!(!control.exists());
        }
        let count = 236;
        let paths = (0..count)
            .map(|i| (if i % 2 == 0 { a.clone() } else { b.clone() }, true))
            .collect::<Vec<_>>();
        let large = command(&paths, &vec![1; count]);
        assert!(large.iter().map(String::len).sum::<usize>() > 32767);
        let (bounded, file) = external(&large, root, &format, "25/1")?;
        assert!(bounded.iter().map(String::len).sum::<usize>() < 1024);
        let actual = String::from_utf8(frames(&bounded)?)?;
        let rows = actual
            .lines()
            .filter(|line| !line.starts_with('#'))
            .collect::<Vec<_>>();
        assert_eq!(rows.len(), count);
        let red = String::from_utf8(frames(&command(&[(a, true)], &[1]))?)?;
        let blue = String::from_utf8(frames(&command(&[(b, true)], &[1]))?)?;
        let hash = |text: &str| {
            text.lines()
                .find(|line| !line.starts_with('#'))
                .unwrap()
                .rsplit(',')
                .next()
                .unwrap()
                .trim()
                .to_owned()
        };
        let red = hash(&red);
        let blue = hash(&blue);
        assert_ne!(red, blue);
        for (i, row) in rows.iter().enumerate() {
            let fields = row.split(',').map(str::trim).collect::<Vec<_>>();
            assert_eq!(fields[1].parse::<usize>()?, i);
            assert_eq!(fields[2].parse::<usize>()?, i);
            assert_eq!(fields[3], "1");
            assert_eq!(
                row.rsplit(',').next().unwrap().trim(),
                if i % 2 == 0 { &red } else { &blue }
            );
        }
        let control = file.path().to_owned();
        drop(file);
        assert!(!control.exists());
        assert_eq!(
            fs::read_dir(root)?.count(),
            3,
            "no copied inputs or leaked controls"
        );
        Ok(())
    }
}
