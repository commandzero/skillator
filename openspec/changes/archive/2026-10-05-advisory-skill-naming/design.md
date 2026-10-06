# Design

## Context

See proposal.md for motivation. Library discovery and replica inspection currently implement separate strict name validators. Materialization compares source metadata names with destination basenames, although non-root destinations derive from skill paths rather than metadata names. Acquisition uses metadata names as local directory components.

## Goals / Non-Goals

Goals: one validation policy for local and replicated documents; explicit error/warning separation; preserve source identity, stale-source checks and filesystem safety.

Non-goals: rewrite upstream frontmatter, rename existing source directories, change configuration formats, introduce consumer profiles or relax malformed/missing required metadata.

## Decisions

- Store blocking errors and advisory warnings separately. Validity depends only on errors. Keeping one fatal diagnostics list would continue conflating conformance and usability.
- Reuse a shared metadata inspector for local discovery and replica inspection. Existing metadata helpers retain the same safe-name and required-description checks without treating style warnings as invalidity.
- Accept safe single-component human-readable names; keep empty names, dot components, path separators and control characters blocking. Do not blindly replace strict slug checking with unchecked filesystem names.
- Compare materialization source metadata with the metadata expected when planning, not the unrelated destination basename. Preserve existing copy fingerprints, source identity, destination and collision guards.
- Attach warnings and metadata errors to skill detail state. Render separate advisory and error sections with the original document; do not manufacture main-table warning/error rows or change warning-only checkbox state.
- Preserve CLI inventory report shape and include warnings as labeled advisory diagnostics, with valid state remaining true.

## Risks / Trade-offs

- A warning disappears at a secondary consumer -> migrate discovery, acquisition, target reconciliation and follower inspection together and smoke each real path.
- Relaxed names escape a filesystem destination -> reject unsafe single components before any mutation and retain existing containment/collision checks.
- A stale source passes after removing the basename comparison -> preserve a planned metadata-name comparison independently of destination naming.
- Long detail content obscures the skill document -> include warning/error sections in ordinary detail scrolling.

## Migration Plan

No configuration or skill-content migration. Existing naming-only invalid entries become usable on the next snapshot. Deploy the updated binary; rollback restores strict validation without altering stored data.
