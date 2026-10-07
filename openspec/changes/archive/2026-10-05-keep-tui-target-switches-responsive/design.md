## Context

See proposal.md for motivation. User and Repo share a loaded Target state and tab model. Library currently retains its full view, but Target retains only browsing preferences. Save preparation already reloads authoritative inputs.

## Goals / Non-Goals

**Goals:** Move Target loading, observation and row construction away from scope-entry rendering. Reuse owned models instead of rescanning or cloning inventories on every switch. Preserve state in both scopes while rejecting obsolete work.

**Non-Goals:** Change save semantics, initial root startup scope selection, mutation planning, configuration formats or SSH behavior. Local scans remain read-only and single-flight, not cancellable subprocess workflows.

## Decisions

- Retain an owned Target model and shared runtime (loaded sessions, Library snapshot/configuration, errors, dirty scopes and refresh controller) in session navigation. This replaces the browsing-only cache; a separate User/Repo cache would duplicate state and complicate inheritance.
- Build a complete replacement state on one worker. Keep the latest queued request and generation; entering again requests a fresh scan even if an old reply completed while inactive. Different targets, undo and successful writes invalidate pending replies. Existing root startup may initialize synchronously to preserve startup scope selection; first Target entry after explicit Library uses a loading model and the same background path.
- Hold completed results while either scope is dirty or any overlay is open. Merge browsing identities against new rows, then activate the current scope/directory; keep directory keys stable even when tab positions change. Empty/loading views reject mutation through the existing scope-error guard while scope controls, filtering, help and target selection remain available.
- Keep save preparation/commit authoritative and unchanged. Do not pass browsing caches to validation. Writes/discards invalidate refreshes before replacing state; undo clears staged state and reloads asynchronously.

## Risks / Trade-offs

- [Stale background reply] → Generation rejection plus invalidation on writes, undo and target changes; coalesce requests without concurrent Target scans.
- [Lost hidden-scope edits] → Gate application on all dirty scopes, not only the active model.
- [Row/tab insertion shifts selection] → Remap cached directory/row identities, retaining filters/collapse state and clamping only when identity disappeared.
- [Worker failure] → Display an explicit diagnostic and retain browsing data, never fabricate an empty successful inventory.
- [Cached display is briefly stale] → Render immediately, refresh asynchronously, keep live save checks; loading state prevents mutation before sessions exist.

## Migration Plan

Replace the browsing-only Target cache in place, update native smoke guidance and TUI requirements, then run full preflight. No persisted migration or compatibility shim is needed.
