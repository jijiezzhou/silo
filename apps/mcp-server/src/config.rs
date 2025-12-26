use globset::{Glob, GlobSet, GlobSetBuilder};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Top-level configuration. Keep this extensible: new sources (messages/apps) will become new entries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SiloConfig {
    #[serde(default)]
    pub sources: Vec<SourceConfig>,
}

impl Default for SiloConfig {
    fn default() -> Self {
        Self {
            sources: vec![SourceConfig::FileSystem(FileSystemSourceConfig::default())],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SourceConfig {
    /// Local filesystem indexing (Phase 2 MVP: everything under `~` with safe exclusions).
    FileSystem(FileSystemSourceConfig),

    // Placeholder for future sources (messages, apps, calendars, etc).
    // Keep as an enum variant later (e.g. `Messages(MessagesSourceConfig)`).
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileSystemSourceConfig {
    /// Root directories to index. MVP default: `~`.
    #[serde(default)]
    pub roots: Vec<PathBuf>,

    /// Glob patterns (matched against full paths) to exclude from indexing.
    #[serde(default)]
    pub exclude_globs: Vec<String>,

    /// Only ingest files with these extensions (case-insensitive, without dot).
    /// Empty means "allow common text-like extensions" (default list).
    #[serde(default)]
    pub allow_extensions: Vec<String>,

    /// Max file size to consider (bytes).
    #[serde(default = "default_max_file_size_bytes")]
    pub max_file_size_bytes: u64,

    /// Max extracted text to keep in memory per file (bytes/chars approximation).
    #[serde(default = "default_max_text_bytes")]
    pub max_text_bytes: u64,

    /// Whether to follow symlinks (generally false for safety).
    #[serde(default)]
    pub follow_symlinks: bool,
}

impl Default for FileSystemSourceConfig {
    fn default() -> Self {
        let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| ".".into());
        Self {
            roots: vec![home],
            exclude_globs: default_exclude_globs(),
            allow_extensions: default_allow_extensions(),
            max_file_size_bytes: default_max_file_size_bytes(),
            max_text_bytes: default_max_text_bytes(),
            follow_symlinks: false,
        }
    }
}

fn default_max_file_size_bytes() -> u64 {
    10 * 1024 * 1024 // 10MB
}

fn default_max_text_bytes() -> u64 {
    2 * 1024 * 1024 // 2MB extracted text cap
}

fn default_exclude_globs() -> Vec<String> {
    vec![
        // VCS + build + deps
        "**/.git/**".into(),
        "**/node_modules/**".into(),
        "**/target/**".into(),
        "**/.venv/**".into(),
        "**/venv/**".into(),
        "**/__pycache__/**".into(),
        // OS cruft
        "**/.DS_Store".into(),
        // Secrets (very conservative; refine later)
        "**/.env".into(),
        "**/.env.*".into(),
        "**/*.key".into(),
        "**/*.pem".into(),
        "**/*id_rsa*".into(),
        // Big hidden caches
        "**/.cache/**".into(),
        "**/Library/**".into(), // macOS app support can be huge/noisy; user can re-add if desired
    ]
}

fn default_allow_extensions() -> Vec<String> {
    vec![
        "txt", "md", "rst",
        "rs", "toml", "json", "yaml", "yml",
        "py", "js", "ts", "tsx", "jsx",
        "java", "kt", "go", "rb", "php",
        "html", "css", "scss",
        "sql",
        "pdf",
    ]
    .into_iter()
    .map(|s| s.to_string())
    .collect()
}

#[derive(Clone)]
pub struct CompiledFileSystemPolicy {
    pub exclude: GlobSet,
    pub allow_extensions: Vec<String>,
    pub max_file_size_bytes: u64,
    pub max_text_bytes: u64,
    pub follow_symlinks: bool,
}

impl CompiledFileSystemPolicy {
    pub fn matches_exclude(&self, path: &Path) -> bool {
        self.exclude.is_match(path)
    }

    pub fn extension_allowed(&self, path: &Path) -> bool {
        let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
            return false;
        };
        let ext = ext.to_ascii_lowercase();
        self.allow_extensions.iter().any(|e| e == &ext)
    }
}

pub fn compile_filesystem_policy(cfg: &FileSystemSourceConfig) -> Result<CompiledFileSystemPolicy, String> {
    let mut builder = GlobSetBuilder::new();
    for pat in &cfg.exclude_globs {
        let glob = Glob::new(pat).map_err(|e| format!("Invalid exclude glob `{pat}`: {e}"))?;
        builder.add(glob);
    }
    let exclude = builder.build().map_err(|e| format!("Failed to build globset: {e}"))?;

    let allow_extensions = if cfg.allow_extensions.is_empty() {
        default_allow_extensions()
    } else {
        cfg.allow_extensions
            .iter()
            .map(|s| s.trim_start_matches('.').to_ascii_lowercase())
            .filter(|s| !s.is_empty())
            .collect()
    };

    Ok(CompiledFileSystemPolicy {
        exclude,
        allow_extensions,
        max_file_size_bytes: cfg.max_file_size_bytes,
        max_text_bytes: cfg.max_text_bytes,
        follow_symlinks: cfg.follow_symlinks,
    })
}

/// Location for config. Keep it simple and predictable:
/// - `SILO_CONFIG_PATH` overrides
/// - default: `~/.config/silo/config.json`
pub fn default_config_path() -> PathBuf {
    if let Some(p) = std::env::var_os("SILO_CONFIG_PATH") {
        return PathBuf::from(p);
    }
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| ".".into());
    home.join(".config").join("silo").join("config.json")
}

pub async fn load_or_init_config(path: &Path) -> Result<SiloConfig, String> {
    match tokio::fs::read_to_string(path).await {
        Ok(s) => serde_json::from_str::<SiloConfig>(&s).map_err(|e| format!("Invalid config JSON: {e}")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let cfg = SiloConfig::default();
            save_config(path, &cfg).await?;
            Ok(cfg)
        }
        Err(e) => Err(format!("Failed to read config {}: {e}", path.display())),
    }
}

pub async fn save_config(path: &Path, cfg: &SiloConfig) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| format!("Failed to create config dir {}: {e}", parent.display()))?;
    }
    let s = serde_json::to_string_pretty(cfg).map_err(|e| format!("Failed to serialize config: {e}"))?;
    tokio::fs::write(path, s)
        .await
        .map_err(|e| format!("Failed to write config {}: {e}", path.display()))
}


