use crate::cli::ScanArgs;
use crate::core::models::FileEntry;
use anyhow::Result;
use std::collections::HashMap;
use std::time::UNIX_EPOCH;
use walkdir::{DirEntry, WalkDir};

pub fn run(args: ScanArgs) -> Result<()> {
    let mut scanned_files: u64 = 0;
    let mut by_size: HashMap<u64, Vec<FileEntry>> = HashMap::new();

    for root in &args.paths {
        let walker = WalkDir::new(root)
            .follow_links(args.follow_symlinks)
            .into_iter()
            .filter_entry(|entry| !is_ignored(entry, &args.ignores));

        for entry in walker {
            let entry = match entry {
                Ok(entry) => entry,
                Err(err) => {
                    eprintln!("warn: skipping entry due to walk error: {err}");
                    continue;
                }
            };

            if !entry.file_type().is_file() {
                continue;
            }

            scanned_files += 1;

            let metadata = match entry.metadata() {
                Ok(meta) => meta,
                Err(err) => {
                    eprintln!(
                        "warn: unable to read metadata for {}: {err}",
                        entry.path().display()
                    );
                    continue;
                }
            };

            let size = metadata.len();
            if size < args.min_size {
                continue;
            }

            let modified_unix_secs = metadata
                .modified()
                .ok()
                .and_then(|mtime| mtime.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_secs());

            let file_entry = FileEntry {
                path: entry.path().to_path_buf(),
                size,
                modified_unix_secs,
            };
            by_size.entry(size).or_default().push(file_entry);
        }
    }

    let candidate_files = by_size
        .values()
        .filter(|group| group.len() > 1)
        .map(|group| group.len() as u64)
        .sum::<u64>();
    let candidate_groups = by_size.values().filter(|group| group.len() > 1).count() as u64;
    let candidate_bytes = by_size
        .iter()
        .filter(|(_, group)| group.len() > 1)
        .map(|(size, group)| size * group.len() as u64)
        .sum::<u64>();

    println!("scan roots: {:?}", args.paths);
    println!("scanned files: {}", scanned_files);
    println!("size-candidate groups: {}", candidate_groups);
    println!("size-candidate files: {}", candidate_files);
    println!("size-candidate bytes: {}", candidate_bytes);
    println!("next step: hashing files inside same-size groups");

    if args.json.is_some() || args.csv.is_some() {
        println!("note: --json/--csv export will be implemented in a later step.");
    }

    Ok(())
}

fn is_ignored(entry: &DirEntry, ignores: &[String]) -> bool {
    if ignores.is_empty() {
        return false;
    }

    let path_text = entry.path().to_string_lossy();
    ignores.iter().any(|needle| path_text.contains(needle))
}
