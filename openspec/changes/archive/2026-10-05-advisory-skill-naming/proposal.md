# Proposal

## Why

Skillator rejects working skills when frontmatter names differ from source directories or use human-readable naming. These conformance checks currently block enablement and Library delivery even though the skill document is usable.

## What Changes

- Treat name/directory divergence and naming-style deviations as advisory warnings rather than invalidating an otherwise usable skill.
- Show naming warnings in skill details in Local Library, Targets and inspected follower replicas, separate from blocking errors; do not show naming-only error markers or error rows.
- Keep parsing, required metadata, unsafe path components, collisions, containment and stale-source checks blocking.
- Make enablement, acquisition, materialization and Library synchronization use the same usable-skill distinction without rewriting source documents.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `library-management`: Define advisory skill naming checks separately from blocking validation so usable skills remain eligible for enablement, acquisition and delivery.
- `tui-workflows`: Expose naming warnings in skill details without inventory error state, including follower inspection.

## Impact

Library metadata validation, CLI inventory diagnostics, acquisition and reconciliation source checks, TUI detail rendering and replica inspection. No persisted configuration change, new dependency, automatic rename or source-document rewrite.
