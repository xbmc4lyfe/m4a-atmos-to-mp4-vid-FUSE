use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;

#[derive(Debug, Clone, Parser)]
#[command(author, version, about)]
pub struct Cli {
    #[arg(long, default_value = "/mnt/source")]
    pub source: PathBuf,
    #[arg(long, default_value = "/mnt/virtual")]
    pub mount: PathBuf,
    #[arg(long, default_value = "/var/cache/m4a-atmos-fuse")]
    pub cache: PathBuf,
    #[arg(long)]
    pub foreground: bool,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub source: PathBuf,
    pub mount: PathBuf,
    pub cache: PathBuf,
    pub foreground: bool,
}

impl Config {
    pub fn from_cli(cli: Cli) -> Result<Self> {
        if !cli.source.is_dir() {
            anyhow::bail!("source path is not a directory: {}", cli.source.display());
        }

        std::fs::create_dir_all(&cli.mount)
            .with_context(|| format!("failed to create mount dir {}", cli.mount.display()))?;
        std::fs::create_dir_all(&cli.cache)
            .with_context(|| format!("failed to create cache dir {}", cli.cache.display()))?;

        Ok(Self {
            source: cli.source,
            mount: cli.mount,
            cache: cli.cache,
            foreground: cli.foreground,
        })
    }
}
