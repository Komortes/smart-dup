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
    /// Scan one or more paths and find exact duplicate files
    Scan(ScanArgs),
    /// Delete duplicates from a previous scan result
    Delete(DeleteArgs),
    /// Photo-specific commands (MVP placeholder)
    Photos(PhotosArgs),
}

#[derive(Debug, Args)]
pub struct ScanArgs {
    /// Paths to scan recursively
    #[arg(required = true)]
    pub paths: Vec<PathBuf>,

    /// Minimum file size to consider (bytes)
    #[arg(long, default_value_t = 1)]
    pub min_size: u64,

    /// Follow symlinked directories
    #[arg(long, default_value_t = false)]
    pub follow_symlinks: bool,

    /// Directory or glob-like fragment to ignore (can be repeated)
    #[arg(long = "ignore")]
    pub ignores: Vec<String>,

    /// Export scan result to JSON file
    #[arg(long)]
    pub json: Option<PathBuf>,

    /// Export scan result to CSV file
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
    /// Path to JSON report produced by `scan --json`
    #[arg(long = "from")]
    pub from_json: PathBuf,

    /// Do not delete anything, only print planned actions
    #[arg(long, default_value_t = false)]
    pub dry_run: bool,

    /// Ask confirmation before deleting each group
    #[arg(long, default_value_t = false)]
    pub interactive: bool,

    /// Rule for deciding which file to keep
    #[arg(long, value_enum, default_value_t = KeepRule::Oldest)]
    pub keep: KeepRule,
}

#[derive(Debug, Args)]
pub struct PhotosArgs {
    /// Paths with photos to scan
    #[arg(required = true)]
    pub paths: Vec<PathBuf>,

    /// Enable similar image mode (post-MVP)
    #[arg(long, default_value_t = false)]
    pub similar: bool,

    /// Hamming distance threshold for similar mode
    #[arg(long, default_value_t = 8)]
    pub threshold: u8,
}
