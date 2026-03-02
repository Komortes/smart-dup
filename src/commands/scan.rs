use crate::cli::ScanArgs;
use crate::core::models::{DuplicateGroup, FileEntry, ScanResult, ScanSummary};
use crate::output::export;
use anyhow::{Context, Result};
use rayon::prelude::*;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};
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

    let mut groups = by_size
        .into_par_iter()
        .filter_map(|(size, files)| {
            if files.len() < 2 {
                return None;
            }
            Some(build_duplicate_groups_for_size(size, files))
        })
        .reduce(Vec::new, |mut acc, mut next| {
            acc.append(&mut next);
            acc
        });

    groups.sort_by(|a, b| {
        b.file_size
            .cmp(&a.file_size)
            .then_with(|| a.content_hash.cmp(&b.content_hash))
    });

    let duplicate_groups = groups.len() as u64;
    let duplicate_files = groups
        .iter()
        .map(|group| group.files.len() as u64)
        .sum::<u64>();
    let reclaimable_bytes = groups
        .iter()
        .map(|group| group.file_size * (group.files.len().saturating_sub(1) as u64))
        .sum::<u64>();

    let result = ScanResult {
        roots: args.paths.clone(),
        generated_at_unix_secs: now_unix_secs(),
        summary: ScanSummary {
            scanned_files,
            candidate_files,
            duplicate_groups,
            duplicate_files,
            reclaimable_bytes,
        },
        groups,
    };

    println!("scan roots: {:?}", result.roots);
    println!("scanned files: {}", result.summary.scanned_files);
    println!("size-candidate groups: {}", candidate_groups);
    println!("size-candidate files: {}", result.summary.candidate_files);
    println!("size-candidate bytes: {}", candidate_bytes);
    println!("duplicate groups: {}", result.summary.duplicate_groups);
    println!("duplicate files: {}", result.summary.duplicate_files);
    println!("reclaimable bytes: {}", result.summary.reclaimable_bytes);

    for (idx, group) in result.groups.iter().enumerate() {
        println!(
            "\n[{}] size={} hash={} files={}",
            idx + 1,
            group.file_size,
            group.content_hash,
            group.files.len()
        );
        for file in &group.files {
            println!("  - {}", file.path.display());
        }
    }

    if let Some(json_path) = args.json.as_deref() {
        export::write_json(json_path, &result)?;
        println!("json report written to {}", json_path.display());
    }

    if let Some(csv_path) = args.csv.as_deref() {
        export::write_csv(csv_path, &result)?;
        println!("csv report written to {}", csv_path.display());
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

fn build_duplicate_groups_for_size(file_size: u64, files: Vec<FileEntry>) -> Vec<DuplicateGroup> {
    let mut by_hash: HashMap<String, Vec<FileEntry>> = HashMap::new();

    for file in files {
        match hash_file_blake3(&file.path) {
            Ok(content_hash) => by_hash.entry(content_hash).or_default().push(file),
            Err(err) => eprintln!(
                "warn: failed to hash {}: {err}",
                file.path.as_path().display()
            ),
        }
    }

    by_hash
        .into_iter()
        .filter_map(|(content_hash, mut hashed_files)| {
            if hashed_files.len() < 2 {
                return None;
            }

            hashed_files.sort_by(|a, b| a.path.cmp(&b.path));
            Some(DuplicateGroup {
                file_size,
                content_hash,
                files: hashed_files,
            })
        })
        .collect()
}

fn hash_file_blake3(path: &Path) -> Result<String> {
    let file = File::open(path).with_context(|| format!("open failed: {}", path.display()))?;
    let mut reader = BufReader::new(file);
    let mut hasher = blake3::Hasher::new();
    let mut buf = [0_u8; 64 * 1024];

    loop {
        let read = reader
            .read(&mut buf)
            .with_context(|| format!("read failed: {}", path.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buf[..read]);
    }

    Ok(hasher.finalize().to_hex().to_string())
}

fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
