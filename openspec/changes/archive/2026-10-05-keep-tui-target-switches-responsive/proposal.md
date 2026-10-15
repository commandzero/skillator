## Why

Library entry now renders cached inventory immediately, but leaving Library still rebuilds User and Repo on the UI thread (278–317 ms measured locally). Scope navigation should not wait for destination filesystem/Git observation.

## What Changes

- Retain the loaded User/Repo state and view while browsing Library.
- Refresh destination configuration, observation and rows on a single background worker; show a loading view when no Target state exists yet.
- Preserve staged edits, overlays, directory selection and browsing identity; invalidate results superseded by undo, save or target changes.
- Keep fresh save-time validation authoritative.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `tui-workflows`: User/Repo entry renders retained or loading state without waiting for discovery/observation.

## Impact

`src/tui.rs`, native terminal verification, behavior regressions, README/contributor smoke guidance and CHANGELOG. No new dependencies, CLI options or saved configuration formats.
