use std::sync::Arc;

use anyhow::{Context, Result};
use clap::Parser;
use fuser::{Config as FuseConfig, MountOption};
use m4a_atmos_to_mp4_vid_fuse::config::{Cli, Config};
use m4a_atmos_to_mp4_vid_fuse::fs::AtmosFs;
use m4a_atmos_to_mp4_vid_fuse::media::{RealProbeRunner, scan_source};
use m4a_atmos_to_mp4_vid_fuse::transcode::{RealCommandRunner, TranscodeCache};

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
