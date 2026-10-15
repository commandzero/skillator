## Why

Scope navigation currently discards loaded Library state and performs a synchronous inventory scan before drawing Library. Native measurement found 559–578 ms per entry, with 24 Git subprocesses. Navigation should not wait for discovery.

## What Changes

- Reuse the session's Library snapshot and cached view when switching scopes.
- Refresh discovery in the background while the existing view remains interactive; explicit Library launch without cached inventory shows its locations immediately.
- Preserve browsing and staged edits; ignore obsolete refresh replies after configuration edits, undo or save. Retain follower cancellation.
- Keep fresh save-time inventory and configuration validation.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `tui-workflows`: responsive scope entry, asynchronous refresh and safe cached state.

## Impact

Private TUI session/cache and event-loop state, behavioral regression coverage, README/contributor checks, changelog and main TUI specs. No discovery format, CLI contract, host protocol or save-validation changes.
