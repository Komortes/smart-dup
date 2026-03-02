use crate::cli::{DeleteArgs, KeepRule};
use crate::core::models::{DuplicateGroup, FileEntry, ScanResult};
use anyhow::{Context, Result, bail};
use std::fs::{self, File};
use std::io::{BufReader, Write, stdin, stdout};
use std::path::Path;

pub fn run(args: DeleteArgs) -> Result<()> {
    if !args.dry_run && !args.interactive {
        bail!("safe mode: use --dry-run or add --interactive for confirmed deletion");
    }

    let scan_result = load_scan_result(&args.from_json)?;
    let plans = build_plans(&scan_result, args.keep);

    if plans.is_empty() {
        println!("no duplicate groups available for deletion.");
        return Ok(());
    }

    let planned_files = plans
        .iter()
        .map(|p| p.delete_files.len() as u64)
        .sum::<u64>();
    let planned_bytes = plans
        .iter()
        .map(|p| p.file_size * p.delete_files.len() as u64)
        .sum::<u64>();

    println!("loaded groups: {}", scan_result.groups.len());
    println!("planned groups: {}", plans.len());
    println!("planned deletions: {} files", planned_files);
    println!("planned reclaimable: {} bytes", planned_bytes);
    println!("keep rule: {:?}", args.keep);
    println!("dry_run: {}", args.dry_run);

    let mut deleted_files = 0_u64;
    let mut failed_files = 0_u64;
    let mut reclaimed_bytes = 0_u64;
    let mut skipped_groups = 0_u64;

    for plan in &plans {
        print_group_plan(plan);

        if args.dry_run {
            continue;
        }

        let confirmed = confirm_group(plan)?;
        if !confirmed {
            skipped_groups += 1;
            println!("  skipped");
            continue;
        }

        for file in &plan.delete_files {
            match fs::remove_file(&file.path) {
                Ok(_) => {
                    deleted_files += 1;
                    reclaimed_bytes += plan.file_size;
                    println!("  deleted: {}", file.path.display());
                }
                Err(err) => {
                    failed_files += 1;
                    eprintln!("  warn: failed to delete {}: {err}", file.path.display());
                }
            }
        }
    }

    if args.dry_run {
        println!("\ndry-run complete. no files were deleted.");
    } else {
        println!("\ninteractive delete complete.");
    }
    println!("deleted files: {}", deleted_files);
    println!("failed deletions: {}", failed_files);
    println!("skipped groups: {}", skipped_groups);
    println!("reclaimed bytes (actual): {}", reclaimed_bytes);

    Ok(())
}

#[derive(Debug)]
struct DeletionPlan {
    group_index: usize,
    content_hash: String,
    file_size: u64,
    keep_file: FileEntry,
    delete_files: Vec<FileEntry>,
}

fn load_scan_result(path: &Path) -> Result<ScanResult> {
    let file = File::open(path).with_context(|| format!("open failed: {}", path.display()))?;
    let reader = BufReader::new(file);
    serde_json::from_reader(reader).with_context(|| format!("invalid JSON: {}", path.display()))
}

fn build_plans(result: &ScanResult, keep_rule: KeepRule) -> Vec<DeletionPlan> {
    result
        .groups
        .iter()
        .enumerate()
        .filter_map(|(idx, group)| build_plan_for_group(idx + 1, group, keep_rule))
        .collect()
}

fn build_plan_for_group(
    group_index: usize,
    group: &DuplicateGroup,
    keep_rule: KeepRule,
) -> Option<DeletionPlan> {
    if group.files.len() < 2 {
        return None;
    }

    let keep_idx = choose_keep_index(&group.files, keep_rule);
    let keep_file = group.files[keep_idx].clone();

    let delete_files = group
        .files
        .iter()
        .enumerate()
        .filter_map(|(idx, file)| {
            if idx == keep_idx {
                None
            } else {
                Some(file.clone())
            }
        })
        .collect::<Vec<_>>();

    if delete_files.is_empty() {
        return None;
    }

    Some(DeletionPlan {
        group_index,
        content_hash: group.content_hash.clone(),
        file_size: group.file_size,
        keep_file,
        delete_files,
    })
}

fn choose_keep_index(files: &[FileEntry], keep_rule: KeepRule) -> usize {
    match keep_rule {
        KeepRule::Lexicographic => files
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| a.path.cmp(&b.path))
            .map(|(idx, _)| idx)
            .unwrap_or(0),
        KeepRule::Oldest => files
            .iter()
            .enumerate()
            .min_by_key(|(_, file)| (file.modified_unix_secs.unwrap_or(u64::MAX), &file.path))
            .map(|(idx, _)| idx)
            .unwrap_or(0),
        KeepRule::Newest => files
            .iter()
            .enumerate()
            .max_by_key(|(_, file)| (file.modified_unix_secs.unwrap_or(0), &file.path))
            .map(|(idx, _)| idx)
            .unwrap_or(0),
    }
}

fn print_group_plan(plan: &DeletionPlan) {
    println!(
        "\n[group {}] size={} hash={} delete={}",
        plan.group_index,
        plan.file_size,
        plan.content_hash,
        plan.delete_files.len()
    );
    println!("  keep:   {}", plan.keep_file.path.display());
    for file in &plan.delete_files {
        println!("  delete: {}", file.path.display());
    }
}

fn confirm_group(plan: &DeletionPlan) -> Result<bool> {
    print!("  confirm deletion for group {}? [y/N]: ", plan.group_index);
    stdout().flush().context("failed to flush stdout")?;

    let mut line = String::new();
    stdin()
        .read_line(&mut line)
        .context("failed to read stdin response")?;
    let answer = line.trim().to_ascii_lowercase();
    Ok(answer == "y" || answer == "yes")
}
