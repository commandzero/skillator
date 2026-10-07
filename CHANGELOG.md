# Changelog

Notable user-facing changes are recorded here. Version headings link to the corresponding source comparison.

## [Unreleased]

### Added

- Deliver only discovered skill directories—not whole registered repositories—to owned follower replicas: push authoritative leader content or pull fresh snapshots, with write-free previews, early local-input validation, pre-write SSH and filesystem capability checks, source permissions and content exclusions preserved, retryable ownership-marker creation, cleanup or retained-export diagnostics, and host-local selections (#38).
- Filter agent-directory presets or enter custom paths with `Ctrl+T`; verify and stage Library followers over SSH, then browse their delivered replicas read-only (#39).
- Sync a selected follower from its Library tab, with optional initialization after registration and confirmation before overwriting or deleting owned replica content (#39).
- Update Library repositories with fast-forward-only pulls using `skillator library update`, with a preview before making changes (#31).
- Automatically synchronize newly populated linked worktrees with an opt-in Git hook installed by `skillator hook install` (#20).

### Changed

- Link inherited user skills into a repository with `m`; remove the override with Space without disabling User Scope (#30).
- Navigate Library, User, and Repo as top-level scopes with host/directory sub-tabs, scope-colored borders (gray for Library), a right-aligned title, and scope/path status on the bottom line. `Tab` stays within a scope; use `Ctrl+H`/`Ctrl+L` to cycle left/right instead of desktop-conflicting Ctrl-arrows or a Library toggle (#39).
- Host entries accept optional verified `hostname` metadata without changing SSH destinations. Older builds reject the field; remove `hostname` before downgrading (#39).
- Release archives include a versionless `skillator` executable (#21).

### Fixed

- Skills with safe human-readable or divergent names remain usable and syncable; skill details show naming warnings instead of inventory errors (#39).
- Root TUI startup selects Repo inside Git and User in the home directory, including home-as-Git, without requiring Library setup first (#39).
- Library scope switches reuse loaded inventory and refresh in the background instead of waiting for a full scan, without overwriting staged edits (#39).
- Leaving Library for User or Repo renders cached views immediately and refreshes saved configuration and current on-disk inventory in the background, preserving browsing and staged edits (#39).
- Deleting a Skill Directory keeps selection and editing within its User or Repo scope (#39).
- Confirmed Target changes discard pending Library edits; canceling the Target picker preserves them (#39).
- Follower tabs can read safely contained relative `SKILL.md` links without rewriting replica content (#39).
- Follower tabs inspect configured bracketed IPv6 destinations, including SSH usernames, using the same endpoint as delivery (#39).
- Returning from a follower retains the selected Local skill after a background inventory refresh inserts earlier rows (#39).
- Editing an empty User or Repo scope shows its setup notice instead of opening a directory from another scope (#39).
- Read-only follower inspection rejects hard-linked symbolic-link inodes before reading skill inventory (#39).
- Removing a user skill before user configuration exists returns an unchanged result.
- Reject target-registry paths containing redundant separators, `.` or `..` components.

## [0.1.0] - 2026-09-04

### Added

- Terminal interface for discovering skill libraries and selecting linked or copied skills per checkout or user scope.
- CLI workflows for library locations, target and user skill management, target registries, and worktree synchronization.
- Preview and force controls, deterministic JSON/YAML reports, drift detection, and guarded filesystem recovery.
- Rust source installation and an MIT license.

[Unreleased]: https://github.com/commandzero/skillator/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/commandzero/skillator/releases/tag/v0.1.0
