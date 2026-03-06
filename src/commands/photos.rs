use crate::cli::PhotosArgs;
use anyhow::{Context, Result};
use rayon::prelude::*;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;
use walkdir::WalkDir;

#[cfg(target_os = "windows")]
use serde::Deserialize;
#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
use std::process::Command;
#[cfg(target_os = "macos")]
use std::time::SystemTime;

const PHOTO_EXTENSIONS: [&str; 10] = [
    "jpg", "jpeg", "png", "gif", "bmp", "tif", "tiff", "webp", "heic", "heif",
];

#[derive(Debug, Clone)]
struct PhotoFile {
    path: PathBuf,
    size: u64,
    modified_unix_secs: Option<u64>,
}

#[derive(Debug, Clone)]
struct ExactDuplicateGroup {
    file_size: u64,
    content_hash: String,
    files: Vec<PhotoFile>,
}

#[derive(Debug, Clone)]
struct SimilarPhoto {
    path: PathBuf,
    size: u64,
    modified_unix_secs: Option<u64>,
    width: Option<u32>,
    height: Option<u32>,
    dhash: u64,
}

#[derive(Debug, Clone, Copy)]
struct PerceptualData {
    dhash: u64,
    width: Option<u32>,
    height: Option<u32>,
}

pub fn run(args: PhotosArgs) -> Result<()> {
    let photos = collect_photo_files(&args.paths);
    if photos.is_empty() {
        println!("no photo files found in provided paths.");
        return Ok(());
    }

    if args.similar {
        run_similar_mode(&photos, args.threshold)?;
    } else {
        run_exact_mode(&photos)?;
    }

    Ok(())
}

fn collect_photo_files(roots: &[PathBuf]) -> Vec<PhotoFile> {
    let mut out = Vec::new();

    for root in roots {
        let walker = WalkDir::new(root).follow_links(false).into_iter();
        for entry in walker {
            let entry = match entry {
                Ok(entry) => entry,
                Err(err) => {
                    eprintln!("warn: failed to walk entry: {err}");
                    continue;
                }
            };
            if !entry.file_type().is_file() || !is_photo_file(entry.path()) {
                continue;
            }

            let metadata = match entry.metadata() {
                Ok(meta) => meta,
                Err(err) => {
                    eprintln!(
                        "warn: failed to read metadata for {}: {err}",
                        entry.path().display()
                    );
                    continue;
                }
            };

            let modified_unix_secs = metadata
                .modified()
                .ok()
                .and_then(|mtime| mtime.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_secs());

            out.push(PhotoFile {
                path: entry.path().to_path_buf(),
                size: metadata.len(),
                modified_unix_secs,
            });
        }
    }

    out.sort_by(|a, b| a.path.cmp(&b.path));
    out
}

fn run_exact_mode(photos: &[PhotoFile]) -> Result<()> {
    let mut by_size: HashMap<u64, Vec<PhotoFile>> = HashMap::new();
    for photo in photos {
        by_size.entry(photo.size).or_default().push(photo.clone());
    }

    let mut duplicate_groups = by_size
        .into_par_iter()
        .filter_map(|(size, files)| {
            if files.len() < 2 {
                return None;
            }

            let mut by_hash: HashMap<String, Vec<PhotoFile>> = HashMap::new();
            for file in files {
                match hash_file_blake3(&file.path) {
                    Ok(hash) => by_hash.entry(hash).or_default().push(file),
                    Err(err) => {
                        eprintln!("warn: failed to hash {}: {err}", file.path.display());
                    }
                }
            }

            let groups = by_hash
                .into_iter()
                .filter_map(|(content_hash, mut dup_files)| {
                    if dup_files.len() < 2 {
                        return None;
                    }
                    dup_files.sort_by(|a, b| a.path.cmp(&b.path));
                    Some(ExactDuplicateGroup {
                        file_size: size,
                        content_hash,
                        files: dup_files,
                    })
                })
                .collect::<Vec<_>>();
            Some(groups)
        })
        .reduce(Vec::new, |mut acc, mut next| {
            acc.append(&mut next);
            acc
        });

    duplicate_groups.sort_by(|a, b| {
        b.file_size
            .cmp(&a.file_size)
            .then_with(|| a.content_hash.cmp(&b.content_hash))
    });

    let duplicate_files = duplicate_groups
        .iter()
        .map(|g| g.files.len() as u64)
        .sum::<u64>();
    let reclaimable_bytes = duplicate_groups
        .iter()
        .map(|g| g.file_size * (g.files.len().saturating_sub(1) as u64))
        .sum::<u64>();

    println!(
        "photos_exact scanned={} duplicate_groups={} duplicate_files={} reclaimable_bytes={}",
        photos.len(),
        duplicate_groups.len(),
        duplicate_files,
        reclaimable_bytes
    );

    for (idx, group) in duplicate_groups.iter().enumerate() {
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

    Ok(())
}

fn run_similar_mode(photos: &[PhotoFile], threshold: u8) -> Result<()> {
    let mut similar_photos = Vec::new();
    let mut skipped = 0_u64;

    for photo in photos {
        match compute_perceptual_data(&photo.path) {
            Ok(perceptual) => similar_photos.push(SimilarPhoto {
                path: photo.path.clone(),
                size: photo.size,
                modified_unix_secs: photo.modified_unix_secs,
                width: perceptual.width,
                height: perceptual.height,
                dhash: perceptual.dhash,
            }),
            Err(err) => {
                skipped += 1;
                eprintln!(
                    "warn: failed to compute perceptual hash for {}: {err}",
                    photo.path.display()
                );
            }
        }
    }

    if similar_photos.is_empty() {
        anyhow::bail!(
            "photos --similar could not decode any photo files on this system; scanned={} skipped={}",
            photos.len(),
            skipped
        );
    }

    if similar_photos.len() < 2 {
        println!(
            "photos_similar scanned={} comparable={} skipped={} groups=0 threshold={}",
            photos.len(),
            similar_photos.len(),
            skipped,
            threshold
        );
        return Ok(());
    }

    let mut groups = group_similar_photos(&similar_photos, threshold);
    groups.sort_by_key(|g| std::cmp::Reverse(g.len()));

    let grouped_files = groups.iter().map(|g| g.len() as u64).sum::<u64>();
    println!(
        "photos_similar scanned={} comparable={} skipped={} groups={} grouped_files={} threshold={}",
        photos.len(),
        similar_photos.len(),
        skipped,
        groups.len(),
        grouped_files,
        threshold
    );

    for (idx, group) in groups.iter().enumerate() {
        println!("\n[{}] similar files={}", idx + 1, group.len());
        for file in group {
            println!(
                "  - {} | size={} | dims={} | mtime={}",
                file.path.display(),
                file.size,
                format_dimensions(file.width, file.height),
                format_mtime(file.modified_unix_secs)
            );
        }
    }

    Ok(())
}

fn group_similar_photos(photos: &[SimilarPhoto], threshold: u8) -> Vec<Vec<SimilarPhoto>> {
    let mut dsu = DisjointSet::new(photos.len());

    // Similar groups are connected components: A~B and B~C will end up together.
    for i in 0..photos.len() {
        for j in (i + 1)..photos.len() {
            if hamming_distance_u64(photos[i].dhash, photos[j].dhash) <= u32::from(threshold) {
                dsu.union(i, j);
            }
        }
    }

    let mut by_root: HashMap<usize, Vec<SimilarPhoto>> = HashMap::new();
    for (idx, photo) in photos.iter().enumerate() {
        let root = dsu.find(idx);
        by_root.entry(root).or_default().push(photo.clone());
    }

    let mut groups = by_root
        .into_values()
        .filter(|g| g.len() > 1)
        .collect::<Vec<_>>();
    for group in &mut groups {
        group.sort_by(|a, b| a.path.cmp(&b.path));
    }
    groups
}

fn hamming_distance_u64(a: u64, b: u64) -> u32 {
    (a ^ b).count_ones()
}

fn is_photo_file(path: &Path) -> bool {
    let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
        return false;
    };
    let lower = ext.to_ascii_lowercase();
    PHOTO_EXTENSIONS.contains(&lower.as_str())
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

fn format_dimensions(width: Option<u32>, height: Option<u32>) -> String {
    match (width, height) {
        (Some(w), Some(h)) => format!("{w}x{h}"),
        _ => "-".to_string(),
    }
}

fn format_mtime(mtime: Option<u64>) -> String {
    mtime
        .map(|v| v.to_string())
        .unwrap_or_else(|| "-".to_string())
}

#[cfg(target_os = "macos")]
fn compute_perceptual_data(path: &Path) -> Result<PerceptualData> {
    let (width, height) = read_dimensions_macos(path).unwrap_or((0, 0));
    let bmp = render_bmp_9x8_macos(path)?;
    let dhash = compute_dhash_from_bmp_9x8(&bmp)?;

    let width = (width > 0).then_some(width);
    let height = (height > 0).then_some(height);
    Ok(PerceptualData {
        dhash,
        width,
        height,
    })
}

#[cfg(target_os = "linux")]
fn compute_perceptual_data(path: &Path) -> Result<PerceptualData> {
    let (width, height) = read_dimensions_linux(path).unwrap_or((0, 0));
    let gray = render_gray_9x8_linux(path)?;
    let dhash = compute_dhash_from_gray_9x8(&gray)?;

    let width = (width > 0).then_some(width);
    let height = (height > 0).then_some(height);
    Ok(PerceptualData {
        dhash,
        width,
        height,
    })
}

#[cfg(target_os = "windows")]
fn compute_perceptual_data(path: &Path) -> Result<PerceptualData> {
    let (width, height, gray) = render_gray_9x8_windows(path)?;
    let dhash = compute_dhash_from_gray_9x8(&gray)?;

    let width = (width > 0).then_some(width);
    let height = (height > 0).then_some(height);
    Ok(PerceptualData {
        dhash,
        width,
        height,
    })
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
fn compute_perceptual_data(path: &Path) -> Result<PerceptualData> {
    let _ = path;
    anyhow::bail!("photos --similar is not supported on this platform")
}

#[cfg(target_os = "macos")]
fn read_dimensions_macos(path: &Path) -> Result<(u32, u32)> {
    let output = Command::new("sips")
        .args(["-g", "pixelWidth", "-g", "pixelHeight"])
        .arg(path)
        .output()
        .with_context(|| format!("failed to run sips for {}", path.display()))?;
    if !output.status.success() {
        anyhow::bail!(
            "sips metadata failed for {}: {}",
            path.display(),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let text = String::from_utf8_lossy(&output.stdout);
    let mut width = None;
    let mut height = None;
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some((k, v)) = trimmed.split_once(':') {
            let key = k.trim();
            let value = v.trim();
            if key == "pixelWidth" {
                width = value.parse::<u32>().ok();
            } else if key == "pixelHeight" {
                height = value.parse::<u32>().ok();
            }
        }
    }

    let width = width.context("pixelWidth not found in sips output")?;
    let height = height.context("pixelHeight not found in sips output")?;
    Ok((width, height))
}

#[cfg(target_os = "macos")]
fn render_bmp_9x8_macos(path: &Path) -> Result<Vec<u8>> {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let tmp = std::env::temp_dir().join(format!(
        "smartdup-dhash-{}-{}.bmp",
        std::process::id(),
        nanos
    ));

    let output = Command::new("sips")
        .args(["-s", "format", "bmp", "-z", "8", "9"])
        .arg(path)
        .args(["--out"])
        .arg(&tmp)
        .output()
        .with_context(|| format!("failed to run sips convert for {}", path.display()))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let _ = std::fs::remove_file(&tmp);
        anyhow::bail!("sips convert failed for {}: {}", path.display(), stderr);
    }

    let bytes = std::fs::read(&tmp).with_context(|| format!("failed to read {}", tmp.display()))?;
    let _ = std::fs::remove_file(&tmp);
    Ok(bytes)
}

#[cfg(target_os = "linux")]
fn read_dimensions_linux(path: &Path) -> Result<(u32, u32)> {
    for mut cmd in [
        {
            let mut cmd = Command::new("magick");
            cmd.args(["identify", "-format", "%w %h"]).arg(path);
            cmd
        },
        {
            let mut cmd = Command::new("identify");
            cmd.args(["-format", "%w %h"]).arg(path);
            cmd
        },
    ] {
        match cmd.output() {
            Ok(output) if output.status.success() => {
                let text = String::from_utf8_lossy(&output.stdout);
                let mut parts = text.split_whitespace();
                if let (Some(w), Some(h)) = (parts.next(), parts.next())
                    && let (Ok(width), Ok(height)) = (w.parse::<u32>(), h.parse::<u32>())
                {
                    return Ok((width, height));
                }
            }
            Ok(_) => {}
            Err(_) => {}
        }
    }

    anyhow::bail!("failed to read image dimensions via ImageMagick identify")
}

#[cfg(target_os = "linux")]
fn render_gray_9x8_linux(path: &Path) -> Result<Vec<u8>> {
    let mut errors = Vec::new();
    for program in ["magick", "convert"] {
        let output = Command::new(program)
            .arg(path)
            .args([
                "-alpha",
                "off",
                "-colorspace",
                "Gray",
                "-resize",
                "9x8!",
                "-depth",
                "8",
                "gray:-",
            ])
            .output();

        match output {
            Ok(out) if out.status.success() => {
                if out.stdout.len() >= 72 {
                    return Ok(out.stdout[..72].to_vec());
                }
                errors.push(format!(
                    "{} produced too little output ({} bytes)",
                    program,
                    out.stdout.len()
                ));
            }
            Ok(out) => errors.push(format!(
                "{} failed with status {}: {}",
                program,
                out.status,
                String::from_utf8_lossy(&out.stderr).trim()
            )),
            Err(err) => errors.push(format!("{} launch error: {err}", program)),
        }
    }

    anyhow::bail!(
        "failed to decode image with linux tools (tried magick/convert): {}",
        errors.join("; ")
    )
}

#[cfg(target_os = "windows")]
fn render_gray_9x8_windows(path: &Path) -> Result<(u32, u32, Vec<u8>)> {
    #[derive(Debug, Deserialize)]
    struct PsResult {
        width: u32,
        height: u32,
        gray: Vec<u8>,
    }

    let path_text = path
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("path is not valid UTF-8: {}", path.display()))?;
    let script = "\
$ErrorActionPreference='Stop';\
Add-Type -AssemblyName System.Drawing;\
$p=$env:SMARTDUP_IMAGE_PATH;\
$img=[System.Drawing.Image]::FromFile($p);\
$w=$img.Width;\
$h=$img.Height;\
$bmp=New-Object System.Drawing.Bitmap 9,8;\
$g=[System.Drawing.Graphics]::FromImage($bmp);\
$g.DrawImage($img,0,0,9,8);\
$g.Dispose();\
$img.Dispose();\
$gray=New-Object System.Collections.Generic.List[byte];\
for($y=0;$y -lt 8;$y++){\
  for($x=0;$x -lt 9;$x++){\
    $c=$bmp.GetPixel($x,$y);\
    $v=[byte]([Math]::Round(0.299*$c.R + 0.587*$c.G + 0.114*$c.B));\
    $gray.Add($v);\
  }\
}\
$bmp.Dispose();\
[PSCustomObject]@{width=$w;height=$h;gray=($gray | ForEach-Object {[int]$_})} | ConvertTo-Json -Compress\
";

    let output = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .env("SMARTDUP_IMAGE_PATH", path_text)
        .output()
        .with_context(|| format!("failed to launch powershell for {}", path.display()))?;

    if !output.status.success() {
        anyhow::bail!(
            "powershell image decode failed for {}: {}",
            path.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let parsed: PsResult = serde_json::from_str(&text).with_context(|| {
        format!(
            "failed to parse powershell image output for {}: {}",
            path.display(),
            text
        )
    })?;

    if parsed.gray.len() < 72 {
        anyhow::bail!(
            "powershell decode returned too few grayscale bytes for {}: {}",
            path.display(),
            parsed.gray.len()
        );
    }
    Ok((parsed.width, parsed.height, parsed.gray[..72].to_vec()))
}

fn compute_dhash_from_gray_9x8(gray: &[u8]) -> Result<u64> {
    if gray.len() < 72 {
        anyhow::bail!(
            "grayscale raster must contain at least 72 bytes (got {})",
            gray.len()
        );
    }

    let mut dhash = 0_u64;
    for row in 0..8 {
        for col in 0..8 {
            let left = gray[row * 9 + col];
            let right = gray[row * 9 + col + 1];
            let bit_idx = row * 8 + col;
            if left > right {
                dhash |= 1_u64 << bit_idx;
            }
        }
    }
    Ok(dhash)
}

#[cfg(target_os = "macos")]
fn compute_dhash_from_bmp_9x8(bytes: &[u8]) -> Result<u64> {
    if bytes.len() < 54 || &bytes[0..2] != b"BM" {
        anyhow::bail!("invalid BMP header");
    }

    let read_u16 = |offset: usize| -> Result<u16> {
        let raw = bytes
            .get(offset..offset + 2)
            .context("BMP too short while reading u16")?;
        Ok(u16::from_le_bytes([raw[0], raw[1]]))
    };
    let read_u32 = |offset: usize| -> Result<u32> {
        let raw = bytes
            .get(offset..offset + 4)
            .context("BMP too short while reading u32")?;
        Ok(u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]))
    };
    let read_i32 = |offset: usize| -> Result<i32> {
        let raw = bytes
            .get(offset..offset + 4)
            .context("BMP too short while reading i32")?;
        Ok(i32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]))
    };

    let pixel_offset = read_u32(10)? as usize;
    let width = read_i32(18)?;
    let height = read_i32(22)?;
    let bit_count = read_u16(28)?;
    let compression = read_u32(30)?;

    if width.abs() < 9 || height.abs() < 8 {
        anyhow::bail!("BMP is smaller than required 9x8 raster");
    }
    if compression != 0 {
        anyhow::bail!("BMP compression is unsupported");
    }
    if bit_count != 24 && bit_count != 32 {
        anyhow::bail!("BMP bit depth {} is unsupported", bit_count);
    }

    let width_abs = width.unsigned_abs() as usize;
    let height_abs = height.unsigned_abs() as usize;
    let bytes_per_pixel = (bit_count / 8) as usize;
    let row_stride = (width_abs * bytes_per_pixel).next_multiple_of(4);
    let total_pixels = row_stride
        .checked_mul(height_abs)
        .context("BMP row stride overflow")?;
    if pixel_offset + total_pixels > bytes.len() {
        anyhow::bail!("BMP pixel buffer is truncated");
    }

    let mut gray = [[0_u8; 9]; 8];
    for (row, row_values) in gray.iter_mut().enumerate() {
        let src_row = if height > 0 {
            height_abs - 1 - row
        } else {
            row
        };
        let row_base = pixel_offset + src_row * row_stride;
        for (col, cell) in row_values.iter_mut().enumerate().take(9) {
            let px = row_base + col * bytes_per_pixel;
            let b = bytes[px] as u32;
            let g = bytes[px + 1] as u32;
            let r = bytes[px + 2] as u32;
            *cell = ((299 * r + 587 * g + 114 * b) / 1000) as u8;
        }
    }

    let mut linear_gray = Vec::with_capacity(72);
    for row_values in &gray {
        linear_gray.extend_from_slice(row_values);
    }
    compute_dhash_from_gray_9x8(&linear_gray)
}

#[derive(Debug)]
struct DisjointSet {
    parent: Vec<usize>,
    rank: Vec<u8>,
}

impl DisjointSet {
    fn new(size: usize) -> Self {
        Self {
            parent: (0..size).collect(),
            rank: vec![0; size],
        }
    }

    fn find(&mut self, x: usize) -> usize {
        if self.parent[x] != x {
            let root = self.find(self.parent[x]);
            self.parent[x] = root;
        }
        self.parent[x]
    }

    fn union(&mut self, a: usize, b: usize) {
        let root_a = self.find(a);
        let root_b = self.find(b);
        if root_a == root_b {
            return;
        }

        let rank_a = self.rank[root_a];
        let rank_b = self.rank[root_b];
        if rank_a < rank_b {
            self.parent[root_a] = root_b;
        } else if rank_a > rank_b {
            self.parent[root_b] = root_a;
        } else {
            self.parent[root_b] = root_a;
            self.rank[root_a] += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{SimilarPhoto, group_similar_photos, hamming_distance_u64, is_photo_file};
    use std::path::Path;

    #[test]
    fn recognizes_photo_extensions_case_insensitively() {
        assert!(is_photo_file(Path::new("/tmp/a.JPG")));
        assert!(is_photo_file(Path::new("/tmp/a.heic")));
        assert!(!is_photo_file(Path::new("/tmp/a.txt")));
    }

    #[test]
    fn hamming_distance_counts_different_bits() {
        assert_eq!(hamming_distance_u64(0b1010, 0b1010), 0);
        assert_eq!(hamming_distance_u64(0b1010, 0b1110), 1);
        assert_eq!(hamming_distance_u64(0, u64::MAX), 64);
    }

    #[test]
    fn similar_grouping_merges_transitive_matches() {
        let photos = vec![
            SimilarPhoto {
                path: "/tmp/a.jpg".into(),
                size: 1,
                modified_unix_secs: None,
                width: None,
                height: None,
                dhash: 0b0000,
            },
            SimilarPhoto {
                path: "/tmp/b.jpg".into(),
                size: 1,
                modified_unix_secs: None,
                width: None,
                height: None,
                dhash: 0b0001,
            },
            SimilarPhoto {
                path: "/tmp/c.jpg".into(),
                size: 1,
                modified_unix_secs: None,
                width: None,
                height: None,
                dhash: 0b0011,
            },
            SimilarPhoto {
                path: "/tmp/other.jpg".into(),
                size: 1,
                modified_unix_secs: None,
                width: None,
                height: None,
                dhash: u64::MAX,
            },
        ];

        let groups = group_similar_photos(&photos, 1);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].len(), 3);
    }
}
