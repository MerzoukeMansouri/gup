pub fn build_commit_message(
    commit_type: Option<&str>,
    scope: &str,
    breaking: bool,
    desc: &str,
    body: &str,
) -> String {
    let header = match commit_type {
        None | Some("") => desc.to_string(),
        Some(t) => {
            let scope_part = if scope.is_empty() {
                String::new()
            } else {
                format!("({})", scope)
            };
            let bang = if breaking { "!" } else { "" };
            format!("{}{}{}: {}", t, scope_part, bang, desc)
        }
    };

    if body.is_empty() {
        return header;
    }

    if breaking {
        format!("{}\n\nBREAKING CHANGE: {}", header, body)
    } else {
        format!("{}\n\n{}", header, body)
    }
}

pub fn parse_commit_header(header: &str) -> (Option<String>, Option<String>, bool, String) {
    if let Some(colon_pos) = header.find(": ") {
        let prefix = &header[..colon_pos];
        let desc = header[colon_pos + 2..].to_string();
        let breaking = prefix.ends_with('!');
        let prefix = if breaking {
            &prefix[..prefix.len() - 1]
        } else {
            prefix
        };

        if let (Some(ps), Some(pe)) = (prefix.find('('), prefix.find(')')) {
            if ps < pe {
                let t = prefix[..ps].to_string();
                let s = prefix[ps + 1..pe].to_string();
                if !t.is_empty() {
                    return (Some(t), Some(s), breaking, desc);
                }
            }
        }

        if !prefix.is_empty() {
            return (Some(prefix.to_string()), None, breaking, desc);
        }
    }

    (None, None, false, header.to_string())
}

pub fn parse_full_commit(raw: &str) -> (Option<String>, String, bool, String, String) {
    let paragraphs: Vec<&str> = raw
        .split("\n\n")
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .collect();

    if paragraphs.is_empty() {
        return (None, String::new(), false, String::new(), String::new());
    }

    let header_line = paragraphs[0].lines().next().unwrap_or("").trim();
    let (commit_type, scope_opt, mut breaking, desc) = parse_commit_header(header_line);
    let scope = scope_opt.unwrap_or_default();

    let footer_body: Option<String> = paragraphs
        .last()
        .filter(|p| p.starts_with("BREAKING CHANGE:"))
        .map(|p| p["BREAKING CHANGE:".len()..].trim().to_string());

    if footer_body.is_some() {
        breaking = true;
    }

    let body_paragraphs = if paragraphs.len() > 1 {
        let rest = &paragraphs[1..];
        if footer_body.is_some() && !rest.is_empty() {
            &rest[..rest.len() - 1]
        } else {
            rest
        }
    } else {
        &[]
    };

    // When a BREAKING CHANGE footer exists, its content becomes the body (used to
    // regenerate the footer via build_commit_message). Regular body paragraphs take
    // precedence if both are present.
    let body = if body_paragraphs.is_empty() {
        footer_body.unwrap_or_default()
    } else {
        body_paragraphs.join("\n\n")
    };

    (commit_type, scope, breaking, desc, body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_simple_type_desc() {
        assert_eq!(
            build_commit_message(Some("feat"), "", false, "add login", ""),
            "feat: add login"
        );
    }

    #[test]
    fn build_with_scope() {
        assert_eq!(
            build_commit_message(Some("feat"), "auth", false, "add login", ""),
            "feat(auth): add login"
        );
    }

    #[test]
    fn build_breaking_no_body() {
        assert_eq!(
            build_commit_message(Some("feat"), "auth", true, "add login", ""),
            "feat(auth)!: add login"
        );
    }

    #[test]
    fn build_breaking_with_body() {
        assert_eq!(
            build_commit_message(Some("feat"), "", true, "rework API", "Details here"),
            "feat!: rework API\n\nBREAKING CHANGE: Details here"
        );
    }

    #[test]
    fn build_body_no_breaking() {
        assert_eq!(
            build_commit_message(Some("fix"), "", false, "handle null", "Extra context"),
            "fix: handle null\n\nExtra context"
        );
    }

    #[test]
    fn build_raw_no_type() {
        assert_eq!(
            build_commit_message(None, "", false, "raw commit message", ""),
            "raw commit message"
        );
    }

    #[test]
    fn parse_header_simple() {
        let (t, s, b, d) = parse_commit_header("feat: add login");
        assert_eq!(t.as_deref(), Some("feat"));
        assert_eq!(s, None);
        assert!(!b);
        assert_eq!(d, "add login");
    }

    #[test]
    fn parse_header_with_scope() {
        let (t, s, b, d) = parse_commit_header("feat(auth): add login");
        assert_eq!(t.as_deref(), Some("feat"));
        assert_eq!(s.as_deref(), Some("auth"));
        assert!(!b);
        assert_eq!(d, "add login");
    }

    #[test]
    fn parse_header_breaking() {
        let (t, s, b, d) = parse_commit_header("feat(auth)!: rework API");
        assert_eq!(t.as_deref(), Some("feat"));
        assert_eq!(s.as_deref(), Some("auth"));
        assert!(b);
        assert_eq!(d, "rework API");
    }

    #[test]
    fn parse_header_raw() {
        let (t, s, b, d) = parse_commit_header("raw commit message");
        assert_eq!(t, None);
        assert_eq!(s, None);
        assert!(!b);
        assert_eq!(d, "raw commit message");
    }

    #[test]
    fn parse_full_roundtrip() {
        let original = "feat(auth)!: rework API\n\nBREAKING CHANGE: the auth API changed";
        let (t, s, b, d, body) = parse_full_commit(original);
        let rebuilt = build_commit_message(t.as_deref(), &s, b, &d, &body);
        assert_eq!(rebuilt, original);
    }

    #[test]
    fn parse_full_with_body() {
        let raw = "fix: handle null\n\nSome description here";
        let (t, _s, b, d, body) = parse_full_commit(raw);
        assert_eq!(t.as_deref(), Some("fix"));
        assert!(!b);
        assert_eq!(d, "handle null");
        assert_eq!(body, "Some description here");
    }

    #[test]
    fn parse_full_simple_header_only() {
        let (t, s, b, d, body) = parse_full_commit("chore: update deps");
        assert_eq!(t.as_deref(), Some("chore"));
        assert!(s.is_empty());
        assert!(!b);
        assert_eq!(d, "update deps");
        assert!(body.is_empty());
    }
}
