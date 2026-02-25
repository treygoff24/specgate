use chrono::NaiveDate;

use super::IgnoreComment;

/// Parse a `@specgate-ignore` comment.
///
/// Supported formats:
/// - `@specgate-ignore: reason`
/// - `@specgate-ignore until:2026-04-01: reason`
pub fn parse_ignore_comment(comment_text: &str) -> Option<IgnoreComment> {
    let mut text = comment_text.trim();

    if text.starts_with("//") {
        text = text.trim_start_matches('/').trim();
    } else if text.starts_with("/*") {
        text = text.trim_start_matches("/*").trim_end_matches("*/").trim();
    }

    if !text.starts_with("@specgate-ignore") {
        return None;
    }

    let rest = text
        .strip_prefix("@specgate-ignore")
        .map(str::trim)
        .unwrap_or_default();

    let (expiry, reason) = if let Some(until_payload) = rest.strip_prefix("until:") {
        let colon_idx = until_payload.find(':').unwrap_or(until_payload.len());
        let date_str = until_payload[..colon_idx].trim();
        let expiry = NaiveDate::parse_from_str(date_str, "%Y-%m-%d").ok();

        let reason = if colon_idx < until_payload.len() {
            until_payload[colon_idx + 1..].trim()
        } else {
            ""
        };

        (expiry, reason)
    } else {
        let reason = rest.strip_prefix(':').unwrap_or(rest).trim();
        (None, reason)
    };

    Some(IgnoreComment {
        reason: reason.to_string(),
        expiry,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_simple_ignore_comment() {
        let parsed = parse_ignore_comment("// @specgate-ignore: legacy import").expect("parsed");
        assert_eq!(parsed.reason, "legacy import");
        assert!(parsed.expiry.is_none());
    }

    #[test]
    fn parses_expiring_ignore_comment() {
        let parsed = parse_ignore_comment("/* @specgate-ignore until:2026-04-01: temporary */")
            .expect("parsed");
        assert_eq!(parsed.reason, "temporary");
        assert_eq!(parsed.expiry.unwrap().to_string(), "2026-04-01");
    }

    #[test]
    fn returns_none_for_non_ignore_comment() {
        assert!(parse_ignore_comment("// some other comment").is_none());
    }
}
