# Changelog

Notable user-facing changes are recorded here. Version headings link to the corresponding source comparison.

## [Unreleased]

### Added

- Deliver only discovered skill directories—not whole registered repositories—to owned follower replicas: push from the authoritative leader or pull fresh content, respecting file and subtree exclusions with prerequisite and inode-boundary checks before writes, convergent read-only previews, and host-local selections (#38).
- Filter agent-directory presets or enter custom paths with `Ctrl+T`; verify and stage Library followers over SSH, then browse their delivered replicas read-only.
- Update Library repositories with fast-forward-only pulls using `skillator library update`, with a preview before making changes (#31).
- Automatically synchronize newly populated linked worktrees with an opt-in Git hook installed by `skillator hook install` (#20).

### Changed

- Link inherited user skills into a repository with `m`; remove the override with Space without disabling User Scope (#30).
- Navigate Library, User, and Repo as top-level scopes with host/directory sub-tabs, scope-colored borders, a right-aligned title, and scope/path status on the bottom line. `Tab` now stays within a scope; `Ctrl+L` returns to the last User/Repo scope instead of the former mixed Skills view.
- Host entries accept optional verified `hostname` metadata without changing SSH destinations. Older builds reject the field; remove `hostname` before downgrading.
- Release archives include a versionless `skillator` executable (#21).

### Fixed

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
