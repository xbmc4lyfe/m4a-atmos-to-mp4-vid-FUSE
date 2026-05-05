#[cfg(not(test))]
use std::sync::Arc;

#[cfg(not(test))]
use anyhow::{Context, Result};
#[cfg(not(test))]
use clap::Parser;
#[cfg(not(test))]
use fuser::{Config as FuseConfig, MountOption};
#[cfg(not(test))]
use m4a_atmos_to_mp4_vid_fuse::config::{Cli, Config};
#[cfg(not(test))]
use m4a_atmos_to_mp4_vid_fuse::fs::AtmosFs;
#[cfg(not(test))]
use m4a_atmos_to_mp4_vid_fuse::media::{RealProbeRunner, scan_source};
#[cfg(not(test))]
use m4a_atmos_to_mp4_vid_fuse::transcode::{RealCommandRunner, TranscodeCache};

#[cfg(not(test))]
fn main() -> Result<()> {
    env_logger::init();

    let config = Config::from_cli(Cli::parse())?;

    let items = scan_source(&config.source, &RealProbeRunner)
        .with_context(|| format!("failed to scan source {}", config.source.display()))?;
    log::info!(
        "indexed {} Atmos M4A files from {}; foreground={}",
        items.len(),
        config.source.display(),
        config.foreground
    );

    let cache = Arc::new(TranscodeCache::new(
        config.cache.clone(),
        Arc::new(RealCommandRunner),
    ));
    let fs = AtmosFs::new(items, cache);
    let options = vec![
        MountOption::RO,
        MountOption::FSName("m4a-atmos-fuse".to_string()),
        MountOption::NoDev,
        MountOption::NoSuid,
        MountOption::NoExec,
    ];
    let mut fuse_config = FuseConfig::default();
    fuse_config.mount_options = options;

    fuser::mount2(fs, &config.mount, &fuse_config)
        .with_context(|| format!("failed to mount {}", config.mount.display()))?;
    Ok(())
}

#[cfg(test)]
fn main() {}
