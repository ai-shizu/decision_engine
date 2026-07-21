//! Phase 11 — layout-preserving OCR text assembly (Vision observation → lines).
//!
//! Pure / offline. Vision framework supplies `(text, bbox)`; this module sorts
//! by Y then X and emits `品目\t金額` style rows.

/// One OCR token with normalized Vision bounding box (origin bottom-left).
#[derive(Debug, Clone, PartialEq)]
pub struct OcrToken {
    pub text: String,
    /// Normalized [0,1] center Y (Vision bottom-left; larger = higher on page).
    pub y_center: f64,
    /// Normalized [0,1] min X (left edge).
    pub x_min: f64,
}

/// Relative Y tolerance for clustering tokens onto the same line.
pub const LINE_Y_TOLERANCE: f64 = 0.018;

/// Sort tokens top→bottom, left→right; cluster into lines; join with tabs when
/// a rightmost amount-like token is present.
pub fn assemble_layout_text(mut tokens: Vec<OcrToken>) -> String {
    tokens.retain(|t| !t.text.trim().is_empty());
    if tokens.is_empty() {
        return String::new();
    }
    // Top of page first: descending y_center.
    tokens.sort_by(|a, b| {
        b.y_center
            .partial_cmp(&a.y_center)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                a.x_min
                    .partial_cmp(&b.x_min)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    });

    let mut lines: Vec<Vec<OcrToken>> = Vec::new();
    for tok in tokens {
        if let Some(last) = lines.last_mut() {
            let ref_y = last[0].y_center;
            if (tok.y_center - ref_y).abs() <= LINE_Y_TOLERANCE {
                last.push(tok);
                continue;
            }
        }
        lines.push(vec![tok]);
    }

    let mut out = String::new();
    for mut line in lines {
        line.sort_by(|a, b| {
            a.x_min
                .partial_cmp(&b.x_min)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let formatted = format_line(&line);
        if formatted.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(&formatted);
    }
    out
}

fn format_line(tokens: &[OcrToken]) -> String {
    if tokens.is_empty() {
        return String::new();
    }
    if tokens.len() == 1 {
        return tokens[0].text.trim().to_string();
    }
    let last = tokens[tokens.len() - 1].text.trim();
    if looks_like_amount(last) {
        let left: Vec<&str> = tokens[..tokens.len() - 1]
            .iter()
            .map(|t| t.text.trim())
            .filter(|s| !s.is_empty())
            .collect();
        if left.is_empty() {
            return last.to_string();
        }
        return format!("{}\t{}", left.join(" "), last);
    }
    tokens
        .iter()
        .map(|t| t.text.trim())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn looks_like_amount(s: &str) -> bool {
    let trimmed = s.trim().trim_start_matches('¥').trim_start_matches('￥');
    let digits: String = trimmed.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return false;
    }
    // Allow commas / yen marks / trailing 円
    let ok_chars = trimmed
        .chars()
        .all(|c| c.is_ascii_digit() || c == ',' || c == '円' || c == '.');
    ok_chars && digits.len() >= 1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sorts_top_to_bottom_and_tabs_amount() {
        let tokens = vec![
            OcrToken {
                text: "120".into(),
                y_center: 0.50,
                x_min: 0.70,
            },
            OcrToken {
                text: "牛乳".into(),
                y_center: 0.51,
                x_min: 0.10,
            },
            OcrToken {
                text: "店舗A".into(),
                y_center: 0.90,
                x_min: 0.20,
            },
        ];
        let text = assemble_layout_text(tokens);
        assert_eq!(text, "店舗A\n牛乳\t120");
    }

    #[test]
    fn empty_input() {
        assert_eq!(assemble_layout_text(vec![]), "");
    }
}
