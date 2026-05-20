use std::collections::HashMap;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};

use crate::media::MediaItem;
use crate::process::wait_status_with_timeout;

pub trait CommandRunner: Send + Sync {
    fn run(&self, program: &str, args: &[OsString]) -> Result<()>;
}

pub struct RealCommandRunner;

impl CommandRunner for RealCommandRunner {
    fn run(&self, program: &str, args: &[OsString]) -> Result<()> {
        let child = Command::new(program)
            .args(args)
            .stdin(Stdio::null())
            .spawn()
            .with_context(|| format!("failed to execute {program}"))?;
        let status = wait_status_with_timeout(child, Duration::from_secs(30 * 60))?;
        if !status.success() {
            anyhow::bail!("{program} failed with status {status}");
        }
        Ok(())
    }
}

pub fn cache_key(path: &Path, size: u64, mtime: SystemTime) -> String {
    let mut hasher = Sha256::new();
    hasher.update(path.as_os_str().as_encoded_bytes());
    hasher.update([0]);
    hasher.update(size.to_le_bytes());
    hasher.update([0]);
    let duration = mtime.duration_since(UNIX_EPOCH).unwrap_or_default();
    hasher.update(duration.as_secs().to_le_bytes());
    hasher.update(duration.subsec_nanos().to_le_bytes());
    let digest = hasher.finalize();

    // Bolt Optimization: ⚡ Fast Hex Encoding
    // We avoid allocating a string per byte using `format!("{byte:02x}")`
    // and collect, reducing allocations from 32 to 1 and speeding up hash computation.
    let mut result = String::with_capacity(64);
    for byte in digest.as_slice() {
        let hex = b"0123456789abcdef";
        result.push(hex[(byte >> 4) as usize] as char);
        result.push(hex[(byte & 0xf) as usize] as char);
    }
    result
}

pub struct TranscodeCache {
    cache_dir: PathBuf,
    runner: Arc<dyn CommandRunner>,
    locks: Mutex<HashMap<String, Weak<Mutex<()>>>>,
}

impl TranscodeCache {
    pub fn new(cache_dir: PathBuf, runner: Arc<dyn CommandRunner>) -> Self {
        Self {
            cache_dir,
            runner,
            locks: Mutex::new(HashMap::new()),
        }
    }

    pub fn cached_path(&self, item: &MediaItem) -> PathBuf {
        self.cache_dir.join(format!(
            "{}.mkv",
            cache_key(&item.source_path, item.size, item.mtime)
        ))
    }

    pub fn cached_path_if_exists(&self, item: &MediaItem) -> Option<PathBuf> {
        let path = self.cached_path(item);
        path.is_file().then_some(path)
    }

    pub fn materialize(&self, item: &MediaItem) -> Result<PathBuf> {
        fs::create_dir_all(&self.cache_dir)
            .with_context(|| format!("failed to create cache dir {}", self.cache_dir.display()))?;
        let key = cache_key(&item.source_path, item.size, item.mtime);
        let output = self.cached_path(item);
        if output.is_file() {
            return Ok(output);
        }

        let lock = self.lock_for_key(&key);
        let _guard = lock.lock().expect("cache lock poisoned");
        if output.is_file() {
            return Ok(output);
        }

        let scratch_suffix = format!(
            "{}.{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        );
        let cover = self
            .cache_dir
            .join(format!(".{key}.{scratch_suffix}.cover.jpg"));
        let black = self
            .cache_dir
            .join(format!(".{key}.{scratch_suffix}.black.jpg"));
        let tmp_output = self
            .cache_dir
            .join(format!(".{key}.{scratch_suffix}.tmp.mkv"));

        let result = (|| -> Result<()> {
            let cover_result = self.runner.run(
                "ffmpeg",
                &[
                    "-nostdin".into(),
                    "-y".into(),
                    "-i".into(),
                    item.source_path.as_os_str().to_os_string(),
                    "-an".into(),
                    "-vcodec".into(),
                    "copy".into(),
                    cover.as_os_str().to_os_string(),
                ],
            );

            let image = if cover_result.is_ok() {
                cover.clone()
            } else {
                self.runner.run(
                    "ffmpeg",
                    &[
                        "-nostdin".into(),
                        "-y".into(),
                        "-f".into(),
                        "lavfi".into(),
                        "-i".into(),
                        "color=c=black:s=1920x1080:r=1".into(),
                        "-frames:v".into(),
                        "1".into(),
                        black.as_os_str().to_os_string(),
                    ],
                )?;
                black.clone()
            };

            self.runner.run(
                "ffmpeg",
                &[
                    "-nostdin".into(),
                    "-y".into(),
                    "-loop".into(),
                    "1".into(),
                    "-i".into(),
                    image.as_os_str().to_os_string(),
                    "-i".into(),
                    item.source_path.as_os_str().to_os_string(),
                    "-map".into(),
                    "0:v:0".into(),
                    "-map".into(),
                    "1:a:0".into(),
                    "-c:v".into(),
                    "libx264".into(),
                    "-tune".into(),
                    "stillimage".into(),
                    "-vf".into(),
                    "scale=trunc(iw/2)*2:trunc(ih/2)*2".into(),
                    "-pix_fmt".into(),
                    "yuv420p".into(),
                    "-c:a".into(),
                    "copy".into(),
                    "-shortest".into(),
                    tmp_output.as_os_str().to_os_string(),
                ],
            )?;

            fs::rename(&tmp_output, &output).with_context(|| {
                format!(
                    "failed to atomically move {} to {}",
                    tmp_output.display(),
                    output.display()
                )
            })?;
            Ok(())
        })();

        let _ = fs::remove_file(cover);
        let _ = fs::remove_file(black);
        let _ = fs::remove_file(tmp_output);
        result?;
        Ok(output)
    }

    fn lock_for_key(&self, key: &str) -> Arc<Mutex<()>> {
        let mut locks = self.locks.lock().expect("cache lock map poisoned");
        locks.retain(|_, lock| lock.strong_count() > 0);
        if let Some(lock) = locks.get(key).and_then(Weak::upgrade) {
            return lock;
        }

        let lock = Arc::new(Mutex::new(()));
        locks.insert(key.to_string(), Arc::downgrade(&lock));
        lock
    }

    #[cfg(test)]
    fn live_lock_count(&self) -> usize {
        let locks = self.locks.lock().expect("cache lock map poisoned");
        locks
            .values()
            .filter(|lock| lock.strong_count() > 0)
            .count()
    }
}

impl crate::fs::CacheProvider for TranscodeCache {
    fn ensure_cached(&self, item: &MediaItem) -> Result<PathBuf> {
        self.materialize(item)
    }

    fn cached_path_if_exists(&self, item: &MediaItem) -> Option<PathBuf> {
        TranscodeCache::cached_path_if_exists(self, item)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;
    use std::sync::Mutex;
    use std::time::{Duration, UNIX_EPOCH};

    use tempfile::TempDir;

    struct RecordingRunner {
        commands: Mutex<Vec<(String, Vec<OsString>)>>,
    }

    impl CommandRunner for RecordingRunner {
        fn run(&self, program: &str, args: &[OsString]) -> Result<()> {
            self.commands
                .lock()
                .unwrap()
                .push((program.to_string(), args.to_vec()));
            if self.commands.lock().unwrap().len() == 1 {
                anyhow::bail!("no cover art")
            }
            if let Some(output) = args.last()
                && output.to_string_lossy().ends_with(".tmp.mkv")
            {
                std::fs::write(output, b"mkv")?;
            }
            Ok(())
        }
    }

    fn item(path: PathBuf, size: u64, mtime: SystemTime) -> MediaItem {
        MediaItem {
            source_path: path,
            relative_m4a: PathBuf::from("song.m4a"),
            virtual_path: PathBuf::from("song.mkv"),
            size,
            mtime,
        }
    }

    #[test]
    fn cache_key_changes_with_path_size_or_mtime() {
        let mtime = UNIX_EPOCH + Duration::from_secs(10);
        let base = cache_key(Path::new("/media/a.m4a"), 100, mtime);
        assert_ne!(base, cache_key(Path::new("/media/b.m4a"), 100, mtime));
        assert_ne!(base, cache_key(Path::new("/media/a.m4a"), 101, mtime));
        assert_ne!(
            base,
            cache_key(
                Path::new("/media/a.m4a"),
                100,
                mtime + Duration::from_secs(1)
            )
        );
        assert_eq!(base.len(), 64);
    }

    #[test]
    fn materialize_uses_black_fallback_then_muxes_when_cover_missing() -> Result<()> {
        let cache_dir = TempDir::new()?;
        let runner = std::sync::Arc::new(RecordingRunner {
            commands: Mutex::new(Vec::new()),
        });
        let cache = TranscodeCache::new(cache_dir.path().to_path_buf(), runner.clone());
        let source = cache_dir.path().join("track.m4a");
        std::fs::write(&source, b"source")?;
        let media = item(source, 6, UNIX_EPOCH + Duration::from_secs(1));

        let output = cache.materialize(&media)?;

        assert_eq!(output.extension().and_then(|ext| ext.to_str()), Some("mkv"));
        let commands = runner.commands.lock().unwrap();
        assert_eq!(commands.len(), 3);
        assert_eq!(commands[0].0, "ffmpeg");
        assert!(commands[0].1.iter().any(|arg| arg == "-vcodec"));
        assert!(
            commands[1]
                .1
                .iter()
                .any(|arg| arg.to_string_lossy().contains("color=c=black"))
        );
        assert!(commands[2].1.iter().any(|arg| arg == "-shortest"));
        assert!(commands[2].1.iter().any(|arg| arg == "-vf"));
        assert!(
            commands[2]
                .1
                .iter()
                .any(|arg| arg.to_string_lossy().contains("trunc(iw/2)*2"))
        );
        Ok(())
    }

    #[test]
    fn lock_table_does_not_retain_strong_locks_after_use() {
        let cache = TranscodeCache::new(
            PathBuf::from("/tmp/m4a-atmos-test-cache"),
            std::sync::Arc::new(RecordingRunner {
                commands: Mutex::new(Vec::new()),
            }),
        );

        let lock = cache.lock_for_key("track");
        assert_eq!(cache.live_lock_count(), 1);

        drop(lock);

        assert_eq!(cache.live_lock_count(), 0);
    }
}
