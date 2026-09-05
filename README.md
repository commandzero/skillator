# Skillator

Skillator is a terminal UI for deciding which agent skills are active in a project.

Keep skills in library folders outside your repositories. Skillator finds folders containing `SKILL.md`, then links or copies the skills you choose into `.agents/skills`. Each checkout saves its choices in `.agents/skillator.yaml`. Git ignores this configuration, the installed skills, and Skillator's generated `.gitignore` file.

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
3. Add any other library folders that contain skills or skill repositories. A skill is a directory with a valid `SKILL.md` file.
4. Press `Ctrl+L` to switch to the Skills view for the current repository.
5. Enable the skills you want. Use `m` to choose `link` or `copy`.
6. Press `s` to review and save, or `Ctrl+S` to save and exit when every pending change is safe.

The Library lists available skills. The Skills view lets you choose which ones to enable for a repository or your user account.

By default, Skillator manages `.agents/skills`. Add another skill tab when a project also needs a directory such as `.claude/skills`.

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

# Clone with Git, then register the local repository with Skillator.
git clone https://github.com/elastic/agent-skills.git ~/Development/elastic-agent-skills
skillator library add ~/Development/elastic-agent-skills

# List discovered Skills from matching Sources.
skillator library list elastic

# List configured Library Locations.
skillator library locations

# Unregister a Location without deleting it.
skillator library remove ~/Development/agent-skills

# Preview and remove registrations for Library directories that no longer exist.
skillator library prune --check
skillator library prune

# Initialize clone-local Target state with no enabled Skills.
skillator init

# Preview and apply one Target Enablement.
skillator target link elastic/agent-skills:skills/esdiag --check
skillator target link elastic/agent-skills:skills/esdiag

# Inspect saved Target Enablements and their observed state.
skillator target list

# Copy or remove a User Scope Skill.
skillator user copy local/library:release-checklist
skillator user remove local/library:release-checklist

# Inspect User Scope Enablements.
skillator user list

# Inspect registered Targets, forget one, or clean stale registrations.
skillator targets list
skillator targets remove /path/to/old-worktree
skillator targets prune --check
skillator targets prune

# Preview filesystem work without writing anything.
skillator sync --check

# Update skill links and copies without opening the interface.
skillator sync

# Allow replacing or removing existing files. Review with --check first.
skillator sync --force

# Use a machine-readable report.
skillator sync --check --format json

# Apply the primary worktree's skill settings to this linked worktree.
skillator sync worktree

# Preview worktree changes without writing.
skillator sync worktree --check

# Git creates linked worktrees; Skillator synchronizes and registers them.
git worktree add ../feature-worktree -b feature
skillator sync worktree ../feature-worktree
```

Bare `sync` inspects the current Git context. It runs worktree synchronization from a linked worktree and Target synchronization everywhere else. Use `sync target [directory]` or `sync worktree [directory]` to select the workflow explicitly. Both default to `.`.

Sync reads existing configuration. It does not create a Library, create repository configuration, or add skills to either one. Use `init`, the explicit `library`, `target`, and `user` commands, or the TUI for those jobs. Worktree sync reads the primary worktree's local configuration and reconciles the current linked worktree using this machine's Library. It never changes the primary worktree or synchronizes Library settings.

## Linking, copying, and unmanaged skills

`link` is the default. The target entry is a symbolic link to the Library skill, so edits to the Library take effect right away.

`copy` gives the repository its own copy of a skill. Skillator reports when that copy differs from the library version.

Skills that Skillator does not manage can still be tracked in Git. The generated `.agents/.gitignore` ignores the local configuration, itself, and all entries under `.agents/skills`. Exceptions at the end of that file allow repository skills to be tracked.

Skillator keeps existing ignore rules when adding its generated section. Replacing a folder or link at the `.gitignore` path requires confirmation. Items marked "Cannot change" are skipped even if you confirm the save or use `--force`.

## Configuration

Library configuration is local to your machine at `~/.skillator/library.yaml`. It records Library locations, not a fixed inventory. Skillator discovers added, removed, and renamed skills whenever it scans those locations.

Configured Target worktrees are registered at `~/.skillator/targets.yaml`. Successful Target initialization, CLI mutations, TUI saves, and worktree synchronization add the canonical worktree path. Skillator uses the registry to report which known Enablements will become unresolved when a Library Location is removed. Missing worktrees remain visible until `targets remove` or `targets prune` forgets them.

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

The local configuration records which skills you enabled. Saving updates the parent `.gitignore` without staging files. If an older checkout still tracks `.agents/skillator.yaml`, stop tracking it while keeping the local file:

```sh
git rm --cached -- .agents/skillator.yaml
```

Skillator saves skills enabled for your user account in `~/.agents/skillator.yaml`. Manage them in the `User` tab. They are also available outside a repository and appear as `[u] user` in repository tabs.

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
| `Tab` / `Shift+Tab` | Switch between skill folders or the User tab |
| `Ctrl+L` | Switch between Skills and Library views |
| `/` | Filter rows. `/pending` shows rows with pending actions. |
| `s` | Save after confirmation |
| `Ctrl+S` | Save and exit when safe |
| `u` | Discard unsaved changes |
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
