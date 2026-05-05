use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result};
use log::warn;
use serde_json::Value;
use walkdir::WalkDir;

use crate::process::wait_output_with_timeout;

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct MediaItem {
    pub source_path: PathBuf,
    pub relative_m4a: PathBuf,
    pub virtual_path: PathBuf,
    pub size: u64,
    pub mtime: SystemTime,
}

pub trait ProbeRunner {
    fn probe_json(&self, path: &Path) -> Result<String>;
}

pub struct RealProbeRunner;

impl ProbeRunner for RealProbeRunner {
    fn probe_json(&self, path: &Path) -> Result<String> {
        let child = Command::new("ffprobe")
            .arg("-nostdin")
            .args(["-v", "quiet", "-print_format", "json"])
            .args(["-show_streams", "-show_format"])
            .arg(path)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| format!("failed to execute ffprobe for {}", path.display()))?;

        let output = wait_output_with_timeout(child, Duration::from_secs(120))
            .with_context(|| format!("ffprobe timed out for {}", path.display()))?;
        if !output.status.success() {
            anyhow::bail!("ffprobe failed for {}", path.display());
        }

        String::from_utf8(output.stdout).context("ffprobe emitted non-UTF-8 JSON")
    }
}

pub fn is_m4a_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("m4a"))
}

pub fn virtual_path_for(source_root: &Path, file: &Path) -> Result<PathBuf> {
    let relative = file
        .strip_prefix(source_root)
        .with_context(|| format!("{} is outside {}", file.display(), source_root.display()))?;
    let mut virtual_path = relative.to_path_buf();
    virtual_path.set_extension("mkv");
    Ok(virtual_path)
}

pub fn unique_virtual_path(path: &Path, existing: &HashSet<PathBuf>) -> PathBuf {
    if !existing.contains(path) {
        return path.to_path_buf();
    }

    let parent = path.parent().unwrap_or_else(|| Path::new(""));
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("track");
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or("mkv");

    for number in 2.. {
        let candidate = parent.join(format!("{stem} ({number}).{extension}"));
        if !existing.contains(&candidate) {
            return candidate;
        }
    }
    unreachable!("unbounded numeric suffix loop always returns")
}

pub fn is_atmos_eac3_probe_json(json: &str) -> bool {
    let Ok(value) = serde_json::from_str::<Value>(json) else {
        return false;
    };

    let has_eac3_audio = value
        .get("streams")
        .and_then(Value::as_array)
        .is_some_and(|streams| {
            streams.iter().any(|stream| {
                stream
                    .get("codec_type")
                    .and_then(Value::as_str)
                    .is_some_and(|kind| kind.eq_ignore_ascii_case("audio"))
                    && stream
                        .get("codec_name")
                        .and_then(Value::as_str)
                        .is_some_and(|codec| codec.eq_ignore_ascii_case("eac3"))
            })
        });

    has_eac3_audio && has_atmos_marker(&value)
}

pub fn scan_source(source_root: &Path, runner: &dyn ProbeRunner) -> Result<Vec<MediaItem>> {
    let source_root = source_root
        .canonicalize()
        .with_context(|| format!("failed to resolve source root {}", source_root.display()))?;
    let mut items = Vec::new();
    let mut virtual_paths = HashSet::new();

    for entry in WalkDir::new(&source_root).follow_links(false) {
        let entry = entry.with_context(|| format!("failed to walk {}", source_root.display()))?;
        if !entry.file_type().is_file() || !is_m4a_path(entry.path()) {
            continue;
        }

        let source_path = entry.path().to_path_buf();
        let probe_json = match runner.probe_json(&source_path) {
            Ok(json) => json,
            Err(error) => {
                warn!(
                    "skipping {} after ffprobe failure: {error:#}",
                    source_path.display()
                );
                continue;
            }
        };
        if !is_atmos_eac3_probe_json(&probe_json) {
            continue;
        }

        let metadata = entry
            .metadata()
            .with_context(|| format!("failed to stat {}", source_path.display()))?;
        let relative_m4a = source_path
            .strip_prefix(&source_root)
            .context("walkdir returned path outside source root")?
            .to_path_buf();
        let virtual_path = unique_virtual_path(
            &virtual_path_for(&source_root, &source_path)?,
            &virtual_paths,
        );
        virtual_paths.insert(virtual_path.clone());

        items.push(MediaItem {
            source_path,
            relative_m4a,
            virtual_path,
            size: metadata.len(),
            mtime: metadata
                .modified()
                .with_context(|| format!("failed to read mtime for {}", entry.path().display()))?,
        });
    }

    items.sort_by(|left, right| left.virtual_path.cmp(&right.virtual_path));
    Ok(items)
}

fn has_atmos_marker(value: &Value) -> bool {
    let stream_marker = value
        .get("streams")
        .and_then(Value::as_array)
        .is_some_and(|streams| streams.iter().any(stream_has_atmos_marker));

    let format_marker = value
        .get("format")
        .and_then(|format| format.get("tags"))
        .is_some_and(value_contains_atmos_marker);

    stream_marker || format_marker
}

fn stream_has_atmos_marker(stream: &Value) -> bool {
    ["profile", "codec_tag_string", "codec_long_name"]
        .iter()
        .any(|field| {
            stream
                .get(field)
                .and_then(Value::as_str)
                .is_some_and(str_contains_atmos_marker)
        })
        || stream.get("tags").is_some_and(value_contains_atmos_marker)
        || stream
            .get("side_data_list")
            .is_some_and(value_contains_atmos_marker)
}

fn value_contains_atmos_marker(value: &Value) -> bool {
    match value {
        Value::String(text) => str_contains_atmos_marker(text),
        Value::Array(values) => values.iter().any(value_contains_atmos_marker),
        Value::Object(values) => values.values().any(value_contains_atmos_marker),
        _ => false,
    }
}

fn str_contains_atmos_marker(text: &str) -> bool {
    text.split(|character: char| !character.is_ascii_alphanumeric())
        .any(|token| token.eq_ignore_ascii_case("joc") || token.eq_ignore_ascii_case("atmos"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::fs;

    use tempfile::TempDir;

    struct FakeProbe {
        responses: HashMap<PathBuf, String>,
    }

    impl ProbeRunner for FakeProbe {
        fn probe_json(&self, path: &Path) -> Result<String> {
            self.responses
                .get(path)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("unexpected probe: {}", path.display()))
        }
    }

    fn atmos_probe() -> String {
        r#"{"streams":[{"codec_type":"audio","codec_name":"eac3","tags":{"title":"Dolby Atmos JOC"}}],"format":{"tags":{"encoder":"atmos"}}}"#.to_string()
    }

    #[test]
    fn m4a_extension_matching_is_case_insensitive() {
        assert!(is_m4a_path(Path::new("track.m4a")));
        assert!(is_m4a_path(Path::new("track.M4A")));
        assert!(!is_m4a_path(Path::new("track.mp4")));
    }

    #[test]
    fn probe_accepts_only_eac3_with_atmos_or_joc_marker() {
        assert!(is_atmos_eac3_probe_json(&atmos_probe()));
        assert!(is_atmos_eac3_probe_json(
            r#"{"streams":[{"codec_type":"audio","codec_name":"eac3","side_data_list":[{"side_data_type":"JOC"}]}]}"#
        ));
        assert!(!is_atmos_eac3_probe_json(
            r#"{"streams":[{"codec_type":"audio","codec_name":"aac","tags":{"title":"Atmos"}}]}"#
        ));
        assert!(!is_atmos_eac3_probe_json(
            r#"{"streams":[{"codec_type":"audio","codec_name":"eac3"}]}"#
        ));
        assert!(!is_atmos_eac3_probe_json(
            r#"{"streams":[{"codec_type":"audio","codec_name":"eac3"}],"format":{"filename":"/music/Atmos Album/Track.m4a"}}"#
        ));
    }

    #[test]
    fn scanner_preserves_relative_layout_and_exposes_mkv() -> Result<()> {
        let tmp = TempDir::new()?;
        let source = tmp.path();
        let album = source.join("Album");
        fs::create_dir_all(&album)?;
        let eligible = album.join("Track One.M4A");
        let ignored = album.join("notes.txt");
        fs::write(&eligible, b"fake")?;
        fs::write(&ignored, b"skip")?;

        let mut responses = HashMap::new();
        responses.insert(eligible.canonicalize()?, atmos_probe());
        let probe = FakeProbe { responses };

        let items = scan_source(source, &probe)?;

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].relative_m4a, PathBuf::from("Album/Track One.M4A"));
        assert_eq!(items[0].virtual_path, PathBuf::from("Album/Track One.mkv"));
        Ok(())
    }

    #[test]
    fn duplicate_virtual_paths_get_numeric_suffixes() {
        let mut existing = HashSet::new();
        let original = PathBuf::from("Album/Track.mkv");
        existing.insert(original.clone());

        let unique = unique_virtual_path(&original, &existing);

        assert_eq!(unique, PathBuf::from("Album/Track (2).mkv"));
    }
}
