//! Turns a source into searchable chunks. Pure functions over the
//! filesystem and network so the store and manager stay thin.

use crate::meeting::chunk::{chunk_text, MAX_CHUNK_CHARS};
use anyhow::{anyhow, Context, Result};
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
    let bytes = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
    if is_probably_binary(&bytes) {
        return Err(anyhow!("{} looks binary", path.display()));
    }
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
    for entry in walker {
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

pub fn chunks_for_caption(caption: &str) -> Vec<(String, String)> {
    chunks_for_text(caption)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn text_source_yields_chunks_with_empty_path() {
        let chunks = chunks_for_text("one\n\ntwo");
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].0, "");
    }

    #[test]
    fn file_source_uses_file_name_as_path() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("notes.md");
        fs::write(&f, "hello").unwrap();
        let chunks = chunks_for_file(&f).unwrap();
        assert_eq!(chunks[0].0, "notes.md");
        assert_eq!(chunks[0].1, "hello");
    }

    #[test]
    fn file_over_size_cap_is_error() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("big.txt");
        fs::write(&f, vec![b'a'; (MAX_FILE_BYTES + 1) as usize]).unwrap();
        assert!(chunks_for_file(&f).is_err());
    }

    #[test]
    fn folder_respects_gitignore_and_skips_binary_and_hidden() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::write(root.join(".gitignore"), "ignored/\n*.log\n").unwrap();
        fs::create_dir_all(root.join("ignored")).unwrap();
        fs::write(root.join("ignored/secret.txt"), "should not index").unwrap();
        fs::write(root.join("app.log"), "log line").unwrap();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/main.rs"), "fn main() {}").unwrap();
        fs::write(root.join("blob.bin"), b"\x00\x01\x02binary").unwrap();
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::write(root.join(".git/config"), "[core]").unwrap();
        fs::write(root.join("README.md"), "readme").unwrap();

        let chunks = chunks_for_folder(root).unwrap();
        let paths: Vec<&str> = chunks.iter().map(|c| c.0.as_str()).collect();
        assert_eq!(paths, vec!["README.md", "src/main.rs"]);
    }

    #[test]
    fn folder_skips_oversize_files_without_failing() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("big.txt"),
            vec![b'a'; (MAX_FILE_BYTES + 1) as usize],
        )
        .unwrap();
        fs::write(dir.path().join("small.txt"), "ok").unwrap();
        let chunks = chunks_for_folder(dir.path()).unwrap();
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].0, "small.txt");
    }

    #[test]
    fn binary_detection() {
        assert!(is_probably_binary(b"abc\x00def"));
        assert!(!is_probably_binary("plain text é".as_bytes()));
    }

    #[test]
    fn folder_respects_gitignore_without_git_dir() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::write(root.join(".gitignore"), "*.log\n").unwrap();
        fs::write(root.join("app.log"), "log").unwrap();
        fs::write(root.join("keep.txt"), "keep").unwrap();
        let paths: Vec<String> = chunks_for_folder(root)
            .unwrap()
            .into_iter()
            .map(|c| c.0)
            .collect();
        assert_eq!(paths, vec!["keep.txt"]);
    }

    #[test]
    fn folder_skips_dependency_dirs_even_without_gitignore() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("node_modules/pkg")).unwrap();
        fs::write(root.join("node_modules/pkg/index.js"), "x").unwrap();
        fs::create_dir_all(root.join("target")).unwrap();
        fs::write(root.join("target/out.txt"), "y").unwrap();
        fs::write(root.join("main.rs"), "fn main() {}").unwrap();
        let paths: Vec<String> = chunks_for_folder(root)
            .unwrap()
            .into_iter()
            .map(|c| c.0)
            .collect();
        assert_eq!(paths, vec!["main.rs"]);
    }

    #[test]
    fn html_is_stripped_to_text_chunks() {
        let html = "<html><body><h1>Title</h1><p>Body <b>bold</b> text.</p><script>var x=1;</script></body></html>";
        let chunks = html_to_chunks(html);
        let joined = chunks
            .iter()
            .map(|c| c.1.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(joined.contains("Title"));
        assert!(joined.contains("bold"));
        assert!(!joined.contains("var x"));
    }

    #[tokio::test]
    async fn link_to_unreachable_host_is_error_not_panic() {
        let res = chunks_for_link("http://127.0.0.1:9/nothing").await;
        assert!(res.is_err());
    }

    #[test]
    fn image_copy_uses_source_id_and_extension() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("pic.PNG");
        fs::write(&src, b"\x89PNG").unwrap();
        let dest = dir.path().join("images");
        let out = copy_image_into(&src, &dest, 42).unwrap();
        assert_eq!(out.file_name().unwrap().to_string_lossy(), "42.png");
        assert!(out.exists());
    }
}
