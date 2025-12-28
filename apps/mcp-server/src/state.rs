use crate::config::{
    compile_filesystem_policy, default_config_path, load_or_init_config, CompiledFileSystemPolicy,
    FileSystemSourceConfig, SiloConfig, SourceConfig,
};
use crate::database::DatabaseHandle;
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Shared server state.
///
/// Scalable design: keep "sources" in config, and compile per-source policies for fast checks.
pub struct AppState {
    pub db: DatabaseHandle,
    pub config_path: PathBuf,
    pub config: RwLock<SiloConfig>,
    pub fs_policy: RwLock<Option<CompiledFileSystemPolicy>>,
}

impl AppState {
    pub async fn new(db: DatabaseHandle) -> Result<Arc<Self>, String> {
        let config_path = default_config_path();
        let cfg = load_or_init_config(&config_path).await?;

        let fs_policy = compile_from_config(&cfg)?;

        Ok(Arc::new(Self {
            db,
            config_path,
            config: RwLock::new(cfg),
            fs_policy: RwLock::new(fs_policy),
        }))
    }

    pub async fn get_config_json(&self) -> serde_json::Value {
        let cfg = self.config.read().await;
        json!({
            "configPath": self.config_path.to_string_lossy(),
            "config": &*cfg
        })
    }

    pub async fn set_index_roots(&self, roots: Vec<PathBuf>) -> Result<serde_json::Value, String> {
        let mut cfg = self.config.write().await;

        let mut updated = false;
        for src in &mut cfg.sources {
            if let SourceConfig::FileSystem(fs) = src {
                fs.roots = roots.clone();
                updated = true;
                break;
            }
        }
        if !updated {
            cfg.sources.push(SourceConfig::FileSystem(FileSystemSourceConfig {
                roots,
                ..FileSystemSourceConfig::default()
            }));
        }

        crate::config::save_config(&self.config_path, &cfg).await?;
        let compiled = compile_from_config(&cfg)?;
        *self.fs_policy.write().await = compiled;

        Ok(self.get_config_json().await)
    }

    pub async fn validate_index_config(&self) -> serde_json::Value {
        let cfg = self.config.read().await;
        let mut issues: Vec<String> = vec![];

        // Validate filesystem source roots exist and are directories.
        if let Some(fs) = filesystem_source(&cfg) {
            if fs.roots.is_empty() {
                issues.push("filesystem.roots is empty".to_string());
            }
            for r in &fs.roots {
                let r_str = r.as_path().to_string_lossy();
                match tokio::fs::metadata(r).await {
                    Ok(m) => {
                        if !m.is_dir() {
                            issues.push(format!("root is not a directory: {r_str}"));
                        }
                    }
                    Err(e) => issues.push(format!("cannot access root {r_str}: {e}")),
                }
            }
            if fs.max_file_size_bytes == 0 {
                issues.push("max_file_size_bytes must be > 0".to_string());
            }
            if fs.max_text_bytes == 0 {
                issues.push("max_text_bytes must be > 0".to_string());
            }
        } else {
            issues.push("No filesystem source configured".to_string());
        }

        json!({
            "ok": issues.is_empty(),
            "issues": issues
        })
    }

    pub async fn filesystem_roots(&self) -> Vec<PathBuf> {
        let cfg = self.config.read().await;
        if let Some(fs) = filesystem_source(&cfg) {
            fs.roots.clone()
        } else {
            vec![]
        }
    }

    pub async fn filesystem_config(&self) -> Option<FileSystemSourceConfig> {
        let cfg = self.config.read().await;
        filesystem_source_owned(&cfg)
    }
}

fn filesystem_source(cfg: &SiloConfig) -> Option<&FileSystemSourceConfig> {
    cfg.sources.iter().find_map(|s| match s {
        SourceConfig::FileSystem(fs) => Some(fs),
    })
}

fn filesystem_source_owned(cfg: &SiloConfig) -> Option<FileSystemSourceConfig> {
    cfg.sources.iter().find_map(|s| match s {
        SourceConfig::FileSystem(fs) => Some(fs.clone()),
    })
}

fn compile_from_config(cfg: &SiloConfig) -> Result<Option<CompiledFileSystemPolicy>, String> {
    if let Some(fs) = filesystem_source(cfg) {
        Ok(Some(compile_filesystem_policy(fs)?))
    } else {
        Ok(None)
    }
}

pub type SharedState = Arc<AppState>;

/// Utility: best-effort resolve `~` prefix if present.
pub fn expand_tilde(path: &str) -> PathBuf {
    if path == "~" {
        return std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| ".".into());
    }
    if let Some(stripped) = path.strip_prefix("~/") {
        let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| ".".into());
        return home.join(stripped);
    }
    PathBuf::from(path)
}


