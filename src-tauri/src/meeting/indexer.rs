//! Turns a source into searchable chunks. Pure functions over the
//! filesystem and network so the store and manager stay thin.

use crate::meeting::chunk::{chunk_text, MAX_CHUNK_CHARS};
use anyhow::{anyhow, Context, Result};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const MAX_FILE_BYTES: u64 = 524_288;

/// Directories never indexed, regardless of .gitignore. Dependency and
/// build output trees are huge and never useful meeting context.
pub const SKIP_DIRS: &[&str] = &[
    "node_modules",
    "target",
    ".venv",
    "venv",
    "dist",
    "build",
    "__pycache__",
];

/// Upper bound on files read per folder source. Hit the cap and the walk
/// stops with a warning; the source still indexes what was read.
pub const MAX_FOLDER_FILES: usize = 5000;
/// Upper bound on walk entries (files, dirs, skipped files) visited per
/// folder source, so a huge tree of unreadable files still terminates fast.
pub const MAX_FOLDER_ENTRIES: usize = 50_000;
/// Upper bound on total text bytes accepted per folder source.
pub const MAX_FOLDER_BYTES: u64 = 50 * 1024 * 1024;

/// Bytes sniffed for NUL before a file is read in full.
const BINARY_SNIFF_BYTES: u64 = 8192;

pub fn is_probably_binary(bytes: &[u8]) -> bool {
    bytes.iter().take(8192).any(|b| *b == 0)
}

pub fn chunks_for_text(text: &str) -> Vec<(String, String)> {
    chunk_text(text, MAX_CHUNK_CHARS)
        .into_iter()
        .map(|c| (String::new(), c))
        .collect()
}

fn read_text_file(path: &Path) -> Result<String> {
    let meta = std::fs::metadata(path).with_context(|| format!("stat {}", path.display()))?;
    if meta.len() > MAX_FILE_BYTES {
        return Err(anyhow!(
            "{} is {} bytes, over the {} byte limit",
            path.display(),
            meta.len(),
            MAX_FILE_BYTES
        ));
    }
    let mut file = std::fs::File::open(path).with_context(|| format!("open {}", path.display()))?;
    let mut bytes = Vec::with_capacity(meta.len() as usize);
    (&mut file)
        .take(BINARY_SNIFF_BYTES)
        .read_to_end(&mut bytes)
        .with_context(|| format!("read {}", path.display()))?;
    if is_probably_binary(&bytes) {
        return Err(anyhow!("{} looks binary", path.display()));
    }
    // Cap the rest too, in case the file grew after the stat.
    file.take(MAX_FILE_BYTES.saturating_sub(bytes.len() as u64))
        .read_to_end(&mut bytes)
        .with_context(|| format!("read {}", path.display()))?;
    String::from_utf8(bytes).map_err(|_| anyhow!("{} is not UTF-8", path.display()))
}

pub fn chunks_for_file(path: &Path) -> Result<Vec<(String, String)>> {
    let text = read_text_file(path)?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    Ok(chunk_text(&text, MAX_CHUNK_CHARS)
        .into_iter()
        .map(|c| (name.clone(), c))
        .collect())
}

pub fn chunks_for_folder(root: &Path) -> Result<Vec<(String, String)>> {
    if !root.is_dir() {
        return Err(anyhow!("{} is not a directory", root.display()));
    }
    let mut out: Vec<(String, String)> = Vec::new();
    let walker = ignore::WalkBuilder::new(root)
        .hidden(true)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .require_git(false)
        .filter_entry(|e| {
            !(e.file_type().map(|t| t.is_dir()).unwrap_or(false)
                && e.file_name()
                    .to_str()
                    .map(|n| SKIP_DIRS.contains(&n))
                    .unwrap_or(false))
        })
        .build();
    let mut files_read = 0usize;
    let mut entries_seen = 0usize;
    let mut bytes_read = 0u64;
    for entry in walker {
        entries_seen += 1;
        if entries_seen > MAX_FOLDER_ENTRIES {
            log::warn!(
                "folder {} hit the {} entry limit; indexing what was read",
                root.display(),
                MAX_FOLDER_ENTRIES
            );
            break;
        }
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                log::debug!("skip walk entry: {}", e);
                continue;
            }
        };
        if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
            continue;
        }
        let path = entry.path();
        let text = match read_text_file(path) {
            Ok(t) => t,
            Err(e) => {
                log::debug!("skip {}: {}", path.display(), e);
                continue;
            }
        };
        files_read += 1;
        bytes_read += text.len() as u64;
        let rel = path
            .strip_prefix(root)
            .unwrap_or(path)
            .components()
            .map(|c| c.as_os_str().to_string_lossy().to_string())
            .collect::<Vec<_>>()
            .join("/");
        for c in chunk_text(&text, MAX_CHUNK_CHARS) {
            out.push((rel.clone(), c));
        }
        if files_read >= MAX_FOLDER_FILES {
            log::warn!(
                "folder {} hit the {} file limit; indexing what was read",
                root.display(),
                MAX_FOLDER_FILES
            );
            break;
        }
        if bytes_read >= MAX_FOLDER_BYTES {
            log::warn!(
                "folder {} hit the {} byte limit; indexing what was read",
                root.display(),
                MAX_FOLDER_BYTES
            );
            break;
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

pub fn html_to_chunks(html: &str) -> Vec<(String, String)> {
    let text = html2text::from_read(html.as_bytes(), 100).unwrap_or_default();
    chunks_for_text(&text)
}

pub async fn chunks_for_link(url: &str) -> Result<Vec<(String, String)>> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .user_agent("MeetAssist/0.1")
        .build()?;
    let resp = client
        .get(url)
        .send()
        .await
        .with_context(|| format!("fetch {}", url))?;
    let status = resp.status();
    if !status.is_success() {
        return Err(anyhow!("{} returned HTTP {}", url, status.as_u16()));
    }
    let content_type = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase();
    let body = resp
        .text()
        .await
        .with_context(|| format!("read body of {}", url))?;
    if content_type.contains("text/html") || content_type.contains("application/xhtml") {
        Ok(html_to_chunks(&body))
    } else if content_type.starts_with("text/") || content_type.is_empty() {
        Ok(chunks_for_text(&body))
    } else {
        Err(anyhow!(
            "{} is {}, only HTML and plain text links are supported",
            url,
            content_type
        ))
    }
}

pub fn copy_image_into(source_path: &Path, dest_dir: &Path, source_id: i64) -> Result<PathBuf> {
    std::fs::create_dir_all(dest_dir)?;
    let ext = source_path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_else(|| "img".to_string());
    let dest = dest_dir.join(format!("{}.{}", source_id, ext));
    std::fs::copy(source_path, &dest)
        .with_context(|| format!("copy {} to {}", source_path.display(), dest.display()))?;
    Ok(dest)
}

pub fn image_digest_prompt() -> String {
    "Describe this image for someone who cannot see it, in at most 120 words: what it shows, any text in it, and why it might matter in a meeting.".to_string()
}

/// Anthropic caps the base64 payload at 5 MB; base64 inflates by 4/3, so
/// 3.7 MB of raw bytes (about 4.93 MB encoded) stays under it.
const MAX_VISION_IMAGE_BYTES: u64 = 3_700_000;

/// Read an image for a vision request: known type, at most 3.7 MB.
pub fn read_image_for_vision(path: &Path) -> Result<(&'static str, Vec<u8>)> {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    let mime = crate::llm_client::mime_for_extension(&ext)
        .ok_or_else(|| anyhow!("unsupported image type: {:?}", ext))?;
    let len = std::fs::metadata(path)
        .with_context(|| format!("stat {}", path.display()))?
        .len();
    if len > MAX_VISION_IMAGE_BYTES {
        return Err(anyhow!("image too large for vision: {} bytes", len));
    }
    let bytes = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
    Ok((mime, bytes))
}

pub fn chunks_for_caption(caption: &str) -> Vec<(String, String)> {
    chunks_for_text(caption)
}
