use crate::cli::ScanArgs;
use anyhow::Result;

pub fn run(args: ScanArgs) -> Result<()> {
    println!("scan: paths={:?}", args.paths);
    println!("scan: min_size={} bytes", args.min_size);
    println!("scan: follow_symlinks={}", args.follow_symlinks);
    println!("scan: ignores={:?}", args.ignores);
    println!("scan: json={:?} csv={:?}", args.json, args.csv);
    println!("scan command is scaffolded and ready for MVP implementation.");
    Ok(())
}
