mod ai;
mod git;

use anyhow::{bail, Result};
use clap::Parser;

const TYPES: &[&str] = &[
    "feat", "fix", "docs", "chore", "refactor", "test", "style", "perf", "ci", "build", "revert",
];

#[derive(Parser)]
#[command(
    name = "gup",
    about = "git add + commit + push with conventional commits",
    long_about = None
)]
struct Cli {
    /// Conventional commit type (feat, fix, docs…) or raw message when used alone
    first: Option<String>,

    /// Commit message body — required when a type is given without --ai
    message: Option<String>,

    /// Generate commit message via Ollama AI
    #[arg(long, short)]
    ai: bool,

    /// Stage and commit only, skip push
    #[arg(long)]
    no_push: bool,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let (commit_type, raw_message) = resolve_args(&cli)?;

    git::add_all()?;

    if !git::has_staged_changes()? {
        bail!("nothing to commit — working tree clean");
    }

    let commit_msg = if cli.ai {
        let diff = git::staged_diff()?;
        let body = ai::generate(&diff, commit_type)?;
        match commit_type {
            Some(t) => format!("{t}: {body}"),
            None => body,
        }
    } else {
        let body = raw_message.unwrap();
        match commit_type {
            Some(t) => format!("{t}: {body}"),
            None => body.to_string(),
        }
    };

    eprintln!("commit: {commit_msg}");
    git::commit(&commit_msg)?;

    if !cli.no_push {
        git::push()?;
        eprintln!("pushed");
    }

    Ok(())
}

fn resolve_args(cli: &Cli) -> Result<(Option<&str>, Option<&str>)> {
    let first = cli.first.as_deref();
    let message = cli.message.as_deref();

    match (first, message, cli.ai) {
        // gup --ai
        (None, None, true) => Ok((None, None)),

        // gup feat --ai  |  gup feat "msg"
        (Some(t), msg, _) if TYPES.contains(&t) => {
            if !cli.ai && msg.is_none() {
                bail!("'{t}' requires a message or --ai");
            }
            Ok((Some(t), msg))
        }

        // gup "raw commit message"  (no type prefix)
        (Some(raw), None, false) => Ok((None, Some(raw))),

        // gup unknown-type "msg"  — typo guard
        (Some(bad), Some(_), _) => {
            bail!("'{bad}' is not a valid commit type. Valid: {}", TYPES.join(", "))
        }

        _ => bail!(
            "usage:\n  gup <type> <message>\n  gup <type> --ai\n  gup --ai\n\nTypes: {}",
            TYPES.join(", ")
        ),
    }
}
