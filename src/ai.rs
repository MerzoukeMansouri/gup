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

pub fn generate(diff: &str, commit_type: Option<&str>) -> Result<String> {
    let type_rule = match commit_type {
        Some(t) => format!(
            "The commit type is fixed: '{t}'. Output ONLY the description after '{t}: ' — do NOT include the type prefix in your response."
        ),
        None => "Choose the most appropriate conventional commit type (feat, fix, docs, chore, refactor, test, style, perf, ci, build, revert). Output the full message as <type>: <description>.".to_string(),
    };

    let prompt = format!(
        "Generate a conventional commit message for the following staged diff.\n\
        {type_rule}\n\
        Rules:\n\
        - Max 72 characters total\n\
        - Imperative mood (\"add\", not \"added\")\n\
        - Be specific, not generic\n\
        - Output ONLY the commit message line, nothing else\n\n\
        Diff:\n{diff}"
    );

    let client = reqwest::blocking::Client::new();
    let resp = client
        .post(OLLAMA_URL)
        .json(&Request { model: MODEL, prompt, stream: false })
        .send()
        .context("failed to reach Ollama — is it running on localhost:11434?")?;

    let body: Response = resp.json().context("failed to parse Ollama response")?;
    Ok(body.response.trim().to_string())
}
