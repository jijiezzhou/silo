use std::path::Path;
use tokio::process::Command;

#[derive(Debug, Clone)]
pub enum ExtractKind {
    Text,
    Pdf,
    Unknown,
}

#[derive(Debug, Clone)]
pub struct ExtractResult {
    pub kind: ExtractKind,
    pub text: String,
    pub truncated: bool,
}

pub async fn extract_text(path: &Path, max_text_bytes: u64) -> Result<ExtractResult, String> {
    let kind = detect_kind(path);
    match kind {
        ExtractKind::Pdf => extract_pdf_pdftotext(path, max_text_bytes).await,
        ExtractKind::Text => extract_plain_text(path, max_text_bytes).await,
        ExtractKind::Unknown => {
            // Still try as plain text; caller can choose to gate by extension.
            extract_plain_text(path, max_text_bytes).await
        }
    }
}

fn detect_kind(path: &Path) -> ExtractKind {
    let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
        return ExtractKind::Unknown;
    };
    match ext.to_ascii_lowercase().as_str() {
        "pdf" => ExtractKind::Pdf,
        _ => ExtractKind::Text,
    }
}

async fn extract_plain_text(path: &Path, max_text_bytes: u64) -> Result<ExtractResult, String> {
    // Read bytes so we can truncate safely without UTF-8 errors.
    let bytes = tokio::fs::read(path)
        .await
        .map_err(|e| format!("Failed to read file {}: {e}", path.display()))?;

    let (bytes, truncated) = truncate_bytes(bytes, max_text_bytes);
    let text = String::from_utf8_lossy(&bytes).to_string();

    Ok(ExtractResult {
        kind: ExtractKind::Text,
        text,
        truncated,
    })
}

async fn extract_pdf_pdftotext(path: &Path, max_text_bytes: u64) -> Result<ExtractResult, String> {
    // Requires poppler's `pdftotext` to be installed (brew install poppler).
    // `pdftotext <in.pdf> -` writes the extracted text to stdout.
    let output = Command::new("pdftotext")
        .arg("-layout")
        .arg("-nopgbrk")
        .arg(path)
        .arg("-")
        .output()
        .await
        .map_err(|e| {
            format!(
                "Failed to run pdftotext (is poppler installed?). Try `brew install poppler`. Details: {e}"
            )
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "pdftotext failed for {} (exit={}): {}",
            path.display(),
            output.status,
            stderr.trim()
        ));
    }

    let (bytes, truncated) = truncate_bytes(output.stdout, max_text_bytes);
    let text = String::from_utf8_lossy(&bytes).to_string();

    Ok(ExtractResult {
        kind: ExtractKind::Pdf,
        text,
        truncated,
    })
}

fn truncate_bytes(mut bytes: Vec<u8>, max_bytes: u64) -> (Vec<u8>, bool) {
    let max = max_bytes as usize;
    if bytes.len() <= max {
        return (bytes, false);
    }
    bytes.truncate(max);
    (bytes, true)
}


