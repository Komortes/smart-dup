use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn photos_exact_finds_duplicate_photo_files() {
    let tmp = make_temp_dir("photos-exact");
    let root = tmp.join("photos");
    fs::create_dir_all(&root).expect("create photos dir");

    let a = root.join("a.jpg");
    let b = root.join("b.JPG");
    let unique = root.join("unique.png");
    let ignored = root.join("note.txt");

    fs::write(&a, b"same-binary-content").expect("write a.jpg");
    fs::write(&b, b"same-binary-content").expect("write b.JPG");
    fs::write(&unique, b"different-content").expect("write unique.png");
    fs::write(&ignored, b"same-binary-content").expect("write note.txt");

    let out = run_smartdup(&["photos", root.to_str().expect("utf-8 path")]);
    assert!(
        out.status.success(),
        "photos exact command failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("photos_exact")
            && stdout.contains("duplicate_groups=1")
            && stdout.contains("duplicate_files=2"),
        "unexpected summary output:\n{}",
        stdout
    );
    assert!(
        stdout.contains(&a.display().to_string()) && stdout.contains(&b.display().to_string()),
        "expected duplicate paths in output:\n{}",
        stdout
    );
    assert!(
        !stdout.contains(&ignored.display().to_string()),
        "non-photo file must be ignored:\n{}",
        stdout
    );

    fs::remove_dir_all(&tmp).expect("cleanup temp dir");
}

#[cfg(not(target_os = "macos"))]
#[test]
fn photos_similar_runs_on_non_macos_and_reports_summary() {
    let tmp = make_temp_dir("photos-similar-non-macos");
    let root = tmp.join("photos");
    fs::create_dir_all(&root).expect("create photos dir");

    let a = root.join("a.jpg");
    let b = root.join("b.jpg");
    fs::write(&a, b"fake-image-a").expect("write a.jpg");
    fs::write(&b, b"fake-image-b").expect("write b.jpg");

    let out = run_smartdup(&["photos", root.to_str().expect("utf-8 path"), "--similar"]);
    assert!(
        out.status.success(),
        "photos --similar should return summary on non-macOS\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("photos_similar")
            && stdout.contains("scanned=2")
            && stdout.contains("threshold=8"),
        "expected similar mode summary, got:\n{}",
        stdout
    );

    fs::remove_dir_all(&tmp).expect("cleanup temp dir");
}

fn run_smartdup(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_smart-dup"))
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
        "smartdup-photos-it-{}-{}-{}",
        tag,
        std::process::id(),
        nanos
    ));
    fs::create_dir_all(&dir).expect("create temp dir");
    dir
}
