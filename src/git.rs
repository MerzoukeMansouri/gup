use anyhow::{bail, Context, Result};
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

pub fn staged_stat() -> Result<Vec<FileStat>> {
    let out = Command::new("git")
        .args(["diff", "--staged", "--numstat"])
        .output()
        .context("failed to run git diff --staged --numstat")?;
    Ok(parse_numstat(&String::from_utf8_lossy(&out.stdout)))
}

pub(crate) fn parse_numstat(text: &str) -> Vec<FileStat> {
    text.lines()
        .filter_map(|line| {
            let mut parts = line.splitn(3, '\t');
            let added = parts.next()?.parse::<i32>().ok()?;
            let deleted = parts.next()?.parse::<i32>().ok()?;
            let file = parts.next()?.to_string();
            Some(FileStat {
                file,
                added,
                deleted,
            })
        })
        .collect()
}

pub fn log_graph() -> Result<String> {
    let out = Command::new("git")
        .args(["log", "--oneline", "--graph", "--decorate", "-12"])
        .output()
        .context("failed to run git log")?;
    Ok(String::from_utf8_lossy(&out.stdout).trim_end().to_string())
}

pub struct FileStat {
    pub file: String,
    pub added: i32,
    pub deleted: i32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_numstat_single_file() {
        let stats = parse_numstat("5\t3\tsrc/main.rs\n");
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].file, "src/main.rs");
        assert_eq!(stats[0].added, 5);
        assert_eq!(stats[0].deleted, 3);
    }

    #[test]
    fn parse_numstat_multiple_files() {
        let input = "10\t2\tsrc/ui.rs\n3\t0\tsrc/git.rs\n";
        let stats = parse_numstat(input);
        assert_eq!(stats.len(), 2);
        assert_eq!(stats[0].file, "src/ui.rs");
        assert_eq!(stats[0].added, 10);
        assert_eq!(stats[1].file, "src/git.rs");
        assert_eq!(stats[1].deleted, 0);
    }

    #[test]
    fn parse_numstat_empty() {
        assert!(parse_numstat("").is_empty());
    }

    #[test]
    fn parse_numstat_skips_malformed_lines() {
        let stats = parse_numstat("bad line\n5\t2\tsrc/lib.rs\n");
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].file, "src/lib.rs");
    }
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
