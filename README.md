# Skillator

Skillator is a terminal UI for deciding which agent skills are active in a project.

Keep skills in one or more Library locations outside your repositories. Skillator discovers the `SKILL.md` directories there, then easily link or copy the skills _you_ choose into the repository's `.agents/skills` directories. Each checkout keeps a small declarative clone-local `skillator.yaml` configuration. The materialized skills and generated local controls stay out of the repository history.

This is useful when a shared skill collection is larger than the set you want an agent to load for one project.

## Install

Skillator compiles with Rust 1.97 or newer.

```sh
cargo install --path .
```

For development, run it from the checkout:

```sh
cargo run --
```

## The basic workflow

1. Start `skillator` from a Git repository.
2. On a first run, Skillator opens the Library and asks you to configure where your skills live. The default is `~/.skillator/library`.
3. Add any other Library locations that contain skills or skill repositories. A skill is a directory with a valid `SKILL.md` file.
4. Press `Ctrl+L` to switch to the current repository's Target view.
5. Enable the skills you want. Use `m` to choose `link` or `copy`.
6. Press `s` to review and save, or `Ctrl+S` to save and exit when every pending change is safe.

The Library is your inventory. A Target is where an agent can discover enabled skills.

By default, Skillator manages `.agents/skills`. Add another Target tab when a project also needs a directory such as `.claude/skills`.

## Commands

```sh
# Open the TUI for the current Git repository.
skillator

# Open the TUI for a specific repository or a directory inside it.
skillator /path/to/repository

# Manage the Library without selecting a repository.
skillator library

# Register a Library Location without activating its Skills.
skillator library add ~/Development/agent-skills

# List discovered Skills from matching Sources.
skillator library list elastic

# List configured Library Locations.
skillator library locations

# Unregister a Location without deleting it.
skillator library remove ~/Development/agent-skills

# Initialize clone-local Target state with no enabled Skills.
skillator init

# Preview and apply one Target Enablement.
skillator target link elastic/agent-skills:skills/esdiag --check
skillator target link elastic/agent-skills:skills/esdiag

# Copy or remove a User Scope Skill.
skillator user copy local/library:release-checklist
skillator user remove local/library:release-checklist

# Preview filesystem work without writing anything.
skillator sync --check

# Reconcile safe work without opening the TUI.
skillator sync

# Permit guarded replacements during non-interactive sync.
skillator sync --force

# Use a machine-readable report.
skillator sync --check --format json

# Project the primary worktree's local Target state into this linked worktree.
skillator sync worktree

# Preview the projection without writing.
skillator sync worktree --check
```

Bare `sync` inspects the current Git context. It runs worktree synchronization from a registered linked worktree and Target synchronization everywhere else. Use `sync target [directory]` or `sync worktree [directory]` to select the workflow explicitly. Both default to `.`.

Sync reads existing configuration. It does not create a Library, create repository configuration, or add skills to either one. Use `init`, the explicit `library`, `target`, and `user` commands, or the TUI for those jobs. Worktree sync reads the primary worktree's local configuration and reconciles the current linked worktree using this machine's Library. It never changes the primary worktree or synchronizes Library settings.

## Linking, copying, and unmanaged skills

`link` is the default. The target entry is a symbolic link to the Library skill, so edits to the Library take effect right away.

`copy` creates an independent snapshot in the Target. Use it when the Target needs its own copy. Skillator compares copied skills with their Library source and reports drift.

Repository-owned skills that Skillator does not manage are allowed and remain ordinary Git-trackable files. Skillator leaves them alone. For the default Target, `.agents/.gitignore` ignores the local configuration, its own generated control file, and every entry under `.agents/skills`. Repository-owned skills become trackable through explicit exceptions kept at the end of that one file.

## Configuration

Library configuration is local to your machine at `~/.skillator/library.yaml`. It records Library locations, not a fixed inventory. Skillator discovers added, removed, and renamed skills whenever it scans those locations.

Configured Target worktrees are registered at `~/.skillator/targets.yaml`. Successful Target initialization, CLI mutations, and TUI saves add the canonical worktree path. Skillator uses the registry to report which known Enablements will become unresolved when a Library Location is removed. Missing worktrees remain registered and are reported as unavailable.

Repository configuration is clone-local at `.agents/skillator.yaml` and is ignored by the generated `.agents/.gitignore`. Skillator does not edit the repository root `.gitignore`. A typical parent control file is:

```gitignore
# Generated by Skillator
.gitignore
skillator.yaml
skills/*

# Exception list for repository tracking
!skills/skillator/
```

Skillator owns the generated section through the exception-list marker and preserves the repository-owned exceptions below it. This keeps the tracking policy documented in one place while allowing project skills such as `.agents/skills/skillator/` to be committed.

A small configuration example:

```yaml
version: 1

skill_directories:
  - key: agents
    path: .agents/skills

enablements:
  - directory: agents
    skill:
      source: local/library
      path: release-checklist
    materialization: linked
  - directory: agents
    skill:
      source: elastic/agent-skills
      path: skills/esdiag
    materialization: copied
```

The local configuration records desired enablements, not a shared inventory. CLI and TUI saves maintain the parent control file without staging files or changing the Git index. If a legacy checkout still tracks `.agents/skillator.yaml`, Skillator preserves it and reports the explicit migration step:

```sh
git rm --cached -- .agents/skillator.yaml
```

Skillator also has a machine-local User Scope at `~/.agents/skillator.yaml`. Its enabled skills appear in the `User` tab and are available outside a repository. Repository tabs show inherited User skills as `[u] user`.

Repository tabs also discover physical Skills that have no Library Enablement. They appear as repository candidates. Press `m` to stage one as `[r] repo`; Save adds its exact parent `.gitignore` exception, such as `!skills/skillator/`. Repo rows are read-only under Space, just like `[u] user` rows. They never create a Library Enablement, link, copy, or participate in Library drift checks.

## TUI keys

The interface uses vim-style navigation. Arrow keys work too.

| Key | Action |
| --- | --- |
| `j` / `k` | Move between rows |
| `J` / `K` | Move between Source groups |
| `h` / `l` | Collapse or expand a Source group |
| `Space` | Toggle a skill or Source group |
| `m` | Change link or copy mode |
| `Tab` / `Shift+Tab` | Change Target or User Scope tab |
| `Ctrl+L` | Switch between Target and Library views |
| `/` | Filter rows. `/pending` shows rows with pending actions. |
| `s` | Save after confirmation |
| `Ctrl+S` | Save and exit when safe |
| `u` | Undo staged edits back to the saved state |
| `?` | Show help |
| `q` | Quit |

## What Skillator does not do

Skillator manages local skills. It does not include a marketplace or skill discovery service. Use a tool such as `npx skills` to find or retrieve skills, then add the local directory or repository to your Library.

It currently targets macOS, Linux, and WSL using its Linux filesystem. Native Windows is not supported.

## Development

```sh
cargo fmt --check
cargo test
cargo clippy -- -D warnings
```
