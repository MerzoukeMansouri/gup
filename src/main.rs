mod ai;
mod git;
mod ui;

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

    let diff = if cli.ai { git::staged_diff()? } else { String::new() };

    let initial_msg = if cli.ai {
        None // TUI shows spinner while generating
    } else {
        Some(match commit_type {
            Some(t) => format!("{t}: {}", raw_message.unwrap()),
            None => raw_message.unwrap().to_string(),
        })
    };

    let commit_msg = ui::run(initial_msg, commit_type, cli.ai, diff)?;

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
        (None, None, true) => Ok((None, None)),

        (Some(t), msg, _) if TYPES.contains(&t) => {
            if !cli.ai && msg.is_none() {
                bail!("'{t}' requires a message or --ai");
            }
            Ok((Some(t), msg))
        }

        (Some(raw), None, false) => Ok((None, Some(raw))),

        (Some(bad), Some(_), _) => {
            bail!("'{bad}' is not a valid commit type. Valid: {}", TYPES.join(", "))
        }

        _ => bail!(
            "usage:\n  gup <type> <message>\n  gup <type> --ai\n  gup --ai\n\nTypes: {}",
            TYPES.join(", ")
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_cli(first: Option<&str>, message: Option<&str>, ai: bool) -> Cli {
        Cli {
            first: first.map(str::to_string),
            message: message.map(str::to_string),
            ai,
            no_push: false,
        }
    }

    #[test]
    fn ai_only_no_type() {
        let cli = make_cli(None, None, true);
        let (t, m) = resolve_args(&cli).unwrap();
        assert_eq!(t, None);
        assert_eq!(m, None);
    }

    #[test]
    fn valid_type_with_message() {
        let cli = make_cli(Some("feat"), Some("add login"), false);
        let (t, m) = resolve_args(&cli).unwrap();
        assert_eq!(t, Some("feat"));
        assert_eq!(m, Some("add login"));
    }

    #[test]
    fn valid_type_with_ai() {
        let cli = make_cli(Some("fix"), None, true);
        let (t, m) = resolve_args(&cli).unwrap();
        assert_eq!(t, Some("fix"));
        assert_eq!(m, None);
    }

    #[test]
    fn valid_type_without_message_or_ai_errors() {
        let cli = make_cli(Some("feat"), None, false);
        let err = resolve_args(&cli).unwrap_err();
        assert!(err.to_string().contains("requires a message or --ai"));
    }

    #[test]
    fn raw_message_no_type() {
        let cli = make_cli(Some("my raw commit message"), None, false);
        let (t, m) = resolve_args(&cli).unwrap();
        assert_eq!(t, None);
        assert_eq!(m, Some("my raw commit message"));
    }

    #[test]
    fn unknown_type_with_message_errors() {
        let cli = make_cli(Some("unknown"), Some("something"), false);
        let err = resolve_args(&cli).unwrap_err();
        assert!(err.to_string().contains("not a valid commit type"));
    }

    #[test]
    fn no_args_no_ai_errors() {
        let cli = make_cli(None, None, false);
        assert!(resolve_args(&cli).is_err());
    }

    #[test]
    fn all_valid_types_accepted() {
        for &t in TYPES {
            let cli = make_cli(Some(t), Some("msg"), false);
            let (typ, msg) = resolve_args(&cli).unwrap();
            assert_eq!(typ, Some(t));
            assert_eq!(msg, Some("msg"));
        }
    }

    #[test]
    fn all_valid_types_with_ai_accepted() {
        for &t in TYPES {
            let cli = make_cli(Some(t), None, true);
            let (typ, msg) = resolve_args(&cli).unwrap();
            assert_eq!(typ, Some(t));
            assert_eq!(msg, None);
        }
    }
}
