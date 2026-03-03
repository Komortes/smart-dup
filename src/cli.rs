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

    /// Minimum file size to consider (examples: 1MB, 512KB, 4096)
    #[arg(long, default_value = "1B", value_parser = parse_size_arg)]
    pub min_size: u64,

    /// Follow symlinked directories
    #[arg(long, default_value_t = false)]
    pub follow_symlinks: bool,

    /// Directory or glob-like fragment to ignore (can be repeated)
    #[arg(long = "ignore")]
    pub ignores: Vec<String>,

    /// Disable built-in ignore directories (.git, node_modules, target)
    #[arg(long, default_value_t = false)]
    pub no_default_ignores: bool,

    /// Number of threads used for parallel hashing
    #[arg(long, value_parser = parse_threads_arg)]
    pub threads: Option<usize>,

    /// Disable progress indicators
    #[arg(long, default_value_t = false)]
    pub no_progress: bool,

    /// Minimal output (summary only)
    #[arg(long, default_value_t = false)]
    pub quiet: bool,

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

    /// Try moving files to Trash before direct delete (default: true on macOS)
    #[arg(long, default_value_t = cfg!(target_os = "macos"))]
    pub trash: bool,

    /// Disable Trash mode and force direct delete
    #[arg(long = "no-trash", default_value_t = false)]
    pub no_trash: bool,

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

fn parse_size_arg(value: &str) -> Result<u64, String> {
    let input = value.trim();
    if input.is_empty() {
        return Err("size cannot be empty".to_string());
    }

    let split_at = input
        .find(|c: char| !c.is_ascii_digit() && c != '_')
        .unwrap_or(input.len());
    let (num_text, suffix_text) = input.split_at(split_at);

    if num_text.is_empty() {
        return Err(format!("invalid size '{value}': expected digits"));
    }

    let normalized_num = num_text.replace('_', "");
    let number = normalized_num
        .parse::<u64>()
        .map_err(|_| format!("invalid size '{value}': number is not a valid u64"))?;

    let suffix = suffix_text.trim().to_ascii_uppercase();
    let multiplier = match suffix.as_str() {
        "" | "B" | "BYTE" | "BYTES" => 1_u64,
        "K" | "KB" | "KIB" => 1024_u64,
        "M" | "MB" | "MIB" => 1024_u64.pow(2),
        "G" | "GB" | "GIB" => 1024_u64.pow(3),
        "T" | "TB" | "TIB" => 1024_u64.pow(4),
        _ => {
            return Err(format!(
                "invalid size unit in '{value}': use B, KB, MB, GB, or TB"
            ));
        }
    };

    number
        .checked_mul(multiplier)
        .ok_or_else(|| format!("size '{value}' is too large"))
}

fn parse_threads_arg(value: &str) -> Result<usize, String> {
    let parsed = value
        .parse::<usize>()
        .map_err(|_| format!("invalid thread count '{value}': expected positive integer"))?;
    if parsed == 0 {
        return Err("invalid thread count '0': must be greater than 0".to_string());
    }
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::{Cli, Commands, parse_size_arg};
    use clap::Parser;

    #[test]
    fn parse_size_accepts_plain_bytes() {
        assert_eq!(parse_size_arg("4096").unwrap(), 4096);
        assert_eq!(parse_size_arg("1B").unwrap(), 1);
    }

    #[test]
    fn parse_size_accepts_units() {
        assert_eq!(parse_size_arg("1KB").unwrap(), 1024);
        assert_eq!(parse_size_arg("2MB").unwrap(), 2 * 1024 * 1024);
        assert_eq!(parse_size_arg("3gb").unwrap(), 3 * 1024 * 1024 * 1024);
    }

    #[test]
    fn parse_size_rejects_invalid_values() {
        assert!(parse_size_arg("").is_err());
        assert!(parse_size_arg("MB").is_err());
        assert!(parse_size_arg("10XB").is_err());
    }

    #[test]
    fn parse_threads_accepts_positive_integer() {
        let cli = Cli::try_parse_from(["smartdup", "scan", "/tmp", "--threads", "4"]).unwrap();
        let Commands::Scan(args) = cli.command else {
            panic!("expected scan command");
        };
        assert_eq!(args.threads, Some(4));
    }

    #[test]
    fn parse_threads_rejects_zero() {
        let parsed = Cli::try_parse_from(["smartdup", "scan", "/tmp", "--threads", "0"]);
        assert!(parsed.is_err());
    }

    #[test]
    fn parse_delete_no_trash_flag() {
        let cli = Cli::try_parse_from([
            "smartdup",
            "delete",
            "--from",
            "/tmp/report.json",
            "--dry-run",
            "--no-trash",
        ])
        .unwrap();
        let Commands::Delete(args) = cli.command else {
            panic!("expected delete command");
        };
        assert!(args.no_trash);
    }

    #[test]
    fn parse_scan_no_progress_and_quiet_flags() {
        let cli =
            Cli::try_parse_from(["smartdup", "scan", "/tmp", "--no-progress", "--quiet"]).unwrap();
        let Commands::Scan(args) = cli.command else {
            panic!("expected scan command");
        };
        assert!(args.no_progress);
        assert!(args.quiet);
    }
}
