## Why

The Library TUI can register and inspect followers but cannot deliver skills to them; users must switch to the CLI after registration. Give users a deliberate, safe way to synchronize one selected follower from the leader without changing the existing command-line workflow.

## What Changes

- Make `s` and `Ctrl+S` on a selected remote Library host initiate a confirmed, leader-authoritative push to that saved follower, rather than saving local edits or exiting.
- After successfully saving newly registered followers, ask whether to initialize each one in registration order; declining retains the saved registration without creating a replica. Defer host-only `Ctrl+S` save-and-exit until these prompts have been declined or completed.
- Confirm the overwrite and stale-file deletion boundary inside the owned follower replica before any transfer. Run delivery off the UI thread, prevent duplicate transfer starts, cancel on host/scope exit, ignore stale replies, and refresh inspection after success.
- Keep unsaved local and registry edits separate from remote delivery; validate fresh saved alias, destination, and source state. Preserve existing read-only follower inspection and source acquisition semantics.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `tui-workflows`: Remote Library key actions, registration follow-up, transfer confirmation, cancellation, and saved-state safety.

## Impact

The TUI and its reuse of the existing leader-push/SSH/rsync pipeline, follower inspection behavior, TUI tests, Library delivery guide, README, contributor smoke guidance, and changelog. No new dependencies, CLI behavior or signatures, persisted configuration formats, atomic-transfer guarantee, or network-operation bound.
