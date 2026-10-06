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

1. Run `skillator` inside a Git repository to start on Repo, or from your home directory to start on User—even if home is a Git worktree.
2. Open Library with `Ctrl+L` from Repo or `Ctrl+H` from User, then add folders containing your skills. First-run Library setup appears only when you open Library, not automatically at root startup.
3. Use `Ctrl+H` / `Ctrl+L` to cycle Library, User, and Repo left/right, then `Space` to select skills.
4. Press `s` to review and save, or `Ctrl+S` to save and exit when no confirmation is needed. On a remote Library host, both keys instead request a confirmed sync of that follower; Local keeps its save controls.

Skills are linked by default, so library edits take effect immediately. Press `m` to switch to a separate copy. Skillator reports when a copy differs from the library.

User installs skills across projects; Repo installs them for the current Git checkout. Library manages available skills. The first line selects a scope; `Tab` / `Shift+Tab` selects its directories or Library hosts. The bottom line identifies the active scope and path.

Scope switches reuse the session's loaded Library/User/Repo views and refresh inventory and destination state in the background. First entry to User/Repo from explicit Library shows a loading view while observation runs. Completed refreshes wait while either destination scope has staged edits or an overlay is open, preserving checks, modes, directory choice, filters and collapsed groups. Save still validates live configuration and filesystem state.

A cold Library launch shows its folders while discovery runs. Press `r` on Local to request a Library refresh.

Press Enter on a skill to read its details and complete `SKILL.md`. A frontmatter name that differs from its source directory, or uses a human-readable naming style, produces an advisory warning there—not `[!]`, disabled usage, or a sync refusal. Skillator does not rewrite the document to normalize its name.

Unreadable or malformed documents, missing required metadata, unsafe names, and actual source or destination conflicts remain blocking errors.

Press `Ctrl+T` in User or Repo to filter Generic/Codex (`.agents/skills`) and Claude (`.claude/skills`) presets. Use arrow keys and Enter to stage a directory, or enter a custom relative path when no preset matches. User paths are relative to your home; Repo paths are relative to the checkout. Directories and enablements are written only when you save.

In Library, `Ctrl+T` stages a follower after verifying its hostname through SSH. Configure credentials and trusted keys in `~/.ssh/config` first. The SSH alias remains the destination even when its reported hostname differs. Save explicitly to persist the host; after a successful new-host save, choose whether to initialize it. Declining keeps the registration without creating a replica. In a saved follower tab, `s` or `Ctrl+S` confirms a leader-to-that-follower sync without saving Local edits or exiting; successful delivery refreshes read-only inspection. Confirmation warns that content may be overwritten and stale files deleted only inside the owned `~/.skillator/library/replica`. An interrupted rsync can leave a partial owned replica; retry after fixing the cause. See [Library delivery and follower tabs](docs/library-rsync.md).

## Command line

```sh
# Register a local skill collection and inspect its locations.
skillator library add /path/to/agent-skills
skillator library locations
skillator library list --format json
skillator library list elastic --format json

# Preview and pull fast-forward updates from configured upstreams.
skillator library update --check
skillator library update
skillator library update --timeout 60 --format json

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

# Push current leader skills to followers; on a follower, pull from its leader.
skillator library rsync --check
skillator library rsync --hosts build,development

# On a follower instead, pull from its configured leader.
skillator library rsync

# Optional: sync newly created linked worktrees automatically.
skillator hook install
skillator hook status
git worktree add -b feature ../feature
```

See [library delivery](docs/library-rsync.md) for leader/follower configuration, fresh pulls, and the owned replica deletion boundary.

Clone remote skill repositories with Git, then add their local folders to the Library. Use `skillator user` to manage skills for your account.

In a linked Git worktree, `skillator sync` applies the primary worktree's skill choices. Elsewhere, it applies the current checkout's saved choices. Use `sync target [directory]` or `sync worktree [directory]` to choose explicitly. Sync requires existing configuration; set it up through the interface or `skillator init` first.

`skillator hook install` enables an optional repository-local Git `post-checkout` hook. It runs `skillator sync worktree .` after a populated linked worktree is created. Use `skillator hook install --check` to preview installation, `--force` only when an existing hook should be preserved and chained, and `skillator hook uninstall` to remove an unchanged Skillator hook. The hook does not run for `git clone`, ordinary checkouts, or `git worktree add --no-checkout`. Set `SKILLATOR_NO_AUTO_SYNC=1` for one Git operation. Hook failures leave the worktree available; retry with `skillator sync worktree <directory>`.

Hooks are optional and local to each repository. Agents and CI should keep the explicit sequence `git worktree add ...` followed by `skillator sync worktree <directory>` because it works whether or not a hook is installed.

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

If an agent does not discover a user skill, select its `[u] user` row in a Repository tab and press `m` to stage a repository link, then save. The link points to the Library skill. Press Space on the override and save to remove only the repository link and return to `[u] user`. User Scope stays unchanged.

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
| `Ctrl+H` / `Ctrl+L` | Cycle Library, User, and Repo scopes left / right |
| `Tab` / `Shift+Tab` | Cycle directories or Library hosts within the active scope |
| `Ctrl+T` | Stage an agent directory, or verify and stage a Library follower |
| `/` | Filter skills; `/pending` shows unsaved changes |
| `s` | Review and save in Local/User/Repo; confirm sync of the selected saved follower in a remote Library tab |
| `Ctrl+S` | Save and exit in Local/User/Repo when safe (ask about initializing newly saved followers first); confirm sync without exit in a remote Library tab |
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

## Update Library repositories

Run `skillator library update` to pull clean repositories beneath registered Library Locations.
Each pull uses the current branch's upstream and permits only fast-forward updates.

1. Dirty, detached, unconfigured, or diverged checkouts produce a diagnostic. Independent repositories continue.
2. Submodules are skipped with an advisory. Manage their recorded commits through Git.
3. Each pull has a 30-second limit, including its transport and hooks. Use `--timeout <seconds>` to change the limit.
4. A timeout stops that pull's subprocess group and continues. Ctrl+C stops the batch and emits a partial report with exit 130.
5. Completed updates remain applied after failure or cancellation. A failed pull may have changed Git metadata or working content; Skillator does not roll it back.
6. Existing skill links expose updated content immediately. Copied skills need a separate synchronization.

`--check` inspects only local eligibility and makes no network request or writes.
It reports "Would attempt pull; remote state not checked." and returns 1 for a nonempty plan, even if cached tracking refs match HEAD.

Interactive terminal text lists each unchanged repository as `up-to-date`.
Redirected text uses an unchanged count; JSON and YAML retain per-checkout outcomes.
Normal updates return 0 for success or an empty Library, and 1 for a partial or blocked batch; submodule skips alone do not cause failure.
