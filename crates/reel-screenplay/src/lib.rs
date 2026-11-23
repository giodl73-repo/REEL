//! Explicit pinned-source development review. No inferred duration or casting approval.
use anyhow::{Context, Result, ensure};
use regex::Regex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{Read, Write},
    path::{Component, Path},
};

pub const SCHEMA: &str = "reel.screenplay-session.v1";
pub const HTML: &[u8] = include_bytes!("review.html");
pub const JS: &[u8] = include_bytes!("review.js");
type BilingualOrder = (Vec<(String, String)>, Vec<String>);

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Character {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub generic: bool,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub language: String,
    pub path: String,
    pub sha256: String,
    pub bytes: u64,
    pub cues: BTreeMap<String, Character>,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Episode {
    pub id: String,
    pub season: u32,
    pub number: u32,
    pub sources: Vec<Source>,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema: String,
    pub show_id: String,
    pub title: String,
    pub revision: String,
    pub source_commit: String,
    pub notice: String,
    pub languages: Vec<String>,
    pub episodes: Vec<Episode>,
    #[serde(default)]
    pub visual_roles: BTreeMap<String, String>,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Turn {
    pub episode: String,
    pub season: u32,
    pub language: String,
    pub character_id: String,
    pub character: String,
    pub generic_role: bool,
    pub cue: String,
    pub scene: String,
    pub source_line: usize,
    pub dialogue_lines: usize,
    pub spoken_words: usize,
}
#[derive(Debug, Serialize)]
pub struct Scene {
    pub episode: String,
    pub language: String,
    pub id: String,
    pub heading: String,
    pub source_line: usize,
}
#[derive(Debug, Serialize)]
pub struct Session {
    pub schema: String,
    pub request: Request,
    pub turns: Vec<Turn>,
    pub scenes: Vec<Scene>,
    pub bilingual_structure_checked: bool,
}

fn safe_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 100
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}
fn relative(s: &str) -> bool {
    !s.is_empty()
        && !s.contains(['\\', ':'])
        && Path::new(s)
            .components()
            .all(|c| matches!(c, Component::Normal(_)))
}
fn read_pin(root: &Path, source: &Source) -> Result<String> {
    ensure!(
        relative(&source.path),
        "Source must be a relative ordinary path"
    );
    let root = root.canonicalize()?;
    let path = root.join(&source.path).canonicalize()?;
    ensure!(path.starts_with(&root), "Source escapes the explicit root");
    ensure!(source.bytes <= 1024 * 1024, "Source exceeds 1 MiB");
    let mut bytes = vec![];
    fs::File::open(path)?
        .take(source.bytes + 1)
        .read_to_end(&mut bytes)?;
    let hash: String = Sha256::digest(&bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    ensure!(
        bytes.len() as u64 == source.bytes && hash == source.sha256,
        "Source pin differs: {}",
        source.path
    );
    Ok(String::from_utf8(bytes)?)
}

/// Bounded Fountain subset: uppercase/forced cues, numbered sluglines, blank-ended
/// dialogue, standalone parentheticals, scene IDs. Unsupported dual dialogue,
/// boneyards and notes refuse instead of silently affecting counts.
pub fn parse(text: &str, episode: &Episode, source: &Source) -> Result<(Vec<Turn>, Vec<Scene>)> {
    ensure!(
        !text.contains("/*") && !text.contains("[["),
        "Boneyards/notes require an explicit supported preprocessing edition"
    );
    let cue_re = Regex::new(r"^[\p{Lu}][\p{Lu}\p{M}0-9 .'’()/-]*$")?;
    let word_re = Regex::new(r"[\p{L}\p{N}]+(?:['’-][\p{L}\p{N}]+)*")?;
    let heading_re = Regex::new(r"^(?:INT\.|EXT\.|INT/EXT\.|I/E\.|\.(?:[^.]))")?;
    let lines: Vec<_> = text.lines().collect();
    let mut turns = vec![];
    let mut scenes = vec![];
    let mut scene = String::new();
    let mut ids = BTreeSet::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i].trim();
        if heading_re.is_match(line) {
            let heading = if let Some(end) = line.strip_suffix('#') {
                let (heading, id) = end.rsplit_once('#').context("Missing scene ID")?;
                ensure!(
                    !id.is_empty() && ids.insert(id.to_owned()),
                    "Empty/duplicate scene ID"
                );
                scene = id.to_owned();
                heading
            } else {
                ensure!(
                    !scene.is_empty(),
                    "First scene needs an explicit #scene-id#"
                );
                line
            };
            scenes.push(Scene {
                episode: episode.id.clone(),
                language: source.language.clone(),
                id: scene.clone(),
                heading: heading.trim().to_owned(),
                source_line: i + 1,
            });
        } else if !scene.is_empty()
            && (i == 0 || lines[i - 1].trim().is_empty())
            && !line.is_empty()
            && i + 1 < lines.len()
            && !lines[i + 1].trim().is_empty()
        {
            ensure!(
                !line.ends_with('^'),
                "Dual dialogue requires a supported edition"
            );
            let forced = line.strip_prefix('@');
            let cue = forced.unwrap_or(line);
            if forced.is_some() || cue_re.is_match(cue) {
                ensure!(
                    !cue.contains("V.O.")
                        && cue != "NARRATOR"
                        && cue != "NARRADOR"
                        && cue != "NARRADORA",
                    "Narrator/voiceover cue: {cue}"
                );
                let character = source.cues.get(cue).with_context(|| {
                    format!(
                        "Unmapped cue {cue} in {} {}:{}",
                        episode.id,
                        source.language,
                        i + 1
                    )
                })?;
                let mut dialogue_lines = 0;
                let mut spoken_words = 0;
                let start = i + 2;
                i += 1;
                while i < lines.len() && !lines[i].trim().is_empty() {
                    let value = lines[i].trim();
                    if !(value.starts_with('(') && value.ends_with(')')) {
                        dialogue_lines += 1;
                        spoken_words += word_re.find_iter(value).count();
                    }
                    i += 1;
                }
                ensure!(dialogue_lines > 0, "Empty dialogue at {start}");
                turns.push(Turn {
                    episode: episode.id.clone(),
                    season: episode.season,
                    language: source.language.clone(),
                    character_id: character.id.clone(),
                    character: character.name.clone(),
                    generic_role: character.generic,
                    cue: cue.to_owned(),
                    scene: scene.clone(),
                    source_line: start,
                    dialogue_lines,
                    spoken_words,
                });
                continue;
            }
        }
        i += 1;
    }
    ensure!(!scenes.is_empty() && !turns.is_empty(), "Empty screenplay");
    let used: BTreeSet<_> = turns.iter().map(|t| t.cue.as_str()).collect();
    ensure!(
        source.cues.keys().all(|c| used.contains(c.as_str())),
        "Unused cue mapping"
    );
    Ok((turns, scenes))
}

pub fn analyze(root: &Path, request: Request) -> Result<(Session, BTreeMap<String, String>)> {
    ensure!(
        request.schema == "reel.screenplay-review.v1",
        "Unsupported request schema"
    );
    ensure!(
        safe_id(&request.show_id) && safe_id(&request.revision),
        "Unsafe show/revision ID"
    );
    ensure!(
        request.source_commit.len() == 40
            && request.source_commit.bytes().all(|b| b.is_ascii_hexdigit()),
        "Exact source commit required"
    );
    ensure!(
        !request.languages.is_empty() && request.languages.len() <= 4,
        "1–4 languages required"
    );
    let languages: BTreeSet<_> = request.languages.iter().collect();
    ensure!(
        languages.len() == request.languages.len() && languages.iter().all(|l| safe_id(l)),
        "Invalid languages"
    );
    ensure!(
        !request.episodes.is_empty() && request.episodes.len() <= 100,
        "1–100 episodes required"
    );
    let mut keys = BTreeSet::new();
    let mut slots = BTreeSet::new();
    let mut turns = vec![];
    let mut scenes = vec![];
    let mut manuscripts = BTreeMap::new();
    let mut total_input = 0;
    for language in &request.languages {
        manuscripts.insert(
            language.clone(),
            format!(
                "{}\n{}\n{}\n\n{}\n\n",
                request.title, request.revision, language, request.notice
            ),
        );
    }
    for episode in &request.episodes {
        ensure!(
            safe_id(&episode.id)
                && episode.season > 0
                && episode.number > 0
                && keys.insert(&episode.id)
                && slots.insert((episode.season, episode.number)),
            "Duplicate/invalid episode"
        );
        ensure!(episode.sources.len() == languages.len(), "Missing language");
        let mut seen = BTreeSet::new();
        let mut reference: Option<BilingualOrder> = None;
        for source in &episode.sources {
            ensure!(
                languages.contains(&source.language) && seen.insert(&source.language),
                "Unexpected/duplicate language"
            );
            let text = read_pin(root, source)?;
            total_input += text.len();
            ensure!(
                total_input <= 32 * 1024 * 1024,
                "Combined inputs exceed 32 MiB"
            );
            let (parsed, scene_list) = parse(&text, episode, source)?;
            let correspondence = (
                parsed
                    .iter()
                    .map(|t| (t.scene.clone(), t.character_id.clone()))
                    .collect(),
                scene_list.iter().map(|s| s.id.clone()).collect(),
            );
            if let Some(expected) = &reference {
                ensure!(
                    expected == &correspondence,
                    "Bilingual scene/turn order differs: {}",
                    episode.id
                );
            } else {
                reference = Some(correspondence);
            }
            manuscripts
                .get_mut(&source.language)
                .unwrap()
                .push_str(&format!(
                    "\n\n=== {} / SEASON {} / EPISODE {} ===\n\n{}\n",
                    episode.id, episode.season, episode.number, text
                ));
            turns.extend(parsed);
            scenes.extend(scene_list);
        }
    }
    Ok((
        Session {
            schema: SCHEMA.into(),
            request,
            turns,
            scenes,
            bilingual_structure_checked: true,
        },
        manuscripts,
    ))
}

/// New directory only. Exports are immutable; later revisions use new directories.
pub fn export(root: &Path, request_path: &Path, output: &Path) -> Result<Session> {
    let request_bytes = fs::read(request_path)?;
    ensure!(request_bytes.len() <= 256 * 1024, "Request exceeds 256 KiB");
    let mut value: serde_json::Value = serde_json::from_slice(&request_bytes)?;
    let base = request_path
        .parent()
        .context("Request has no parent")?
        .canonicalize()?;
    let entries = value["episodes"]
        .as_array_mut()
        .context("Missing episodes")?;
    ensure!(entries.len() <= 100, "Too many episode references");
    for entry in entries {
        if let Some(path) = entry.get("path").and_then(|v| v.as_str()) {
            ensure!(
                entry.as_object().is_some_and(|o| o.len() == 1) && relative(path),
                "Episode reference must contain only a relative path"
            );
            let path = base.join(path).canonicalize()?;
            ensure!(
                path.starts_with(&base),
                "Episode reference escapes request directory"
            );
            let bytes = fs::read(path)?;
            ensure!(
                bytes.len() <= 64 * 1024,
                "Episode definition exceeds 64 KiB"
            );
            *entry = serde_json::from_slice(&bytes)?;
        }
    }
    let request: Request = serde_json::from_value(value)?;
    let (session, manuscripts) = analyze(root, request)?;
    let data = serde_json::to_vec(&session)?;
    ensure!(
        data.len() <= 2 * 1024 * 1024,
        "Session exceeds host control budget"
    );
    fs::create_dir(output).context("Output must be a new directory beneath an existing parent")?;
    let write = |name: &str, bytes: &[u8]| -> Result<()> {
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(output.join(name))?
            .write_all(bytes)?;
        Ok(())
    };
    write("index.html", HTML)?;
    write("editor.js", JS)?;
    write("session.json", &data)?;
    write("request.json", &request_bytes)?;
    for (language, text) in manuscripts {
        write(
            &format!(
                "{}-{}-{}.txt",
                session.request.show_id, language, session.request.revision
            ),
            text.as_bytes(),
        )?;
    }
    Ok(session)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sample() -> (Episode, Source) {
        let source = Source {
            language: "en".into(),
            path: "one.fountain".into(),
            sha256: String::new(),
            bytes: 0,
            cues: BTreeMap::from([(
                "ANA".into(),
                Character {
                    id: "ana".into(),
                    name: "Ana".into(),
                    generic: false,
                },
            )]),
        };
        (
            Episode {
                id: "E1".into(),
                season: 1,
                number: 1,
                sources: vec![],
            },
            source,
        )
    }
    #[test]
    fn unicode_words_coordinates_and_parenthetical() {
        let (ep, source) = sample();
        let (turns, scenes) = parse("Title: Test\n\nEXT. DOCK - DAY #one#\n\nANA\n(quietly)\nMaría's caña — aquí.\nSecond line.\n\nShe leaves.", &ep, &source).unwrap();
        assert_eq!(
            (
                turns.len(),
                turns[0].source_line,
                turns[0].dialogue_lines,
                turns[0].spoken_words
            ),
            (1, 6, 2, 5)
        );
        assert_eq!(scenes[0].source_line, 3);
    }
    #[test]
    fn rejects_unsupported_or_unmapped_content() {
        let (ep, source) = sample();
        for text in [
            "EXT. DOCK #one#\n\nANA ^\nHello.",
            "EXT. DOCK #one#\n\nBOB\nHello.",
            "/* omitted */",
            "EXT. DOCK\n\nANA\nHi.",
            "EXT. DOCK #one#\n\nANA\nHi.\n\nEXT. ROAD #one#",
            "EXT. DOCK #one#\n\nNARRATOR\nHi.",
        ] {
            assert!(parse(text, &ep, &source).is_err(), "{text}");
        }
    }
    #[test]
    fn refuses_escape_and_stale_pins() {
        let dir = tempfile::tempdir().unwrap();
        let (_, mut source) = sample();
        source.path = "../escape".into();
        assert!(read_pin(dir.path(), &source).is_err());
        source.path = "one.fountain".into();
        fs::write(dir.path().join(&source.path), "changed").unwrap();
        assert!(read_pin(dir.path(), &source).is_err());
        assert!(!relative("C:/outside"));
        assert!(!safe_id("../../name"));
    }

    fn bilingual_fixture(root: &Path) -> Request {
        let (mut episode, _) = sample();
        for (language, cue, text) in [("en", "ANA", "Hello there."), ("es", "ANITA", "Hola.")] {
            let path = format!("{language}.fountain");
            let contents =
                format!("EXT. DOCK #one#\n\n@{cue}\n{text}\n\nINT. ROOM - DAY\n\n@{cue}\n{text}\n");
            fs::write(root.join(&path), &contents).unwrap();
            let sha256 = Sha256::digest(contents.as_bytes())
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect();
            episode.sources.push(Source {
                language: language.into(),
                path,
                sha256,
                bytes: contents.len() as u64,
                cues: BTreeMap::from([(
                    cue.into(),
                    Character {
                        id: "ana".into(),
                        name: "Ana".into(),
                        generic: false,
                    },
                )]),
            });
        }
        Request {
            schema: "reel.screenplay-review.v1".into(),
            show_id: "sample".into(),
            title: "Sample".into(),
            revision: "r1".into(),
            source_commit: "a".repeat(40),
            notice: "Development only".into(),
            languages: vec!["en".into(), "es".into()],
            episodes: vec![episode],
            visual_roles: BTreeMap::new(),
        }
    }
    #[test]
    fn bilingual_identity_and_secondary_sluglines_are_checked() {
        let root = tempfile::tempdir().unwrap();
        let request = bilingual_fixture(root.path());
        let (session, texts) = analyze(root.path(), request).unwrap();
        assert_eq!(
            (session.turns.len(), session.scenes.len(), texts.len()),
            (4, 4, 2)
        );
        assert_eq!(
            session
                .turns
                .iter()
                .filter(|t| t.language == "en")
                .map(|t| t.spoken_words)
                .sum::<usize>(),
            4
        );
        let mut request = bilingual_fixture(root.path());
        request.episodes[0].sources[1]
            .cues
            .get_mut("ANITA")
            .unwrap()
            .id = "someone-else".into();
        assert!(analyze(root.path(), request).is_err());
    }
    #[test]
    fn export_preserves_sources_and_refuses_overwrite_or_changed_pin() {
        let root = tempfile::tempdir().unwrap();
        let request = bilingual_fixture(root.path());
        let path = root.path().join("request.json");
        fs::write(&path, serde_json::to_vec(&request).unwrap()).unwrap();
        let output = root.path().join("packet");
        export(root.path(), &path, &output).unwrap();
        let original = fs::read(root.path().join("en.fountain")).unwrap();
        assert!(export(root.path(), &path, &output).is_err());
        assert_eq!(fs::read(root.path().join("en.fountain")).unwrap(), original);
        fs::write(root.path().join("en.fountain"), "corrupted").unwrap();
        let failed = root.path().join("must-not-exist");
        assert!(export(root.path(), &path, &failed).is_err());
        assert!(!failed.exists());
    }
}
