# gup

git add + commit + push with conventional commits and AI-generated messages.

![Release](https://img.shields.io/github/v/release/MerzoukeMansouri/gup)
![Rust](https://img.shields.io/badge/rust-stable-orange?logo=rust&logoColor=white)
![License](https://img.shields.io/badge/license-MIT-blue)

## Features

- Stage all changes, commit, and push in one command
- Conventional commit types: `feat`, `fix`, `docs`, `chore`, `refactor`, `test`, `style`, `perf`, `ci`, `build`, `revert`
- AI-generated commit messages via Ollama (`--ai`)
- Interactive TUI to review and edit the message before committing
- `--no-push` to stage and commit only

## Install

### Homebrew

```bash
brew tap MerzoukeMansouri/homebrew
brew install MerzoukeMansouri/homebrew/gup
```

### Update

```bash
brew update
brew upgrade MerzoukeMansouri/homebrew/gup
```

### From source

```bash
cargo build --release
ln -sf $(pwd)/target/release/gup ~/.local/bin/gup
```

Add to `PATH` if needed:

```bash
export PATH="$HOME/.local/bin:$PATH"
```

## Usage

```bash
gup feat "add login page"        # conventional commit
gup fix "null pointer on logout" # fix type
gup "quick typo fix"             # raw message, no type
gup --ai                         # let Ollama generate the message
gup feat --ai                    # AI message with explicit type hint
gup feat "add login" --no-push   # commit only, skip push
```

## Requirements

- Rust 1.70+
- Git
- [Ollama](https://ollama.com/) — for `--ai` flag (optional)

## Contributing

Issues and PRs welcome.
