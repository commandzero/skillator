# Skillator

Choose which agent skills are active in each project, from your terminal.

Keep your skills in library folders. Skillator finds directories containing `SKILL.md` and links or copies the ones you choose into a project's `.agents/skills`. Each Git checkout keeps its own choices, outside version control.

## Install

Requires Rust 1.97 or newer. Install the published crate with:

```sh
cargo install skillator --locked
```

To install the current checkout instead:

```sh
cargo install --path . --locked
```

To install the prebuilt formula from GitHub:

```sh
brew install commandzero/tools/skillator
```

Supports macOS, Linux, and WSL using its Linux filesystem. Native Windows is not supported.

## Get started

1. Run `skillator` inside a Git repository.
2. Add the folders containing your skills to the Library. The default is `~/.skillator/library`.
3. Press `Ctrl+L` to open the Skills view, then `Space` to select skills.
4. Press `s` to review and save, or `Ctrl+S` to save and exit when no confirmation is needed.

Skills are linked by default, so library edits take effect immediately. Press `m` to switch to a separate copy. Skillator reports when a copy differs from the library.

Use the `User` tab for skills available across projects. Add another skill tab for folders such as `.claude/skills`.

## Command line

```sh
# Add a local skill collection and find skills in it.
skillator library add /path/to/agent-skills
skillator library list

# Set up this checkout, then inspect its selected skills.
skillator init
skillator target list

# Preview and apply a skill, using a selector from the library listing.
skillator target link SOURCE:PATH --check
skillator target link SOURCE:PATH

# Preview and update installed skills from saved settings.
skillator sync --check
skillator sync
```

Clone remote skill repositories with Git, then add their local folders to the Library. Use `skillator user` to manage skills for your account.

In a linked Git worktree, `skillator sync` applies the primary worktree's skill choices. Elsewhere, it applies the current checkout's saved choices. Use `sync target [directory]` or `sync worktree [directory]` to choose explicitly. Sync requires existing configuration; set it up through the interface or `skillator init` first.

Every command has `--help`. Use `--check` to preview changes and `--format json` for scripts. Review affected paths before using `--force` to replace or remove existing content. Items marked "Cannot change" are skipped even with `--force`.

## Local files and Git

Skillator stores the library's folder list in `~/.skillator/library.yaml` and its registered worktrees in `~/.skillator/targets.yaml`. Account-wide skill choices live in `~/.agents/skillator.yaml`.

Each checkout stores its choices in `.agents/skillator.yaml`. Saving or syncing maintains `.agents/.gitignore`, which ignores itself, the local configuration, and installed skills. Existing ignore rules are preserved. Skillator leaves the repository's root `.gitignore` alone.

To track a project-owned skill in Git, select its row and press `m` to mark it as `[r] repo`, then save. Skillator adds an ignore exception for that folder. Skills marked `[u] user` are managed in the User tab.

If an older checkout already tracks the local configuration, keep the file while removing it from Git:

```sh
git rm --cached -- .agents/skillator.yaml
```

## Keys

| Key | Action |
| --- | --- |
| `j` / `k` or arrow keys | Move between rows |
| `h` / `l` | Collapse or expand a group |
| `Space` | Toggle a skill or group |
| `m` | Change how a skill is installed or tracked |
| `Tab` / `Shift+Tab` | Switch skill folders or the User tab |
| `Ctrl+L` | Switch between Skills and Library |
| `/` | Filter skills; `/pending` shows unsaved changes |
| `s` | Review and save |
| `Ctrl+S` | Save and exit when no confirmation is needed |
| `u` | Discard unsaved changes |
| `?` | Show all shortcuts |
| `q` | Quit |

## Development

```sh
cargo run --
cargo fmt --check
cargo test --all-targets
cargo clippy --all-targets -- -D warnings
```

## License

[MIT](LICENSE)
