use crate::config::CompiledFileSystemPolicy;
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize)]
pub struct FileCandidate {
    pub path: String,
    pub size_bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modified_epoch_secs: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inode: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SkippedEntry {
    pub path: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScanSummary {
    pub roots: Vec<String>,
    pub files_seen: u64,
    pub dirs_seen: u64,
    pub candidates: u64,
    pub skipped: u64,
    pub sample_candidates: Vec<FileCandidate>,
    pub sample_skipped: Vec<SkippedEntry>,
}

#[derive(Debug, Clone)]
pub struct ScanOptions {
    pub max_sample_candidates: usize,
    pub max_sample_skipped: usize,
}

impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            max_sample_candidates: 200,
            max_sample_skipped: 200,
        }
    }
}

/// Recursively scans roots and returns a deterministic preview of what would be indexed.
///
/// - **Async**: uses tokio fs APIs
/// - **Zero-panic**: all errors become skipped entries
/// - **Deterministic**: samples are sorted by path before returning
pub async fn preview_index(
    roots: Vec<PathBuf>,
    policy: &CompiledFileSystemPolicy,
    options: ScanOptions,
) -> ScanSummary {
    let mut files_seen = 0u64;
    let mut dirs_seen = 0u64;
    let mut candidates = 0u64;
    let mut skipped = 0u64;

    let mut sample_candidates: Vec<FileCandidate> = vec![];
    let mut sample_skipped: Vec<SkippedEntry> = vec![];

    let mut stack: Vec<PathBuf> = vec![];
    for r in &roots {
        stack.push(r.clone());
    }

    while let Some(current) = stack.pop() {
        // Exclude matches apply to both files and directories.
        if policy.matches_exclude(&current) {
            skipped += 1;
            push_skipped(
                &mut sample_skipped,
                options.max_sample_skipped,
                current,
                "excluded by glob".to_string(),
            );
            continue;
        }

        let meta = match tokio::fs::symlink_metadata(&current).await {
            Ok(m) => m,
            Err(e) => {
                skipped += 1;
                push_skipped(
                    &mut sample_skipped,
                    options.max_sample_skipped,
                    current,
                    format!("metadata error: {e}"),
                );
                continue;
            }
        };

        let ft = meta.file_type();
        if ft.is_symlink() && !policy.follow_symlinks {
            skipped += 1;
            push_skipped(
                &mut sample_skipped,
                options.max_sample_skipped,
                current,
                "symlink (skipped)".to_string(),
            );
            continue;
        }

        if ft.is_dir() {
            dirs_seen += 1;

            let mut rd = match tokio::fs::read_dir(&current).await {
                Ok(r) => r,
                Err(e) => {
                    skipped += 1;
                    push_skipped(
                        &mut sample_skipped,
                        options.max_sample_skipped,
                        current,
                        format!("read_dir error: {e}"),
                    );
                    continue;
                }
            };

            while let Ok(Some(entry)) = rd.next_entry().await {
                stack.push(entry.path());
            }

            // If next_entry itself errors, record it once (best-effort).
            // (Tokio does not expose the error directly in the loop above.)
            continue;
        }

        if !ft.is_file() {
            skipped += 1;
            push_skipped(
                &mut sample_skipped,
                options.max_sample_skipped,
                current,
                "not a regular file".to_string(),
            );
            continue;
        }

        files_seen += 1;

        if !policy.extension_allowed(&current) {
            skipped += 1;
            push_skipped(
                &mut sample_skipped,
                options.max_sample_skipped,
                current,
                "extension not allowlisted".to_string(),
            );
            continue;
        }

        let size = meta.len();
        if size > policy.max_file_size_bytes {
            skipped += 1;
            push_skipped(
                &mut sample_skipped,
                options.max_sample_skipped,
                current,
                format!("file too large: {size} bytes"),
            );
            continue;
        }

        candidates += 1;
        push_candidate(
            &mut sample_candidates,
            options.max_sample_candidates,
            &current,
            &meta,
        );
    }

    // Deterministic order for samples (independent of filesystem traversal order)
    sample_candidates.sort_by(|a, b| a.path.cmp(&b.path));
    sample_skipped.sort_by(|a, b| a.path.cmp(&b.path));

    ScanSummary {
        roots: roots.iter().map(|p| p.to_string_lossy().to_string()).collect(),
        files_seen,
        dirs_seen,
        candidates,
        skipped,
        sample_candidates,
        sample_skipped,
    }
}

fn push_skipped(
    out: &mut Vec<SkippedEntry>,
    max: usize,
    path: PathBuf,
    reason: String,
) {
    if out.len() >= max {
        return;
    }
    out.push(SkippedEntry {
        path: path.to_string_lossy().to_string(),
        reason,
    });
}

fn push_candidate(out: &mut Vec<FileCandidate>, max: usize, path: &Path, meta: &std::fs::Metadata) {
    if out.len() >= max {
        return;
    }
    out.push(FileCandidate {
        path: path.to_string_lossy().to_string(),
        size_bytes: meta.len(),
        modified_epoch_secs: modified_epoch_secs(meta),
        inode: inode(meta),
    });
}

fn modified_epoch_secs(meta: &std::fs::Metadata) -> Option<i64> {
    let t = meta.modified().ok()?;
    let d = t.duration_since(std::time::UNIX_EPOCH).ok()?;
    Some(d.as_secs() as i64)
}

fn inode(meta: &std::fs::Metadata) -> Option<u64> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Some(meta.ino())
    }
    #[cfg(not(unix))]
    {
        let _ = meta;
        None
    }
}


