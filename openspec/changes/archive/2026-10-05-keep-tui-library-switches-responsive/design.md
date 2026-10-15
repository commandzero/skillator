## Context

The Target workspace already loads a Library snapshot. Scope navigation retains only browsing state; Library construction repeats discovery synchronously. Existing follower inspection uses workers and request identities.

## Goals / Non-Goals

Goals: immediate cached Library rendering; background discovery; stable selection/filter/collapse state; no lost staged changes; unchanged fresh save validation.

Non-goals: rewriting the scanner, Git metadata optimization, background save commits, filesystem watching, or changing CLI/SSH behavior.

## Decisions

- Retain an immutable shared Library snapshot and an owned Library view in session navigation. Seed Library from the Target's existing snapshot instead of scanning again. Reuse snapshots only with matching loaded configuration fingerprints; reload configuration cheaply on entry.
- Use one in-flight discovery worker and coalesce newer refresh requests. Build inventory rows off the UI thread. Request generations reject obsolete replies after location edits, undo, save or configuration replacement.
- Render retained rows immediately. A cold explicit Library launch renders location rows plus a loading status before scanning. Tick processing installs completed results only when local edits and active overlays cannot be disrupted; preserve selected row identity, filter and collapse state. Pending refresh never becomes a write.
- Retain host browsing and cached follower rows, restart background follower inspection on reentry, and cancel follower operations on exit. Host registry reload remains cheap and respects staged state.
- Existing Library acquisition/save and User/Repo save workflows remain authoritative. Cached browsing data never replaces their fresh validation. Location editing keeps its existing explicit staging validation.

## Risks / Trade-offs

Cached rows can briefly be stale; background refresh reconciles them without blocking navigation. A refresh finishing during staged edits must wait rather than overwrite selections or acquisition modes. A superseded scan may finish, but only one scan runs at a time and its result cannot resurrect obsolete configuration. Cold root Target initialization still needs its initial snapshot; this change targets scope-switch discovery waits.
