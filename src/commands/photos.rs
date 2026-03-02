use crate::cli::PhotosArgs;
use anyhow::Result;

pub fn run(args: PhotosArgs) -> Result<()> {
    println!("photos: paths={:?}", args.paths);
    println!(
        "photos: similar={} threshold={}",
        args.similar, args.threshold
    );
    println!("photos command is a post-MVP placeholder.");
    Ok(())
}
