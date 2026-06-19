use anyhow::{Context, Result, bail};
use std::process::Command;

pub fn add_all() -> Result<()> {
    run("git", &["add", "-A"])
}

pub fn staged_diff() -> Result<String> {
    let out = Command::new("git")
        .args(["diff", "--staged"])
        .output()
        .context("failed to run git diff --staged")?;
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

pub fn has_staged_changes() -> Result<bool> {
    let status = Command::new("git")
        .args(["diff", "--staged", "--quiet"])
        .status()
        .context("failed to check staged changes")?;
    Ok(!status.success())
}

pub fn commit(message: &str) -> Result<()> {
    run("git", &["commit", "-m", message])
}

pub fn push() -> Result<()> {
    run("git", &["push"])
}

fn run(cmd: &str, args: &[&str]) -> Result<()> {
    let status = Command::new(cmd)
        .args(args)
        .status()
        .with_context(|| format!("failed to spawn: {cmd}"))?;

    if !status.success() {
        bail!("{cmd} exited with {status}");
    }
    Ok(())
}
