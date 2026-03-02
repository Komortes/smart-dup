use crate::cli::DeleteArgs;
use anyhow::Result;

pub fn run(args: DeleteArgs) -> Result<()> {
    println!("delete: from={:?}", args.from_json);
    println!(
        "delete: dry_run={} interactive={}",
        args.dry_run, args.interactive
    );
    println!("delete: keep={:?}", args.keep);
    println!("delete command is scaffolded and ready for MVP implementation.");
    Ok(())
}
