use crate::cli::ScanArgs;
use crate::core::models::{DuplicateGroup, FileEntry, ScanResult, ScanSummary};
use crate::output::export;
use anyhow::{Context, Result};
use indicatif::{ProgressBar, ProgressStyle};
use rayon::ThreadPoolBuilder;
use rayon::prelude::*;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};
use walkdir::{DirEntry, WalkDir};

const DEFAULT_IGNORES: [&str; 3] = [".git", "node_modules", "target"];

pub fn run(args: ScanArgs) -> Result<()> {
    let mut scanned_files: u64 = 0;
    let mut by_size: HashMap<u64, Vec<FileEntry>> = HashMap::new();
    let ignore_rules = build_ignore_rules(&args.ignores, args.no_default_ignores);
    let walk_progress = make_walk_progress();

    for root in &args.paths {
        walk_progress.set_message(format!("walking {}", root.display()));
        let walker = WalkDir::new(root)
            .follow_links(args.follow_symlinks)
            .into_iter()
            .filter_entry(|entry| !is_ignored(entry, &ignore_rules));

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
            walk_progress.inc(1);

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
    walk_progress.finish_and_clear();

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

    let hash_progress = make_hash_progress(candidate_files);
    let mut groups = compute_duplicate_groups(by_size, hash_progress.clone(), args.threads)?;
    hash_progress.finish_and_clear();

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
    if let Some(threads) = args.threads {
        println!("hash threads: {}", threads);
    }
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
    let components = entry
        .path()
        .components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>();

    ignores.iter().any(|rule| {
        if rule.contains('/') || rule.contains('\\') {
            path_text.contains(rule.as_str())
        } else {
            components.iter().any(|component| component == rule)
        }
    })
}

fn build_ignore_rules(user_ignores: &[String], no_default_ignores: bool) -> Vec<String> {
    let mut rules = Vec::new();
    if !no_default_ignores {
        rules.extend(DEFAULT_IGNORES.iter().map(|s| s.to_string()));
    }
    rules.extend(user_ignores.iter().cloned());
    rules
}

fn build_duplicate_groups_for_size(
    file_size: u64,
    files: Vec<FileEntry>,
    hash_progress: ProgressBar,
) -> Vec<DuplicateGroup> {
    let mut by_hash: HashMap<String, Vec<FileEntry>> = HashMap::new();

    for file in files {
        match hash_file_blake3(&file.path) {
            Ok(content_hash) => by_hash.entry(content_hash).or_default().push(file),
            Err(err) => eprintln!(
                "warn: failed to hash {}: {err}",
                file.path.as_path().display()
            ),
        }
        hash_progress.inc(1);
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

fn compute_duplicate_groups(
    by_size: HashMap<u64, Vec<FileEntry>>,
    hash_progress: ProgressBar,
    threads: Option<usize>,
) -> Result<Vec<DuplicateGroup>> {
    if let Some(threads) = threads {
        let pool = ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .context("failed to initialize rayon thread pool for hashing")?;
        Ok(pool.install(|| hash_duplicate_groups_parallel(by_size, hash_progress)))
    } else {
        Ok(hash_duplicate_groups_parallel(by_size, hash_progress))
    }
}

fn hash_duplicate_groups_parallel(
    by_size: HashMap<u64, Vec<FileEntry>>,
    hash_progress: ProgressBar,
) -> Vec<DuplicateGroup> {
    by_size
        .into_par_iter()
        .filter_map(|(size, files)| {
            if files.len() < 2 {
                return None;
            }
            Some(build_duplicate_groups_for_size(
                size,
                files,
                hash_progress.clone(),
            ))
        })
        .reduce(Vec::new, |mut acc, mut next| {
            acc.append(&mut next);
            acc
        })
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

fn make_walk_progress() -> ProgressBar {
    let bar = ProgressBar::new_spinner();
    bar.set_style(
        ProgressStyle::with_template("{spinner:.green} {msg} ({pos} files)")
            .unwrap_or_else(|_| ProgressStyle::default_spinner()),
    );
    bar.enable_steady_tick(Duration::from_millis(120));
    bar
}

fn make_hash_progress(total: u64) -> ProgressBar {
    if total == 0 {
        return ProgressBar::hidden();
    }

    let bar = ProgressBar::new(total);
    bar.set_style(
        ProgressStyle::with_template(
            "{spinner:.green} hashing [{bar:40.cyan/blue}] {pos}/{len} ({percent}%)",
        )
        .unwrap_or_else(|_| ProgressStyle::default_bar()),
    );
    bar
}

#[cfg(test)]
mod tests {
    use super::build_ignore_rules;

    #[test]
    fn default_ignore_rules_are_enabled_by_default() {
        let rules = build_ignore_rules(&["custom".to_string()], false);
        assert!(rules.contains(&".git".to_string()));
        assert!(rules.contains(&"node_modules".to_string()));
        assert!(rules.contains(&"target".to_string()));
        assert!(rules.contains(&"custom".to_string()));
    }

    #[test]
    fn default_ignore_rules_can_be_disabled() {
        let rules = build_ignore_rules(&["custom".to_string()], true);
        assert_eq!(rules, vec!["custom".to_string()]);
    }
}
