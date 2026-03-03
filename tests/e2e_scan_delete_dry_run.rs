use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn scan_then_delete_dry_run_keeps_files() {
    let tmp = make_temp_dir("e2e");
    let root = tmp.join("data");
    fs::create_dir_all(&root).expect("create data dir");

    let dup_a = root.join("dup-a.txt");
    let dup_b = root.join("dup-b.txt");
    let unique = root.join("unique.txt");
    let report = tmp.join("report.json");

    fs::write(&dup_a, b"same-content").expect("write dup-a");
    fs::write(&dup_b, b"same-content").expect("write dup-b");
    fs::write(&unique, b"different").expect("write unique");

    let scan = run_smartdup(&[
        "scan",
        root.to_str().expect("utf-8 path"),
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

    assert!(dup_a.exists(), "dry-run must not remove dup-a");
    assert!(dup_b.exists(), "dry-run must not remove dup-b");
    assert!(unique.exists(), "dry-run must not remove unique file");

    fs::remove_dir_all(&tmp).expect("cleanup temp dir");
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
