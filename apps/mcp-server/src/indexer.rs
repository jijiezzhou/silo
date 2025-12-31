use crate::config::CompiledFileSystemPolicy;
use crate::database::DatabaseHandle;
use crate::embed::EmbedderHandle;
use crate::ingest::process_file;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Semaphore;

#[derive(Debug, Clone, Serialize)]
pub struct IndexSummary {
    pub roots: Vec<String>,
    pub scanned_files: u64,
    pub scanned_dirs: u64,
    pub ingested: u64,
    pub skipped: u64,
    pub errors: u64,
    pub stored: u64,
    pub sample_errors: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct IndexOptions {
    pub max_files: Option<u64>,
    pub concurrency: usize,
    pub max_sample_errors: usize,
}

impl Default for IndexOptions {
    fn default() -> Self {
        Self {
            max_files: None,
            concurrency: 2,
            max_sample_errors: 20,
        }
    }
}

/// MVP bulk indexer: traverse roots and ingest eligible files.
///
/// Notes:
/// - Uses the same policy as preview scan
/// - Limits concurrency to avoid oversubscribing CPU (embedding runs in blocking threads)
pub async fn index_roots(
    roots: Vec<PathBuf>,
    policy: Arc<CompiledFileSystemPolicy>,
    db: DatabaseHandle,
    embedder: EmbedderHandle,
    opts: IndexOptions,
) -> IndexSummary {
    let sem = Arc::new(Semaphore::new(opts.concurrency.max(1)));

    let mut scanned_files = 0u64;
    let mut scanned_dirs = 0u64;
    let mut ingested = 0u64;
    let mut skipped = 0u64;
    let mut errors = 0u64;
    let mut stored = 0u64;
    let mut sample_errors: Vec<String> = vec![];

    let mut stack: Vec<PathBuf> = roots.clone();
    let mut tasks = tokio::task::JoinSet::new();

    let mut ingested_target = opts.max_files.unwrap_or(u64::MAX);

    while let Some(current) = stack.pop() {
        if ingested >= ingested_target {
            break;
        }

        if policy.matches_exclude(&current) {
            skipped += 1;
            continue;
        }

        let meta = match tokio::fs::symlink_metadata(&current).await {
            Ok(m) => m,
            Err(e) => {
                skipped += 1;
                push_err(&mut sample_errors, opts.max_sample_errors, format!("metadata {}: {e}", current.display()));
                continue;
            }
        };

        let ft = meta.file_type();
        if ft.is_symlink() && !policy.follow_symlinks {
            skipped += 1;
            continue;
        }

        if ft.is_dir() {
            scanned_dirs += 1;
            let mut rd = match tokio::fs::read_dir(&current).await {
                Ok(r) => r,
                Err(e) => {
                    skipped += 1;
                    push_err(&mut sample_errors, opts.max_sample_errors, format!("read_dir {}: {e}", current.display()));
                    continue;
                }
            };
            while let Ok(Some(entry)) = rd.next_entry().await {
                stack.push(entry.path());
            }
            continue;
        }

        if !ft.is_file() {
            skipped += 1;
            continue;
        }

        scanned_files += 1;

        if !policy.extension_allowed(&current) {
            skipped += 1;
            continue;
        }

        let size = meta.len();
        if size > policy.max_file_size_bytes {
            skipped += 1;
            continue;
        }

        // Spawn ingestion task (bounded by semaphore)
        let permit = match sem.clone().acquire_owned().await {
            Ok(p) => p,
            Err(_) => break,
        };

        let db = db.clone();
        let embedder = embedder.clone();
        let policy = policy.clone();
        let path_str = current.to_string_lossy().to_string();
        let max_text_bytes = policy.max_text_bytes;
        // NOTE: This is an MVP default. In the next small patch we'll thread per-config values here.
        let chunk_tokens = 500usize;
        let chunk_overlap = 50usize;

        tasks.spawn(async move {
            let _permit = permit;
            let res = process_file(&db, &embedder, &path_str, max_text_bytes, chunk_tokens, chunk_overlap).await;
            (path_str, res)
        });

        // Drain finished tasks opportunistically
        while tasks.len() >= opts.concurrency * 2 {
            if let Some(joined) = tasks.join_next().await {
                match joined {
                    Ok((_path, Ok(stats))) => {
                        ingested += 1;
                        if stats.stored {
                            stored += 1;
                        }
                    }
                    Ok((path, Err(e))) => {
                        errors += 1;
                        push_err(&mut sample_errors, opts.max_sample_errors, format!("ingest {path}: {e}"));
                    }
                    Err(e) => {
                        errors += 1;
                        push_err(&mut sample_errors, opts.max_sample_errors, format!("task join error: {e}"));
                    }
                }
            } else {
                break;
            }
        }
    }

    // Finish remaining tasks
    while let Some(joined) = tasks.join_next().await {
        match joined {
            Ok((_path, Ok(stats))) => {
                ingested += 1;
                if stats.stored {
                    stored += 1;
                }
            }
            Ok((path, Err(e))) => {
                errors += 1;
                push_err(&mut sample_errors, opts.max_sample_errors, format!("ingest {path}: {e}"));
            }
            Err(e) => {
                errors += 1;
                push_err(&mut sample_errors, opts.max_sample_errors, format!("task join error: {e}"));
            }
        }
    }

    IndexSummary {
        roots: roots.iter().map(|p| p.to_string_lossy().to_string()).collect(),
        scanned_files,
        scanned_dirs,
        ingested,
        skipped,
        errors,
        stored,
        sample_errors,
    }
}

fn push_err(out: &mut Vec<String>, max: usize, msg: String) {
    if out.len() < max {
        out.push(msg);
    }
}


