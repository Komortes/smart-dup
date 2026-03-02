use clap::{Args, Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(
    name = "smartdup",
    about = "Fast and safe duplicate finder for files and photos"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    Scan(ScanArgs),
    Delete(DeleteArgs),
    Photos(PhotosArgs),
}

#[derive(Debug, Args)]
pub struct ScanArgs {
    #[arg(required = true)]
    pub paths: Vec<PathBuf>,

    #[arg(long, default_value_t = 1)]
    pub min_size: u64,

    #[arg(long, default_value_t = false)]
    pub follow_symlinks: bool,

    #[arg(long = "ignore")]
    pub ignores: Vec<String>,

    #[arg(long)]
    pub json: Option<PathBuf>,

    #[arg(long)]
    pub csv: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum KeepRule {
    Oldest,
    Newest,
    Lexicographic,
}

#[derive(Debug, Args)]
pub struct DeleteArgs {
    #[arg(long = "from")]
    pub from_json: PathBuf,

    #[arg(long, default_value_t = false)]
    pub dry_run: bool,

    #[arg(long, default_value_t = false)]
    pub interactive: bool,

    #[arg(long, value_enum, default_value_t = KeepRule::Oldest)]
    pub keep: KeepRule,
}

#[derive(Debug, Args)]
pub struct PhotosArgs {
    #[arg(required = true)]
    pub paths: Vec<PathBuf>,

    #[arg(long, default_value_t = false)]
    pub similar: bool,

    #[arg(long, default_value_t = 8)]
    pub threshold: u8,
}
