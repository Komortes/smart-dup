use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn scan_then_delete_dry_run_keeps_files() {
    let fixture = create_basic_fixture("e2e");
    let report = fixture.tmp.join("report.json");

    let scan = run_smartdup(&[
        "scan",
        fixture.root.to_str().expect("utf-8 path"),
        "--min-size",
        "1B",
        "--no-default-ignores",
        "--json",
        report.to_str().expect("utf-8 path"),
    ]);
    assert!(
        scan.status.success(),
        "scan failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&scan.stdout),
        String::from_utf8_lossy(&scan.stderr)
    );
    assert!(report.exists(), "expected json report to be created");

    let raw = fs::read_to_string(&report).expect("read json report");
    let json: serde_json::Value = serde_json::from_str(&raw).expect("parse json report");
    let groups = json["groups"].as_array().expect("groups array in report");
    assert!(
        groups.iter().any(|g| {
            g["files"]
                .as_array()
                .map(|files| files.len() >= 2)
                .unwrap_or(false)
        }),
        "expected at least one duplicate group with 2+ files"
    );

    let delete = run_smartdup(&[
        "delete",
        "--from",
        report.to_str().expect("utf-8 path"),
        "--dry-run",
        "--keep",
        "oldest",
        "--no-trash",
    ]);
    assert!(
        delete.status.success(),
        "delete dry-run failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&delete.stdout),
        String::from_utf8_lossy(&delete.stderr)
    );

    assert!(fixture.dup_a.exists(), "dry-run must not remove dup-a");
    assert!(fixture.dup_b.exists(), "dry-run must not remove dup-b");
    assert!(
        fixture.unique.exists(),
        "dry-run must not remove unique file"
    );

    fs::remove_dir_all(&fixture.tmp).expect("cleanup temp dir");
}

#[test]
fn delete_path_priority_picks_preferred_path_in_dry_run() {
    let tmp = make_temp_dir("path-priority");
    let preferred_dir = tmp.join("preferred");
    let other_dir = tmp.join("other");
    fs::create_dir_all(&preferred_dir).expect("create preferred dir");
    fs::create_dir_all(&other_dir).expect("create other dir");

    let preferred_file = preferred_dir.join("dup.txt");
    let other_file = other_dir.join("dup.txt");
    fs::write(&preferred_file, b"same-content").expect("write preferred dup");
    fs::write(&other_file, b"same-content").expect("write other dup");

    let report = tmp.join("report.json");
    let scan = run_smartdup(&[
        "scan",
        tmp.to_str().expect("utf-8 path"),
        "--min-size",
        "1B",
        "--no-default-ignores",
        "--json",
        report.to_str().expect("utf-8 path"),
    ]);
    assert!(
        scan.status.success(),
        "scan failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&scan.stdout),
        String::from_utf8_lossy(&scan.stderr)
    );

    let delete = run_smartdup(&[
        "delete",
        "--from",
        report.to_str().expect("utf-8 path"),
        "--dry-run",
        "--keep",
        "path-priority",
        "--prefer-path",
        preferred_dir.to_str().expect("utf-8 path"),
        "--no-trash",
    ]);
    assert!(
        delete.status.success(),
        "delete dry-run path-priority failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&delete.stdout),
        String::from_utf8_lossy(&delete.stderr)
    );

    let stdout = String::from_utf8_lossy(&delete.stdout);
    assert!(
        stdout.contains(&format!("keep:   {}", preferred_file.display())),
        "expected preferred file to be selected as keep\nstdout:\n{}",
        stdout
    );

    assert!(
        preferred_file.exists(),
        "dry-run must not remove preferred file"
    );
    assert!(
        other_file.exists(),
        "dry-run must not remove non-preferred file"
    );

    fs::remove_dir_all(&tmp).expect("cleanup temp dir");
}

#[test]
fn scan_quiet_no_progress_outputs_summary_only() {
    let fixture = create_basic_fixture("quiet");

    let scan = run_smartdup(&[
        "scan",
        fixture.root.to_str().expect("utf-8 path"),
        "--min-size",
        "1B",
        "--no-default-ignores",
        "--quiet",
        "--no-progress",
    ]);
    assert!(
        scan.status.success(),
        "scan quiet/no-progress failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&scan.stdout),
        String::from_utf8_lossy(&scan.stderr)
    );

    let stdout = String::from_utf8_lossy(&scan.stdout);
    assert!(
        stdout.contains("scanned_files=")
            && stdout.contains("duplicate_groups=")
            && stdout.contains("reclaimable_bytes="),
        "expected compact summary line, got:\n{}",
        stdout
    );
    assert!(
        !stdout.contains("hashing") && !stdout.contains("walking"),
        "expected no progress output, got:\n{}",
        stdout
    );
    assert!(
        !stdout.contains("\n[1]"),
        "expected no detailed duplicate groups in quiet mode, got:\n{}",
        stdout
    );

    fs::remove_dir_all(&fixture.tmp).expect("cleanup temp dir");
}

#[test]
fn delete_path_priority_requires_prefer_path_argument() {
    let fixture = create_basic_fixture("path-priority-missing-arg");
    let report = fixture.tmp.join("report.json");

    let scan = run_smartdup(&[
        "scan",
        fixture.root.to_str().expect("utf-8 path"),
        "--min-size",
        "1B",
        "--no-default-ignores",
        "--json",
        report.to_str().expect("utf-8 path"),
    ]);
    assert!(
        scan.status.success(),
        "scan failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&scan.stdout),
        String::from_utf8_lossy(&scan.stderr)
    );

    let delete = run_smartdup(&[
        "delete",
        "--from",
        report.to_str().expect("utf-8 path"),
        "--dry-run",
        "--keep",
        "path-priority",
    ]);
    assert!(
        !delete.status.success(),
        "delete should fail without --prefer-path\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&delete.stdout),
        String::from_utf8_lossy(&delete.stderr)
    );

    let stderr = String::from_utf8_lossy(&delete.stderr);
    assert!(
        stderr.contains("requires at least one `--prefer-path <PATH>`"),
        "expected validation error message, got:\n{}",
        stderr
    );

    fs::remove_dir_all(&fixture.tmp).expect("cleanup temp dir");
}

#[test]
fn delete_quiet_dry_run_outputs_summary_only() {
    let fixture = create_basic_fixture("delete-quiet");
    let report = fixture.tmp.join("report.json");

    let scan = run_smartdup(&[
        "scan",
        fixture.root.to_str().expect("utf-8 path"),
        "--min-size",
        "1B",
        "--no-default-ignores",
        "--json",
        report.to_str().expect("utf-8 path"),
    ]);
    assert!(
        scan.status.success(),
        "scan failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&scan.stdout),
        String::from_utf8_lossy(&scan.stderr)
    );

    let delete = run_smartdup(&[
        "delete",
        "--from",
        report.to_str().expect("utf-8 path"),
        "--dry-run",
        "--quiet",
        "--no-trash",
    ]);
    assert!(
        delete.status.success(),
        "delete quiet dry-run failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&delete.stdout),
        String::from_utf8_lossy(&delete.stderr)
    );

    let stdout = String::from_utf8_lossy(&delete.stdout);
    assert!(
        stdout.contains("planned_groups=")
            && stdout.contains("planned_files=")
            && stdout.contains("dry_run=true"),
        "expected compact delete summary line, got:\n{}",
        stdout
    );
    assert!(
        !stdout.contains("[group ") && !stdout.contains("keep:"),
        "expected no detailed group output in quiet mode, got:\n{}",
        stdout
    );

    fs::remove_dir_all(&fixture.tmp).expect("cleanup temp dir");
}

#[test]
fn interactive_delete_decline_keeps_files() {
    let fixture = create_basic_fixture("interactive-decline");
    let report = fixture.tmp.join("report.json");

    let scan = run_smartdup(&[
        "scan",
        fixture.root.to_str().expect("utf-8 path"),
        "--min-size",
        "1B",
        "--no-default-ignores",
        "--json",
        report.to_str().expect("utf-8 path"),
    ]);
    assert!(
        scan.status.success(),
        "scan failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&scan.stdout),
        String::from_utf8_lossy(&scan.stderr)
    );

    let delete = run_smartdup_with_stdin(
        &[
            "delete",
            "--from",
            report.to_str().expect("utf-8 path"),
            "--interactive",
            "--keep",
            "oldest",
            "--no-trash",
        ],
        "n\n",
    );
    assert!(
        delete.status.success(),
        "interactive delete (decline) failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&delete.stdout),
        String::from_utf8_lossy(&delete.stderr)
    );

    assert!(fixture.dup_a.exists(), "declined delete must keep dup-a");
    assert!(fixture.dup_b.exists(), "declined delete must keep dup-b");
    assert!(fixture.unique.exists(), "declined delete must keep unique");

    fs::remove_dir_all(&fixture.tmp).expect("cleanup temp dir");
}

#[test]
fn interactive_delete_confirm_removes_one_duplicate() {
    let fixture = create_basic_fixture("interactive-confirm");
    let report = fixture.tmp.join("report.json");

    let scan = run_smartdup(&[
        "scan",
        fixture.root.to_str().expect("utf-8 path"),
        "--min-size",
        "1B",
        "--no-default-ignores",
        "--json",
        report.to_str().expect("utf-8 path"),
    ]);
    assert!(
        scan.status.success(),
        "scan failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&scan.stdout),
        String::from_utf8_lossy(&scan.stderr)
    );

    let delete = run_smartdup_with_stdin(
        &[
            "delete",
            "--from",
            report.to_str().expect("utf-8 path"),
            "--interactive",
            "--keep",
            "oldest",
            "--no-trash",
        ],
        "y\n",
    );
    assert!(
        delete.status.success(),
        "interactive delete (confirm) failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&delete.stdout),
        String::from_utf8_lossy(&delete.stderr)
    );

    let remaining_dups = fixture.dup_a.exists() as u8 + fixture.dup_b.exists() as u8;
    assert_eq!(
        remaining_dups, 1,
        "expected exactly one duplicate file to remain after confirmed delete"
    );
    assert!(
        fixture.unique.exists(),
        "confirmed delete must not remove non-duplicate file"
    );

    fs::remove_dir_all(&fixture.tmp).expect("cleanup temp dir");
}

#[test]
fn interactive_delete_skips_when_file_hash_changed_after_scan() {
    let fixture = create_basic_fixture("interactive-hash-mismatch");
    let report = fixture.tmp.join("report.json");

    let scan = run_smartdup(&[
        "scan",
        fixture.root.to_str().expect("utf-8 path"),
        "--min-size",
        "1B",
        "--no-default-ignores",
        "--json",
        report.to_str().expect("utf-8 path"),
    ]);
    assert!(
        scan.status.success(),
        "scan failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&scan.stdout),
        String::from_utf8_lossy(&scan.stderr)
    );

    fs::write(&fixture.dup_b, b"changed-after-scan").expect("mutate duplicate after scan");

    let delete = run_smartdup_with_stdin(
        &[
            "delete",
            "--from",
            report.to_str().expect("utf-8 path"),
            "--interactive",
            "--keep",
            "oldest",
            "--no-trash",
            "--quiet",
        ],
        "y\n",
    );
    assert!(
        delete.status.success(),
        "interactive delete with mismatch failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&delete.stdout),
        String::from_utf8_lossy(&delete.stderr)
    );

    let stdout = String::from_utf8_lossy(&delete.stdout);
    assert!(
        stdout.contains("hash_mismatch_files=1"),
        "expected hash mismatch counter in summary\nstdout:\n{}",
        stdout
    );
    assert!(fixture.dup_a.exists(), "keep file should remain");
    assert!(
        fixture.dup_b.exists(),
        "mutated file should be skipped and remain"
    );
    assert!(fixture.unique.exists(), "unique file should remain");

    fs::remove_dir_all(&fixture.tmp).expect("cleanup temp dir");
}

fn run_smartdup(args: &[&str]) -> std::process::Output {
    Command::new("cargo")
        .arg("run")
        .arg("--quiet")
        .arg("--")
        .args(args)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("run smartdup command")
}

fn run_smartdup_with_stdin(args: &[&str], input: &str) -> std::process::Output {
    let mut child = Command::new("cargo")
        .arg("run")
        .arg("--quiet")
        .arg("--")
        .args(args)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn smartdup command");

    if let Some(stdin) = child.stdin.as_mut() {
        stdin
            .write_all(input.as_bytes())
            .expect("write stdin to smartdup");
    } else {
        panic!("failed to open stdin for smartdup process");
    }

    child.wait_with_output().expect("wait for smartdup output")
}

fn make_temp_dir(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time went backwards")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "smartdup-it-{}-{}-{}",
        tag,
        std::process::id(),
        nanos
    ));
    fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

struct BasicFixture {
    tmp: PathBuf,
    root: PathBuf,
    dup_a: PathBuf,
    dup_b: PathBuf,
    unique: PathBuf,
}

fn create_basic_fixture(tag: &str) -> BasicFixture {
    let tmp = make_temp_dir(tag);
    let root = tmp.join("data");
    fs::create_dir_all(&root).expect("create data dir");

    let dup_a = root.join("dup-a.txt");
    let dup_b = root.join("dup-b.txt");
    let unique = root.join("unique.txt");
    fs::write(&dup_a, b"same-content").expect("write dup-a");
    fs::write(&dup_b, b"same-content").expect("write dup-b");
    fs::write(&unique, b"different").expect("write unique");

    BasicFixture {
        tmp,
        root,
        dup_a,
        dup_b,
        unique,
    }
}
