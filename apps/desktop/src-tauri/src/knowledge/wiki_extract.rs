//! Second fixed Wikipedia outbound template: page extract by title.
//!
//! Independent of [`super::net_gateway::build_request`] / `validate_outbound_url`
//! (search lane). Same strictness: exact key set, no duplicates, send-time
//! re-validation. Does **not** name the HTTP client crate — transport goes
//! through [`super::net_gateway::HttpTransport`] only.

use std::collections::BTreeSet;

use super::net_gateway::{percent_encode_query_param, GatewayError, WIKI_HOST, WIKI_PATH};

/// MediaWiki `exchars` cap for plain-text extracts (fixed template constant).
pub const WIKI_EXTRACT_CHARS: usize = 20_000;

/// Extract-lane wall-clock budget (seconds). Mirrored by the research caller.
pub const WIKI_EXTRACT_DEADLINE_SECS: u64 = 15;

/// Outbound search/extract raw material: company name only.
///
/// Constructible solely from a company-name `&str`. Vault rows / profile structs
/// have no `From`/`Into` impls — that is the PII boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompanyNameForWiki {
    name: String,
}

impl CompanyNameForWiki {
    pub fn from_company_name(name: &str) -> Result<Self, GatewayError> {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return Err(GatewayError::UrlViolation);
        }
        Ok(Self {
            name: trimmed.to_string(),
        })
    }

    pub fn as_str(&self) -> &str {
        &self.name
    }
}

fn percent_decode(s: &str) -> Result<String, GatewayError> {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::new();
    let mut i = 0usize;
    while i < bytes.len() {
        let b = *bytes.get(i).ok_or(GatewayError::UrlViolation)?;
        if b == b'%' {
            let h1 = *bytes.get(i + 1).ok_or(GatewayError::UrlViolation)?;
            let h2 = *bytes.get(i + 2).ok_or(GatewayError::UrlViolation)?;
            let hi = (h1 as char).to_digit(16).ok_or(GatewayError::UrlViolation)?;
            let lo = (h2 as char).to_digit(16).ok_or(GatewayError::UrlViolation)?;
            let byte = u8::try_from(hi * 16 + lo).map_err(|_| GatewayError::UrlViolation)?;
            out.push(byte);
            i += 3;
        } else {
            out.push(b);
            i += 1;
        }
    }
    String::from_utf8(out).map_err(|_| GatewayError::UrlViolation)
}

/// Build the extract-by-title URL (second fixed template).
pub fn build_extract_request(title: &str) -> String {
    format!(
        "https://{WIKI_HOST}{WIKI_PATH}?action=query&format=json&prop=extracts&explaintext=1&exsectionformat=wiki&exlimit=1&exchars={WIKI_EXTRACT_CHARS}&redirects=1&titles={}",
        percent_encode_query_param(title)
    )
}

/// Send-time assertion for the extract template (mirrors search-lane strictness).
pub fn validate_extract_url(url: &str, expected_title: &str) -> Result<(), GatewayError> {
    let prefix = format!("https://{WIKI_HOST}{WIKI_PATH}?");
    let rest = url.strip_prefix(&prefix).ok_or(GatewayError::UrlViolation)?;
    if rest.contains('#') || rest.contains('@') {
        return Err(GatewayError::UrlViolation);
    }
    let expected_keys: BTreeSet<&str> = [
        "action",
        "format",
        "prop",
        "explaintext",
        "exsectionformat",
        "exlimit",
        "exchars",
        "redirects",
        "titles",
    ]
    .into_iter()
    .collect();
    let mut got_keys: BTreeSet<&str> = BTreeSet::new();
    let mut titles_val: Option<&str> = None;
    for pair in rest.split('&') {
        let mut it = pair.splitn(2, '=');
        let k = it.next().ok_or(GatewayError::UrlViolation)?;
        let v = it.next().ok_or(GatewayError::UrlViolation)?;
        if !got_keys.insert(k) {
            return Err(GatewayError::UrlViolation);
        }
        if k == "titles" {
            titles_val = Some(v);
        }
    }
    if got_keys != expected_keys {
        return Err(GatewayError::UrlViolation);
    }
    let decoded = percent_decode(titles_val.ok_or(GatewayError::UrlViolation)?)?;
    if decoded != expected_title {
        return Err(GatewayError::UrlViolation);
    }
    Ok(())
}

/// Parse MediaWiki `prop=extracts` JSON. `query.pages` is an object keyed by page id.
pub fn extract_page_text(body: &[u8]) -> Result<String, GatewayError> {
    let root: serde_json::Value =
        serde_json::from_slice(body).map_err(|_| GatewayError::Malformed)?;
    let pages = root
        .get("query")
        .and_then(|q| q.get("pages"))
        .and_then(|p| p.as_object())
        .ok_or(GatewayError::Malformed)?;
    // Take the first page entry (exlimit=1).
    let mut page_iter = pages.values();
    let page = page_iter.next().ok_or(GatewayError::Malformed)?;
    if page.get("missing").is_some() {
        return Err(GatewayError::Malformed);
    }
    let extract = page
        .get("extract")
        .and_then(|v| v.as_str())
        .ok_or(GatewayError::Malformed)?;
    if extract.trim().is_empty() {
        return Err(GatewayError::Malformed);
    }
    Ok(extract.to_string())
}

/// Convert MediaWiki `exsectionformat=wiki` headings (`== H ==`) to `## H`
/// so [`crate::rag::chunk::chunk_markdown`] can split on H2. `===` and deeper
/// are left unchanged (chunker ignores `###`).
pub fn wiki_sections_to_markdown(plain: &str) -> String {
    let mut out = String::with_capacity(plain.len());
    for (i, line) in plain.lines().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let trimmed = line.trim();
        if let Some(heading) = parse_wiki_h2(trimmed) {
            out.push_str("## ");
            out.push_str(heading);
        } else {
            out.push_str(line);
        }
    }
    out
}

/// Match `== heading ==` (exactly two equals each side). Not `===`.
fn parse_wiki_h2(line: &str) -> Option<&str> {
    let t = line.trim();
    if !t.starts_with("==") || t.starts_with("===") {
        return None;
    }
    if !t.ends_with("==") || t.ends_with("===") {
        return None;
    }
    // Strip exactly one `==` from each side.
    let inner = t.get(2..t.len().checked_sub(2)?)?;
    if inner.starts_with('=') || inner.ends_with('=') {
        return None;
    }
    let heading = inner.trim();
    if heading.is_empty() {
        return None;
    }
    Some(heading)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_extract_passes_validate() {
        let title = "マッキンゼー・アンド・カンパニー";
        let url = build_extract_request(title);
        assert!(validate_extract_url(&url, title).is_ok());
    }

    #[test]
    fn validate_rejects_key_delete_add_duplicate() {
        let title = "Test";
        let good = build_extract_request(title);
        // delete
        let deleted = good.replacen("&redirects=1", "", 1);
        assert_eq!(
            validate_extract_url(&deleted, title),
            Err(GatewayError::UrlViolation)
        );
        // add
        let added = format!("{good}&extra=1");
        assert_eq!(
            validate_extract_url(&added, title),
            Err(GatewayError::UrlViolation)
        );
        // duplicate
        let dup = format!("{good}&titles=Other");
        assert_eq!(
            validate_extract_url(&dup, title),
            Err(GatewayError::UrlViolation)
        );
    }

    #[test]
    fn validate_rejects_titles_tamper() {
        let url = build_extract_request("Alpha");
        assert_eq!(
            validate_extract_url(&url, "Beta"),
            Err(GatewayError::UrlViolation)
        );
    }

    #[test]
    fn extract_page_text_ok_and_missing() {
        let ok = r#"{"query":{"pages":{"123":{"pageid":123,"title":"T","extract":"本文です。"}}}}"#;
        assert_eq!(extract_page_text(ok.as_bytes()).unwrap(), "本文です。");

        let missing =
            br#"{"query":{"pages":{"-1":{"title":"No","missing":""}}}}"#;
        assert_eq!(extract_page_text(missing), Err(GatewayError::Malformed));

        // Adversarial: pages as array — must not panic.
        let hostile = br#"{"query":{"pages":[{"extract":"x"}]}}"#;
        assert_eq!(extract_page_text(hostile), Err(GatewayError::Malformed));
    }

    #[test]
    fn wiki_sections_h2_converts_h3_unchanged() {
        let plain = "== 概要 ==\n本文\n=== 詳細 ===\n細部";
        let md = wiki_sections_to_markdown(plain);
        assert!(md.contains("## 概要"));
        assert!(md.contains("=== 詳細 ==="));
        assert!(!md.contains("## 詳細"));
    }

    #[test]
    fn company_name_for_wiki_is_only_company_name_boundary() {
        // Positive: company name string is the sole constructor input.
        let q = CompanyNameForWiki::from_company_name("  トヨタ自動車  ").unwrap();
        assert_eq!(q.as_str(), "トヨタ自動車");
        assert!(CompanyNameForWiki::from_company_name("").is_err());
        assert!(CompanyNameForWiki::from_company_name("   ").is_err());

        // Negative (type boundary): there is no From/Into for vault/profile
        // shapes. Documented by compiling only company-name construction —
        // KnowledgeChunkRow / deep_profile types are not accepted by this API.
        fn accepts_query(q: &CompanyNameForWiki) -> &str {
            q.as_str()
        }
        assert_eq!(accepts_query(&q), "トヨタ自動車");
    }
}
