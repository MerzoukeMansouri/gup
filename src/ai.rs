use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

const OLLAMA_URL: &str = "http://localhost:11434/api/generate";
const MODEL: &str = "devstral-small-2:24b-cloud";

#[derive(Serialize)]
struct Request<'a> {
    model: &'a str,
    prompt: String,
    stream: bool,
}

#[derive(Deserialize)]
struct Response {
    response: String,
}

pub fn generate_with_hint(
    diff: &str,
    commit_type: Option<&str>,
    hint: Option<&str>,
) -> Result<String> {
    let type_rule = match commit_type {
        Some(t) => format!(
            "The commit type is fixed: '{t}'. Output ONLY the description after '{t}: ' — do NOT include the type prefix in your response."
        ),
        None => "Choose the most appropriate conventional commit type (feat, fix, docs, chore, refactor, test, style, perf, ci, build, revert). Output the full message as <type>: <description>.".to_string(),
    };

    let hint_section = match hint {
        Some(h) if !h.is_empty() => format!("\nUser feedback on previous attempt: {h}\n"),
        _ => String::new(),
    };

    let prompt = format!(
        "Generate a conventional commit message for the following staged diff.\n\
        {type_rule}\n\
        Rules:\n\
        - Max 72 characters total\n\
        - Imperative mood (\"add\", not \"added\")\n\
        - Be specific, not generic\n\
        - Output ONLY the commit message line, nothing else\n\
        {hint_section}\n\
        Diff:\n{diff}"
    );

    let client = reqwest::blocking::Client::new();
    let resp = client
        .post(OLLAMA_URL)
        .json(&Request {
            model: MODEL,
            prompt,
            stream: false,
        })
        .send()
        .context("failed to reach Ollama — is it running on localhost:11434?")?;

    let body: Response = resp.json().context("failed to parse Ollama response")?;
    Ok(strip_fences(body.response.trim()))
}

pub(crate) fn strip_fences(s: &str) -> String {
    if !s.starts_with("```") {
        return s.to_string();
    }
    let lines: Vec<&str> = s.lines().collect();
    let end = if lines.last().map(|l| l.trim()) == Some("```") {
        lines.len() - 1
    } else {
        lines.len()
    };
    lines[1..end].join("\n").trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_fences_plain_message() {
        assert_eq!(strip_fences("feat: add login"), "feat: add login");
    }

    #[test]
    fn strip_fences_with_closing_fence() {
        let input = "```\nfeat: add login\n```";
        assert_eq!(strip_fences(input), "feat: add login");
    }

    #[test]
    fn strip_fences_with_language_tag() {
        let input = "```text\nfix: handle null pointer\n```";
        assert_eq!(strip_fences(input), "fix: handle null pointer");
    }

    #[test]
    fn strip_fences_multiline_keeps_first_line() {
        let input = "```\nfeat: add feature\nextra line\n```";
        assert_eq!(strip_fences(input), "feat: add feature\nextra line");
    }

    #[test]
    fn strip_fences_unclosed_fence() {
        let input = "```\nfeat: add feature";
        assert_eq!(strip_fences(input), "feat: add feature");
    }
}
