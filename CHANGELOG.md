# Changelog

Notable user-facing changes are recorded here. Version headings link to the corresponding source comparison.

## [Unreleased]

### Added

- Synchronize library content across configured SSH hosts with batched transfers, exact-commit Git bootstrap, explicit conflict and missing-file policies, read-only previews, and recoverable publication; user selections and materializations stay local (#32).
- Update Library repositories with fast-forward-only pulls using `skillator library update`, with a preview before making changes (#31).
- Automatically synchronize newly populated linked worktrees with an opt-in Git hook installed by `skillator hook install` (#20).

### Changed

- Link inherited user skills into a repository with `m`; remove the override with Space without disabling User Scope (#30).
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
