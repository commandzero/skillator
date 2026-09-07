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

Release builds provide macOS arm64, Linux x86_64, and Linux arm64 archives.
The next release targets macOS 14 or Ubuntu 24.04 with glibc 2.39 and newer.
See [release support and installation checks](docs/release.md) for the exact matrix and limits.

## Get started

1. Run `skillator` inside a Git repository.
2. Add the folders containing your skills to the Library. The default is `~/.skillator/library`.
3. Press `Ctrl+L` to open the Skills view, then `Space` to select skills.
4. Press `s` to review and save, or `Ctrl+S` to save and exit when no confirmation is needed.

Skills are linked by default, so library edits take effect immediately. Press `m` to switch to a separate copy. Skillator reports when a copy differs from the library.

Use the `User` tab for skills available across projects. Add another skill tab for folders such as `.claude/skills`.

## Command line

```sh
# Register a local skill collection and inspect its locations.
skillator library add /path/to/agent-skills
skillator library locations
skillator library list --format json
skillator library list elastic --format json

# Set up this checkout, then inspect or change its selected skills.
skillator init
skillator target list
skillator target link SOURCE:PATH --check
skillator target link SOURCE:PATH
skillator target copy SOURCE:PATH
skillator target remove SOURCE:PATH --check

# Inspect and change account-wide skills.
skillator user list
skillator user link SOURCE:PATH --check
skillator user copy SOURCE:PATH
skillator user remove SOURCE:PATH

# Inspect or clean the machine-local Target registry.
skillator targets list
skillator targets prune --check

# Preview and update installed skills from saved settings.
skillator sync --check
skillator sync
```

Clone remote skill repositories with Git, then add their local folders to the Library. Use `skillator user` to manage skills for your account.

In a linked Git worktree, `skillator sync` applies the primary worktree's skill choices. Elsewhere, it applies the current checkout's saved choices. Use `sync target [directory]` or `sync worktree [directory]` to choose explicitly. Sync requires existing configuration; set it up through the interface or `skillator init` first.

Every command has `--help`. Use `--check` to preview changes and `--format json` for scripts. Review affected paths before using `--force` to replace or remove existing content. Items marked "Cannot change" are skipped even with `--force`.

Commands take paths and selectors as arguments. They do not read documents from stdin or use `-` as a stdin marker.
JSON and YAML each contain one complete report. Diagnostics go to stderr.

| Exit status | Meaning |
| --- | --- |
| 0 | Completed acceptable result |
| 1 | Completed result with unresolved or unapplied work |
| 2 | Invalid invocation |
| 3 | Invalid or unavailable required input |
| 4 | Target is busy |
| 5 | Fatal failure, including a failed stdout write |

A failed output write, including a broken pipe, returns 5. It can leave a partial report, so scripts must check the exit status before parsing it.
Mutations may already have completed when output fails; inspect state before retrying.

Skillator retains the discovered inventory and complete report in memory.
It reads configuration, skill metadata, and individual skill files in full; it has no configured size or depth limit.
Use narrowly scoped library locations for large collections. There is no bounded-memory or network-filesystem timeout guarantee.

For the TUI, use `q` to quit or `u` to discard pending edits.
Process signals can interrupt work without a final report or graceful rollback.
After an interrupted mutation, inspect the next run's recovery diagnostics and preserve any reported recovery artifacts.

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
bash scripts/tools-setup.sh
bash scripts/preflight.sh
```

Setup requires Node.js 22 or newer, curl, tar, and SHA-256 tools. Install Rust through rustup, plus ShellCheck and ripgrep.
See [contributor checks](docs/contributing.md), [release procedure](docs/release.md), and [change history](CHANGELOG.md).

## License

[MIT](LICENCE.md)
