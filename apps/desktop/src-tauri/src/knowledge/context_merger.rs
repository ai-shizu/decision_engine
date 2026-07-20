//! Daily Context merger (M13).
//!
//! Ports the Markdown assembly philosophy from
//! `src/python/core/data_merger.py::_render_text` for the Pocket Brain path:
//! calendar events + daily log → one dated Markdown document. No iOS EventKit
//! binding here — callers pass JSON / text already obtained elsewhere.
//!
//! Profiler must NOT run from this path (AI_SKILLS §1: 記録と分析の分離).

#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::string_slice
)]

use serde::Deserialize;

/// Soft caps (Jetsam / embed loop / IPC).
pub const MAX_EVENTS_JSON_BYTES: usize = 256 * 1024;
pub const MAX_DAILY_LOG_BYTES: usize = 256 * 1024;
pub const MAX_MERGED_MARKDOWN_BYTES: usize = 512 * 1024;
pub const MAX_EVENT_TITLE_BYTES: usize = 512;
pub const MAX_EVENTS: usize = 200;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MergeError {
    InvalidDate,
    InvalidEventsJson,
    TooLarge,
    EmptyContext,
}

impl std::fmt::Display for MergeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidDate => write!(f, "invalid date_str"),
            Self::InvalidEventsJson => write!(f, "invalid events_json"),
            Self::TooLarge => write!(f, "daily context too large"),
            Self::EmptyContext => write!(f, "empty daily context"),
        }
    }
}

impl std::error::Error for MergeError {}

/// One calendar / schedule row accepted from frontend JSON.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct CalendarEventIn {
    /// Wall-clock time as displayed (e.g. `"09:00"` or `"09:00-10:00"`).
    #[serde(default)]
    pub time: Option<String>,
    /// Event title / summary.
    #[serde(default)]
    pub title: Option<String>,
    /// Optional alternate keys from EventKit-shaped payloads.
    #[serde(default)]
    pub start: Option<String>,
    #[serde(default)]
    pub summary: Option<String>,
}

/// Normalized event ready for Markdown rendering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedEvent {
    pub time: String,
    pub title: String,
}

/// Validate and normalize `YYYY-MM-DD` (zero-padded).
pub fn normalize_date(date_str: &str) -> Result<String, MergeError> {
    let s = date_str.trim();
    let b = s.as_bytes();
    if b.len() != 10 {
        return Err(MergeError::InvalidDate);
    }
    if !matches!(b.get(4), Some(b'-')) || !matches!(b.get(7), Some(b'-')) {
        return Err(MergeError::InvalidDate);
    }
    for (i, c) in b.iter().enumerate() {
        if i == 4 || i == 7 {
            continue;
        }
        if !c.is_ascii_digit() {
            return Err(MergeError::InvalidDate);
        }
    }
    let y: u32 = s.get(0..4).and_then(|x| x.parse().ok()).ok_or(MergeError::InvalidDate)?;
    let mo: u32 = s.get(5..7).and_then(|x| x.parse().ok()).ok_or(MergeError::InvalidDate)?;
    let d: u32 = s.get(8..10).and_then(|x| x.parse().ok()).ok_or(MergeError::InvalidDate)?;
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) || y < 1970 || y > 2100 {
        return Err(MergeError::InvalidDate);
    }
    Ok(format!("{y:04}-{mo:02}-{d:02}"))
}

/// Deterministic vault `source_id` for a calendar day (`daily-YYYY-MM-DD`).
/// Hyphens only — must satisfy `ingest` source_id rules (no `_` / `%` / `::`).
pub fn daily_source_id(date: &str) -> Result<String, MergeError> {
    let date = normalize_date(date)?;
    Ok(format!("daily-{date}"))
}

fn truncate_field(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    s.get(..end).unwrap_or("").to_string()
}

fn event_time(ev: &CalendarEventIn) -> String {
    ev.time
        .as_deref()
        .or(ev.start.as_deref())
        .unwrap_or("")
        .trim()
        .to_string()
}

fn event_title(ev: &CalendarEventIn) -> String {
    ev.title
        .as_deref()
        .or(ev.summary.as_deref())
        .unwrap_or("")
        .trim()
        .to_string()
}

/// Parse `events_json` into sorted, capped, normalized events.
pub fn parse_events_json(events_json: &str) -> Result<Vec<NormalizedEvent>, MergeError> {
    if events_json.len() > MAX_EVENTS_JSON_BYTES {
        return Err(MergeError::TooLarge);
    }
    let trimmed = events_json.trim();
    if trimmed.is_empty() || trimmed == "[]" || trimmed == "null" {
        return Ok(Vec::new());
    }
    let raw: Vec<CalendarEventIn> =
        serde_json::from_str(trimmed).map_err(|_| MergeError::InvalidEventsJson)?;
    let mut out = Vec::new();
    for ev in raw.into_iter().take(MAX_EVENTS) {
        let title = truncate_field(&event_title(&ev), MAX_EVENT_TITLE_BYTES);
        if title.is_empty() {
            continue;
        }
        let time = truncate_field(&event_time(&ev), 64);
        out.push(NormalizedEvent {
            time: if time.is_empty() {
                "--:--".into()
            } else {
                time
            },
            title,
        });
    }
    out.sort_by(|a, b| a.time.cmp(&b.time).then_with(|| a.title.cmp(&b.title)));
    Ok(out)
}

fn format_calendar_section(events: &[NormalizedEvent]) -> String {
    if events.is_empty() {
        return "(この日の予定なし)".into();
    }
    let mut lines = Vec::with_capacity(events.len());
    for e in events {
        lines.push(format!("- {} {}", e.time, e.title));
    }
    lines.join("\n")
}

/// Assemble Daily Context Markdown for one day.
///
/// Layout (Python `_render_text` spirit + M13 dated H2):
/// ```text
/// # DailyContext: {date}
///
/// ## {date}の記録
///
/// ### 予定
/// - HH:MM title
/// …
///
/// ### 日誌
/// {daily_log | placeholder}
/// ```
pub fn build_daily_context_markdown(
    date_str: &str,
    events_json: &str,
    daily_log: &str,
) -> Result<String, MergeError> {
    if daily_log.len() > MAX_DAILY_LOG_BYTES {
        return Err(MergeError::TooLarge);
    }
    let date = normalize_date(date_str)?;
    let events = parse_events_json(events_json)?;
    let log = daily_log.trim();
    let diary = if log.is_empty() {
        "(この日の日誌なし)"
    } else {
        log
    };

    let mut out = String::with_capacity(diary.len().saturating_add(256));
    out.push_str("# DailyContext: ");
    out.push_str(&date);
    out.push_str("\n\n## ");
    out.push_str(&date);
    out.push_str("の記録\n\n### 予定\n");
    out.push_str(&format_calendar_section(&events));
    out.push_str("\n\n### 日誌\n");
    out.push_str(diary);
    out.push('\n');

    if out.len() > MAX_MERGED_MARKDOWN_BYTES {
        return Err(MergeError::TooLarge);
    }
    if events.is_empty() && log.is_empty() {
        return Err(MergeError::EmptyContext);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_date() {
        assert_eq!(normalize_date("2026-07-20").unwrap(), "2026-07-20");
        assert!(normalize_date("2026/07/20").is_err());
        assert!(normalize_date("26-07-20").is_err());
    }

    #[test]
    fn source_id_hyphenated() {
        assert_eq!(
            daily_source_id("2026-07-20").unwrap(),
            "daily-2026-07-20"
        );
    }

    #[test]
    fn merges_events_and_log() {
        let md = build_daily_context_markdown(
            "2026-07-20",
            r#"[{"time":"10:00","title":"面接"},{"time":"09:00","title":"朝会"}]"#,
            "今日は緊張した。",
        )
        .unwrap();
        assert!(md.contains("# DailyContext: 2026-07-20"));
        assert!(md.contains("## 2026-07-20の記録"));
        assert!(md.contains("### 予定"));
        assert!(md.contains("- 09:00 朝会"));
        assert!(md.contains("- 10:00 面接"));
        assert!(md.contains("### 日誌"));
        assert!(md.contains("今日は緊張した。"));
        // time-sorted
        let i_morning = md.find("09:00").unwrap();
        let i_interview = md.find("10:00").unwrap();
        assert!(i_morning < i_interview);
    }

    #[test]
    fn rejects_empty() {
        assert!(matches!(
            build_daily_context_markdown("2026-07-20", "[]", "  "),
            Err(MergeError::EmptyContext)
        ));
    }
}
