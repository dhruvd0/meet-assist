//! Text chunking for the source index. Prefers paragraph boundaries, then
//! lines, then hard splits, so search snippets stay readable.

pub const MAX_CHUNK_CHARS: usize = 1500;

pub fn chunk_text(text: &str, max_chars: usize) -> Vec<String> {
    let max_chars = max_chars.max(1);
    let mut out: Vec<String> = Vec::new();
    let mut current = String::new();

    for para in text.split("\n\n") {
        let para = para.trim();
        if para.is_empty() {
            continue;
        }
        let para_len = para.chars().count();
        if para_len > max_chars {
            flush(&mut out, &mut current);
            for piece in hard_split(para, max_chars) {
                out.push(piece);
            }
            continue;
        }
        let cur_len = current.chars().count();
        let joined_len = if current.is_empty() {
            para_len
        } else {
            cur_len + 2 + para_len
        };
        if joined_len > max_chars {
            flush(&mut out, &mut current);
        }
        if !current.is_empty() {
            current.push_str("\n\n");
        }
        current.push_str(para);
    }
    flush(&mut out, &mut current);
    out
}

fn flush(out: &mut Vec<String>, current: &mut String) {
    let trimmed = current.trim();
    if !trimmed.is_empty() {
        out.push(trimmed.to_string());
    }
    current.clear();
}

fn hard_split(text: &str, max_chars: usize) -> Vec<String> {
    let mut pieces = Vec::new();
    let mut buf = String::new();
    for line in text.split('\n') {
        for ch in line.chars() {
            if buf.chars().count() >= max_chars {
                pieces.push(std::mem::take(&mut buf));
            }
            buf.push(ch);
        }
        if buf.chars().count() >= max_chars {
            pieces.push(std::mem::take(&mut buf));
        } else if !buf.is_empty() {
            buf.push('\n');
        }
    }
    let tail = buf.trim().to_string();
    if !tail.is_empty() {
        pieces.push(tail);
    }
    pieces
        .into_iter()
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_text_is_one_chunk() {
        assert_eq!(chunk_text("hello world", 100), vec!["hello world"]);
    }

    #[test]
    fn splits_on_paragraphs_under_limit() {
        let text = "para one\n\npara two\n\npara three";
        let chunks = chunk_text(text, 12);
        assert_eq!(chunks, vec!["para one", "para two", "para three"]);
    }

    #[test]
    fn packs_paragraphs_until_limit() {
        let text = "aaaa\n\nbbbb\n\ncccc";
        let chunks = chunk_text(text, 11);
        assert_eq!(chunks, vec!["aaaa\n\nbbbb", "cccc"]);
    }

    #[test]
    fn hard_splits_long_paragraph_on_char_boundary() {
        let text = "é".repeat(10);
        let chunks = chunk_text(&text, 4);
        assert_eq!(chunks.len(), 3);
        assert!(chunks.iter().all(|c| c.chars().count() <= 4));
    }

    #[test]
    fn drops_empty_chunks() {
        assert!(chunk_text("\n\n   \n\n", 10).is_empty());
    }
}
