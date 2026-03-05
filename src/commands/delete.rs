use crate::cli::{DeleteArgs, KeepRule};
use crate::core::models::{DuplicateGroup, FileEntry, ScanResult};
use anyhow::{Context, Result, bail};
use std::fs::{self, File};
use std::io::{BufReader, Read, Write, stdin, stdout};
use std::path::{Path, PathBuf};

pub fn run(args: DeleteArgs) -> Result<()> {
    if !args.dry_run && !args.interactive && !args.yes {
        bail!("safe mode: use --dry-run, or pass --interactive, or --yes");
    }
    if matches!(args.keep, KeepRule::PathPriority) && args.prefer_path.is_empty() {
        bail!("`--keep path-priority` requires at least one `--prefer-path <PATH>`");
    }

    let scan_result = load_scan_result(&args.from_json)?;
    let plans = build_plans(&scan_result, args.keep, &args.prefer_path);

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
    if !args.dry_run
        && let Some(limit) = args.max_delete
        && planned_files > limit
    {
        bail!(
            "safety limit exceeded: planned deletions {} > --max-delete {}",
            planned_files,
            limit
        );
    }
    if !args.dry_run
        && let Some(limit_bytes) = args.max_delete_bytes
        && planned_bytes > limit_bytes
    {
        bail!(
            "safety limit exceeded: planned bytes {} > --max-delete-bytes {}",
            planned_bytes,
            limit_bytes
        );
    }

    let use_trash = args.trash && !args.no_trash;
    let verify_hash = !args.no_verify_hash;
    if !args.quiet {
        println!("loaded groups: {}", scan_result.groups.len());
        println!("planned groups: {}", plans.len());
        println!("planned deletions: {} files", planned_files);
        println!("planned reclaimable: {} bytes", planned_bytes);
        println!("keep rule: {:?}", args.keep);
        if matches!(args.keep, KeepRule::PathPriority) {
            println!("preferred paths: {:?}", args.prefer_path);
        }
        println!("dry_run: {}", args.dry_run);
        println!("assume yes: {}", args.yes);
        if let Some(limit) = args.max_delete {
            println!("max delete: {}", limit);
        }
        if let Some(limit_bytes) = args.max_delete_bytes {
            println!("max delete bytes: {}", limit_bytes);
        }
        println!("trash mode: {}", use_trash);
        println!("verify hash: {}", verify_hash);
    }

    let mut deleted_files = 0_u64;
    let mut failed_files = 0_u64;
    let mut hash_mismatch_files = 0_u64;
    let mut reclaimed_bytes = 0_u64;
    let mut skipped_groups = 0_u64;

    for plan in &plans {
        if !args.quiet {
            print_group_plan(plan);
        }

        if args.dry_run {
            continue;
        }

        let confirmed = if args.yes { true } else { confirm_group(plan)? };
        if !confirmed {
            skipped_groups += 1;
            if !args.quiet {
                println!("  skipped");
            }
            continue;
        }

        for file in &plan.delete_files {
            if verify_hash {
                match verify_file_matches_group_hash(&file.path, &plan.content_hash) {
                    Ok(true) => {}
                    Ok(false) => {
                        hash_mismatch_files += 1;
                        eprintln!(
                            "  warn: hash mismatch for {}, skipping delete",
                            file.path.display()
                        );
                        continue;
                    }
                    Err(err) => {
                        failed_files += 1;
                        eprintln!(
                            "  warn: failed to verify hash for {}: {err}",
                            file.path.display()
                        );
                        continue;
                    }
                }
            }

            match delete_file_safely(&file.path, use_trash) {
                Ok(_) => {
                    deleted_files += 1;
                    reclaimed_bytes += plan.file_size;
                    if !args.quiet {
                        println!("  removed: {}", file.path.display());
                    }
                }
                Err(err) => {
                    failed_files += 1;
                    eprintln!("  warn: failed to delete {}: {err}", file.path.display());
                }
            }
        }
    }

    if args.quiet {
        println!(
            "{}",
            format_delete_summary_line(DeleteSummary {
                planned_groups: plans.len() as u64,
                planned_files,
                planned_bytes,
                deleted_files,
                failed_files,
                hash_mismatch_files,
                skipped_groups,
                reclaimed_bytes,
                dry_run: args.dry_run,
            })
        );
    } else {
        if args.dry_run {
            println!("\ndry-run complete. no files were deleted.");
        } else {
            println!("\ninteractive delete complete.");
        }
        println!("deleted files: {}", deleted_files);
        println!("failed deletions: {}", failed_files);
        println!("hash mismatch skips: {}", hash_mismatch_files);
        println!("skipped groups: {}", skipped_groups);
        println!("reclaimed bytes (actual): {}", reclaimed_bytes);
    }

    Ok(())
}

#[derive(Debug)]
struct DeleteSummary {
    planned_groups: u64,
    planned_files: u64,
    planned_bytes: u64,
    deleted_files: u64,
    failed_files: u64,
    hash_mismatch_files: u64,
    skipped_groups: u64,
    reclaimed_bytes: u64,
    dry_run: bool,
}

fn format_delete_summary_line(summary: DeleteSummary) -> String {
    format!(
        "planned_groups={} planned_files={} planned_bytes={} deleted_files={} failed_files={} hash_mismatch_files={} skipped_groups={} reclaimed_bytes={} dry_run={}",
        summary.planned_groups,
        summary.planned_files,
        summary.planned_bytes,
        summary.deleted_files,
        summary.failed_files,
        summary.hash_mismatch_files,
        summary.skipped_groups,
        summary.reclaimed_bytes,
        summary.dry_run
    )
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

fn build_plans(
    result: &ScanResult,
    keep_rule: KeepRule,
    prefer_paths: &[PathBuf],
) -> Vec<DeletionPlan> {
    result
        .groups
        .iter()
        .enumerate()
        .filter_map(|(idx, group)| build_plan_for_group(idx + 1, group, keep_rule, prefer_paths))
        .collect()
}

fn build_plan_for_group(
    group_index: usize,
    group: &DuplicateGroup,
    keep_rule: KeepRule,
    prefer_paths: &[PathBuf],
) -> Option<DeletionPlan> {
    if group.files.len() < 2 {
        return None;
    }

    let keep_idx = choose_keep_index(&group.files, keep_rule, prefer_paths);
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

fn choose_keep_index(files: &[FileEntry], keep_rule: KeepRule, prefer_paths: &[PathBuf]) -> usize {
    match keep_rule {
        KeepRule::Lexicographic => choose_lexicographic_index(files),
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
        KeepRule::PathPriority => choose_path_priority_index(files, prefer_paths)
            .unwrap_or_else(|| choose_lexicographic_index(files)),
    }
}

fn choose_lexicographic_index(files: &[FileEntry]) -> usize {
    files
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| a.path.cmp(&b.path))
        .map(|(idx, _)| idx)
        .unwrap_or(0)
}

fn choose_path_priority_index(files: &[FileEntry], prefer_paths: &[PathBuf]) -> Option<usize> {
    files
        .iter()
        .enumerate()
        .filter_map(|(idx, file)| {
            let priority = prefer_paths
                .iter()
                .position(|prefer| file.path.starts_with(prefer))?;
            Some((priority, &file.path, idx))
        })
        .min_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(b.1)))
        .map(|(_, _, idx)| idx)
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

fn delete_file_safely(path: &Path, prefer_trash: bool) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        if prefer_trash {
            if let Err(err) = move_to_macos_trash(path) {
                eprintln!(
                    "  warn: failed to move to Trash ({}), fallback to delete",
                    err
                );
                fs::remove_file(path)
                    .with_context(|| format!("remove failed for {}", path.display()))?;
            }
            Ok(())
        } else {
            fs::remove_file(path)
                .with_context(|| format!("remove failed for {}", path.display()))?;
            Ok(())
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = prefer_trash;
        fs::remove_file(path).with_context(|| format!("remove failed for {}", path.display()))?;
        Ok(())
    }
}

fn verify_file_matches_group_hash(path: &Path, expected_hash: &str) -> Result<bool> {
    let actual = hash_file_blake3(path)?;
    Ok(actual.eq_ignore_ascii_case(expected_hash))
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

#[cfg(target_os = "macos")]
fn move_to_macos_trash(path: &Path) -> Result<()> {
    let home = std::env::var_os("HOME").context("HOME is not set")?;
    let trash_dir = PathBuf::from(home).join(".Trash");
    fs::create_dir_all(&trash_dir)
        .with_context(|| format!("failed to create Trash dir {}", trash_dir.display()))?;

    let file_name = path
        .file_name()
        .with_context(|| format!("missing file name for {}", path.display()))?;
    let target = unique_destination_in_dir(&trash_dir, file_name);

    fs::rename(path, &target).with_context(|| {
        format!(
            "failed to move {} to {}",
            path.display(),
            target.as_path().display()
        )
    })?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn unique_destination_in_dir(dir: &Path, file_name: &std::ffi::OsStr) -> PathBuf {
    let first = dir.join(file_name);
    if !first.exists() {
        return first;
    }

    let name = file_name.to_string_lossy();
    let (base, ext) = split_name_and_ext(&name);
    for idx in 1.. {
        let candidate = if ext.is_empty() {
            format!("{base} ({idx})")
        } else {
            format!("{base} ({idx}).{ext}")
        };
        let path = dir.join(candidate);
        if !path.exists() {
            return path;
        }
    }

    unreachable!("infinite loop should always return with a free file name")
}

#[cfg(target_os = "macos")]
fn split_name_and_ext(name: &str) -> (&str, &str) {
    if name.starts_with('.') && !name[1..].contains('.') {
        return (name, "");
    }
    if let Some((base, ext)) = name.rsplit_once('.') {
        if base.is_empty() {
            return (name, "");
        }
        return (base, ext);
    }
    (name, "")
}

#[cfg(test)]
mod tests {
    use super::{DeleteSummary, format_delete_summary_line};
    use super::{build_plan_for_group, choose_keep_index, delete_file_safely, run};
    #[cfg(target_os = "macos")]
    use super::{split_name_and_ext, unique_destination_in_dir};
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
        assert_eq!(choose_keep_index(&files, KeepRule::Oldest, &[]), 1);
    }

    #[test]
    fn choose_keep_index_by_newest() {
        let files = vec![
            entry("/tmp/c.txt", Some(300)),
            entry("/tmp/a.txt", Some(100)),
            entry("/tmp/b.txt", Some(200)),
        ];
        assert_eq!(choose_keep_index(&files, KeepRule::Newest, &[]), 0);
    }

    #[test]
    fn choose_keep_index_by_lexicographic() {
        let files = vec![
            entry("/tmp/c.txt", Some(300)),
            entry("/tmp/a.txt", Some(100)),
            entry("/tmp/b.txt", Some(200)),
        ];
        assert_eq!(choose_keep_index(&files, KeepRule::Lexicographic, &[]), 1);
    }

    #[test]
    fn choose_keep_index_by_path_priority() {
        let files = vec![
            entry("/Volumes/Archive/a.txt", Some(300)),
            entry("/Users/me/Photos/b.txt", Some(100)),
            entry("/Users/me/Downloads/c.txt", Some(200)),
        ];
        let prefer_paths = vec![
            PathBuf::from("/Users/me/Photos"),
            PathBuf::from("/Volumes/Archive"),
        ];
        assert_eq!(
            choose_keep_index(&files, KeepRule::PathPriority, &prefer_paths),
            1
        );
    }

    #[test]
    fn choose_keep_index_path_priority_falls_back_to_lexicographic() {
        let files = vec![
            entry("/tmp/c.txt", Some(300)),
            entry("/tmp/a.txt", Some(100)),
            entry("/tmp/b.txt", Some(200)),
        ];
        let prefer_paths = vec![PathBuf::from("/Users/me/Photos")];
        assert_eq!(
            choose_keep_index(&files, KeepRule::PathPriority, &prefer_paths),
            1
        );
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

        let plan =
            build_plan_for_group(1, &group, KeepRule::Oldest, &[]).expect("plan should exist");
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
            yes: false,
            quiet: false,
            trash: false,
            no_trash: true,
            no_verify_hash: false,
            max_delete: None,
            max_delete_bytes: None,
            keep: KeepRule::Oldest,
            prefer_path: vec![],
        };
        run(args).expect("dry-run should succeed");

        assert!(keep_path.exists());
        assert!(dup_path.exists());

        fs::remove_dir_all(&tmp).expect("cleanup temp dir");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn split_name_and_ext_works_for_common_cases() {
        assert_eq!(split_name_and_ext("photo.jpg"), ("photo", "jpg"));
        assert_eq!(split_name_and_ext("archive.tar.gz"), ("archive.tar", "gz"));
        assert_eq!(split_name_and_ext(".env"), (".env", ""));
        assert_eq!(split_name_and_ext("README"), ("README", ""));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn unique_destination_adds_suffix_when_target_exists() {
        let tmp = make_temp_dir("trash-name");
        let first = tmp.join("dup.jpg");
        File::create(&first).expect("create first file");

        let chosen = unique_destination_in_dir(&tmp, std::path::Path::new("dup.jpg").as_os_str());
        assert_eq!(chosen, tmp.join("dup (1).jpg"));

        fs::remove_dir_all(&tmp).expect("cleanup temp dir");
    }

    #[test]
    fn direct_delete_removes_file_when_trash_disabled() {
        let tmp = make_temp_dir("direct-delete");
        let file_path = tmp.join("x.txt");
        fs::write(&file_path, b"x").expect("write test file");

        delete_file_safely(&file_path, false).expect("delete should succeed");
        assert!(!file_path.exists());

        fs::remove_dir_all(&tmp).expect("cleanup temp dir");
    }

    #[test]
    fn delete_summary_line_has_stable_key_value_format() {
        let line = format_delete_summary_line(DeleteSummary {
            planned_groups: 2,
            planned_files: 5,
            planned_bytes: 1000,
            deleted_files: 3,
            failed_files: 1,
            hash_mismatch_files: 2,
            skipped_groups: 1,
            reclaimed_bytes: 700,
            dry_run: false,
        });
        assert_eq!(
            line,
            "planned_groups=2 planned_files=5 planned_bytes=1000 deleted_files=3 failed_files=1 hash_mismatch_files=2 skipped_groups=1 reclaimed_bytes=700 dry_run=false"
        );
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
