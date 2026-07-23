//! Markdown / plain-text chunking for on-device RAG (M10).
//!
//! Ports the `##` section-split philosophy from
//! `src/python/core/consultation_engine.py::load_knowledge_chunks`, with a
//! paragraph fallback so header-less documents still become bounded chunks.

/// Soft upper bound per chunk body (bytes). Oversized `##` sections are split
/// further on blank-line paragraph boundaries.
pub const MAX_CHUNK_CHARS: usize = 4_000;

/// Hard cap on chunks produced from one ingest call (Jetsam / embed loop bound).
pub const MAX_CHUNKS: usize = 128;

/// One knowledge chunk ready for embedding + vault insert.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextChunk {
    /// Deterministic id: `{source_id}::{index:04}`.
    pub id: String,
    /// Section title (source id, or `source § heading`).
    pub title: String,
    /// Chunk body text (non-empty, trimmed).
    pub text: String,
}

/// Split `text` into chunks (capped at [`MAX_CHUNKS`]).
pub fn chunk_markdown(text: &str, source_id: &str) -> Vec<TextChunk> {
    chunk_markdown_capped(text, source_id, MAX_CHUNKS)
}

/// Split `text` into chunks with an explicit cap (`usize::MAX` = no practical trunc).
///
/// Rules (Python-compatible core):
/// 1. Lines matching `^##\s+…` start a new section; prior body is flushed.
/// 2. Text before the first `##` belongs to a section titled `source_id`.
/// 3. After a `##` heading, the section title becomes `{source_id} § {heading}`.
/// 4. Empty bodies are dropped.
/// 5. Bodies longer than [`MAX_CHUNK_CHARS`] are split on blank-line paragraphs
///    (then hard-sliced if a single paragraph still overflows).
/// 6. Output is truncated to `max_chunks` (LINE full-import uses a high cap then
///    batches into multiple `source_id` parts).
pub fn chunk_markdown_capped(text: &str, source_id: &str, max_chunks: usize) -> Vec<TextChunk> {
    let source_id = source_id.trim();
    if source_id.is_empty() || text.is_empty() || max_chunks == 0 {
        return Vec::new();
    }

    let mut sections: Vec<(String, String)> = Vec::new();
    let mut title = source_id.to_string();
    let mut body: Vec<&str> = Vec::new();

    for line in text.lines() {
        if let Some(heading) = parse_h2_heading(line) {
            flush_section(&mut sections, &title, &body);
            title = format!("{source_id} § {heading}");
            body.clear();
        } else {
            body.push(line);
        }
    }
    flush_section(&mut sections, &title, &body);

    let mut chunks = Vec::new();
    for (section_title, section_body) in sections {
        for piece in split_oversized(&section_body) {
            if chunks.len() >= max_chunks {
                return chunks;
            }
            let index = chunks.len();
            chunks.push(TextChunk {
                id: format!("{source_id}::{index:04}"),
                title: section_title.clone(),
                text: piece,
            });
        }
        if chunks.len() >= max_chunks {
            break;
        }
    }
    chunks
}

fn parse_h2_heading(line: &str) -> Option<String> {
    // Python: re.match(r"^##\s+(.*)", line) — H3+ must not match.
    let line = line.trim_end();
    let rest = line.strip_prefix("##")?;
    if rest.starts_with('#') {
        return None;
    }
    if !rest.starts_with(' ') && !rest.starts_with('\t') {
        return None;
    }
    let heading = rest.trim();
    if heading.is_empty() {
        None
    } else {
        Some(heading.to_string())
    }
}

fn flush_section(out: &mut Vec<(String, String)>, title: &str, body: &[&str]) {
    let joined = body.join("\n");
    let trimmed = joined.trim();
    if trimmed.is_empty() {
        return;
    }
    out.push((title.to_string(), trimmed.to_string()));
}

fn split_oversized(body: &str) -> Vec<String> {
    if body.len() <= MAX_CHUNK_CHARS {
        return vec![body.to_string()];
    }

    let mut parts = Vec::new();
    let mut buf = String::new();
    for para in body.split("\n\n") {
        let para = para.trim();
        if para.is_empty() {
            continue;
        }
        if buf.is_empty() {
            if para.len() <= MAX_CHUNK_CHARS {
                buf.push_str(para);
            } else {
                parts.extend(hard_slice(para));
            }
            continue;
        }
        if buf.len() + 2 + para.len() <= MAX_CHUNK_CHARS {
            buf.push_str("\n\n");
            buf.push_str(para);
        } else {
            parts.push(std::mem::take(&mut buf));
            if para.len() <= MAX_CHUNK_CHARS {
                buf.push_str(para);
            } else {
                parts.extend(hard_slice(para));
            }
        }
    }
    if !buf.is_empty() {
        parts.push(buf);
    }
    if parts.is_empty() {
        parts.extend(hard_slice(body));
    }
    parts
}

fn hard_slice(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut start = 0;
    let bytes = text.as_bytes();
    while start < bytes.len() {
        let mut end = (start + MAX_CHUNK_CHARS).min(bytes.len());
        if end < bytes.len() {
            // Back up to a char boundary.
            while end > start && !text.is_char_boundary(end) {
                end -= 1;
            }
        }
        if end == start {
            end = (start + 1).min(bytes.len());
            while end < bytes.len() && !text.is_char_boundary(end) {
                end += 1;
            }
        }
        let slice = &text[start..end];
        let trimmed = slice.trim();
        if !trimmed.is_empty() {
            out.push(trimmed.to_string());
        }
        start = end;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_on_h2_headers_like_python() {
        let text = "preamble line\n## Alpha\nbody a\n## Beta\nbody b\n";
        let chunks = chunk_markdown(text, "doc");
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].title, "doc");
        assert!(chunks[0].text.contains("preamble"));
        assert_eq!(chunks[1].title, "doc § Alpha");
        assert_eq!(chunks[1].text, "body a");
        assert_eq!(chunks[2].title, "doc § Beta");
        assert_eq!(chunks[2].id, "doc::0002");
    }

    #[test]
    fn ignores_h3_as_section_break() {
        let text = "## Keep\n### nested\nstill here\n";
        let chunks = chunk_markdown(text, "x");
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].text.contains("### nested"));
    }

    #[test]
    fn empty_source_or_text_yields_nothing() {
        assert!(chunk_markdown("hi", "").is_empty());
        assert!(chunk_markdown("", "src").is_empty());
    }
}
