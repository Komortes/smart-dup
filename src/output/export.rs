use crate::core::models::ScanResult;
use anyhow::{Context, Result};
use serde::Serialize;
use std::fs::File;
use std::path::Path;

pub fn write_json(path: &Path, result: &ScanResult) -> Result<()> {
    let file = File::create(path).with_context(|| format!("create failed: {}", path.display()))?;
    serde_json::to_writer_pretty(file, result)
        .with_context(|| format!("json write failed: {}", path.display()))?;
    Ok(())
}

pub fn write_csv(path: &Path, result: &ScanResult) -> Result<()> {
    let mut writer = csv::Writer::from_path(path)
        .with_context(|| format!("csv create failed: {}", path.display()))?;

    for (group_index, group) in result.groups.iter().enumerate() {
        for file in &group.files {
            let row = CsvRow {
                group_index: group_index + 1,
                content_hash: group.content_hash.clone(),
                file_size: group.file_size,
                path: file.path.to_string_lossy().into_owned(),
                modified_unix_secs: file.modified_unix_secs,
            };
            writer.serialize(row).with_context(|| {
                format!(
                    "csv row serialization failed for {}",
                    file.path.as_path().display()
                )
            })?;
        }
    }

    writer
        .flush()
        .with_context(|| format!("csv flush failed: {}", path.display()))?;
    Ok(())
}

#[derive(Debug, Serialize)]
struct CsvRow {
    group_index: usize,
    content_hash: String,
    file_size: u64,
    path: String,
    modified_unix_secs: Option<u64>,
}
