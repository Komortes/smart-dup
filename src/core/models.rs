use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntry {
    pub path: PathBuf,
    pub size: u64,
    pub modified_unix_secs: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DuplicateGroup {
    pub file_size: u64,
    pub content_hash: String,
    pub files: Vec<FileEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanSummary {
    pub scanned_files: u64,
    pub candidate_files: u64,
    pub duplicate_groups: u64,
    pub duplicate_files: u64,
    pub reclaimable_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanResult {
    pub roots: Vec<PathBuf>,
    pub generated_at_unix_secs: u64,
    pub summary: ScanSummary,
    pub groups: Vec<DuplicateGroup>,
}
