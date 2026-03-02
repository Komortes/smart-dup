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

#[cfg(test)]
mod tests {
    use super::{build_plan_for_group, choose_keep_index, run};
    use crate::cli::{DeleteArgs, KeepRule};
    use crate::core::models::{DuplicateGroup, FileEntry, ScanResult, ScanSummary};
    use std::fs::{self, File};
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn choose_keep_index_by_oldest() {
        let files = vec![
            entry("/tmp/c.txt", Some(300)),
            entry("/tmp/a.txt", Some(100)),
            entry("/tmp/b.txt", Some(200)),
        ];
        assert_eq!(choose_keep_index(&files, KeepRule::Oldest), 1);
    }

    #[test]
    fn choose_keep_index_by_newest() {
        let files = vec![
            entry("/tmp/c.txt", Some(300)),
            entry("/tmp/a.txt", Some(100)),
            entry("/tmp/b.txt", Some(200)),
        ];
        assert_eq!(choose_keep_index(&files, KeepRule::Newest), 0);
    }

    #[test]
    fn choose_keep_index_by_lexicographic() {
        let files = vec![
            entry("/tmp/c.txt", Some(300)),
            entry("/tmp/a.txt", Some(100)),
            entry("/tmp/b.txt", Some(200)),
        ];
        assert_eq!(choose_keep_index(&files, KeepRule::Lexicographic), 1);
    }

    #[test]
    fn build_plan_keeps_one_and_marks_rest_for_delete() {
        let group = DuplicateGroup {
            file_size: 42,
            content_hash: "h".to_string(),
            files: vec![
                entry("/tmp/c.txt", Some(300)),
                entry("/tmp/a.txt", Some(100)),
                entry("/tmp/b.txt", Some(200)),
            ],
        };

        let plan = build_plan_for_group(1, &group, KeepRule::Oldest).expect("plan should exist");
        assert_eq!(plan.keep_file.path, PathBuf::from("/tmp/a.txt"));
        assert_eq!(plan.delete_files.len(), 2);
        assert_eq!(plan.file_size, 42);
    }

    #[test]
    fn dry_run_does_not_delete_files() {
        let tmp = make_temp_dir("dry-run");
        let keep_path = tmp.join("keep.txt");
        let dup_path = tmp.join("dup.txt");
        let json_path = tmp.join("scan.json");

        fs::write(&keep_path, b"same").expect("write keep file");
        fs::write(&dup_path, b"same").expect("write dup file");

        let report = ScanResult {
            roots: vec![tmp.clone()],
            generated_at_unix_secs: 0,
            summary: ScanSummary {
                scanned_files: 2,
                candidate_files: 2,
                duplicate_groups: 1,
                duplicate_files: 2,
                reclaimable_bytes: 4,
            },
            groups: vec![DuplicateGroup {
                file_size: 4,
                content_hash: "hash".to_string(),
                files: vec![
                    FileEntry {
                        path: keep_path.clone(),
                        size: 4,
                        modified_unix_secs: Some(1),
                    },
                    FileEntry {
                        path: dup_path.clone(),
                        size: 4,
                        modified_unix_secs: Some(2),
                    },
                ],
            }],
        };

        let file = File::create(&json_path).expect("create json");
        serde_json::to_writer(file, &report).expect("write json");

        let args = DeleteArgs {
            from_json: json_path,
            dry_run: true,
            interactive: false,
            keep: KeepRule::Oldest,
        };
        run(args).expect("dry-run should succeed");

        assert!(keep_path.exists());
        assert!(dup_path.exists());

        fs::remove_dir_all(&tmp).expect("cleanup temp dir");
    }

    fn entry(path: &str, modified_unix_secs: Option<u64>) -> FileEntry {
        FileEntry {
            path: PathBuf::from(path),
            size: 1,
            modified_unix_secs,
        }
    }

    fn make_temp_dir(tag: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time went backwards")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "smartdup-tests-{}-{}-{}",
            tag,
            std::process::id(),
            nanos
        ));
        fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }
}
