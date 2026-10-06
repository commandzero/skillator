## 1. Responsive cached views

- [x] 1.1 Retain the loaded Library snapshot/view, seed it from Target, and render cached or cold Library locations without waiting for discovery.
- [x] 1.2 Refresh inventory/rows on a single background worker, coalesce requests, preserve browsing and staged edits, reject obsolete results, and retain follower cancellation and fresh save validation.

## 2. Proof and contracts

- [x] 2.1 Keep deterministic regression coverage for refresh/state races; exercise real native cached entry, cold Library entry, discovery changes, edits, save/undo and follower lifecycle, comparing latency against the measured baseline.
- [x] 2.2 Update documentation/changelog and synchronize TUI requirements; run full preflight and strict change/main-spec validation.

## Native verification

- macOS arm64 PTY: actual configured Library entry improved from the measured 559–578 ms baseline to 11–23 ms, including the first Target-seeded entry. With a real Git wrapper delaying each invocation by 200 ms, cached entry took 8 ms and cold Library accepted input while discovery was running.
- Six rapid refresh requests produced one scan (six real Git calls). External additions appeared asynchronously; filters, collapsed groups and selected skill identity survived refresh. Staged visibility/acquisition edits and editor input were not overwritten; undo rejected obsolete results and restored current inventory.
- Replacing configuration while an older result was queued showed only the new configured location. After a scan completed in an inactive scope, a subsequent disk addition appeared through a fresh scan on re-entry (14 ms cached entry).
- Changing a location into an overlapping symlink after caching caused Save to report the overlap; configuration bytes remained unchanged.
- Actual SSH against a deliberately stalled loopback handshake was canceled on scope/host exit. Re-entry retained follower selection, rendered in 7 ms and started a new background connection. This proves cancellation/restart, not successful replica inspection.
- Three deterministic filesystem-backed regression tests cover superseded discovery, staged visibility/acquisition changes, and editor/browsing identity. The Library-focused run passed all eight tests.
- README, contributor smoke guidance and changelog updated; all seven new scenarios synchronized into the main TUI specification. Native Linux/WSL verification was not performed. The changelog entry remains an unnumbered draft pending an implementing PR reference.
- Full `bash scripts/preflight.sh` passed: 316 unit/integration/repository-check tests, formatting/lint gates, filename/OKF checks, all 12 main specifications and release safeguards. Separate strict validation of this change and all main specifications passed.
