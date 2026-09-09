# Changelog

Notable user-facing changes are recorded here. Version headings link to the corresponding source comparison.

## [Unreleased]

### Changed

- Release archives gain a versionless executable, MIT license, and build details. The legacy executable name remains as a hard link for the current Homebrew formula.
- Release publication verifies the complete platform matrix and uploaded bytes before publishing a draft. Reruns preserve existing assets and fail on differences.
- Document the next release's macOS 14 and Ubuntu 24.04 binary baselines, compatibility policy, and interrupted-command behavior.

### Fixed

- Treat removal of an account-wide skill before user configuration exists as an unchanged result.
- Reject noncanonical target-registry paths containing redundant separators, `.` or `..` components.
- Correct the domain glossary to describe checkout-local configuration and ignore controls.

## [0.1.0] - 2026-09-04

### Added

- Terminal interface for discovering skill libraries and selecting linked or copied skills per checkout or user scope.
- CLI workflows for library locations, target and user skill management, target registries, and worktree synchronization.
- Preview and force controls, deterministic JSON/YAML reports, drift detection, and guarded filesystem recovery.
- Rust source installation and an MIT license.

[Unreleased]: https://github.com/commandzero/skillator/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/commandzero/skillator/releases/tag/v0.1.0
