## 1. Responsive destination views

- [x] 1.1 Retain the User/Repo runtime and model and replace browsing-only caching; background-load cold destinations and refresh retained ones, proven by native bidirectional entry timing and cold explicit-Library navigation.
- [x] 1.2 Reject superseded replies and defer application for either scope's edits/overlays; preserve browsing identity and live save checks, proven by deterministic state-race regressions and native undo/save/configuration-change scenarios.

## 2. Contracts and integration

- [x] 2.1 Update README, contributor smoke guidance and changelog and synchronize the six TUI scenarios into the main specification; verify strict change/main-spec validation.
- [x] 2.2 Run full preflight and record native macOS PTY proof of cached/cold exits, external changes, edit protection and stale-save rejection; remove owned smoke fixtures.

## Native proof

- macOS arm64, xterm-256color PTY, actual configured HOME: Library → User improved from 311–317 ms to 13–14 ms; Library → Repo improved from 278 ms to 13 ms.
- Isolated HOME/Git fixtures, real Git delayed by 200 ms per invocation: cached and cold exits rendered in 6–7 ms; cold User/Repo accepted scope controls and retained a typed filter before observation completed. Final binary also exercised both directions with delayed Git.
- Completed refreshes preserved typed directory-chooser input and staged visibility/copy mode. An externally added directory stayed deferred while dirty; undo asynchronously loaded it without publishing staged content.
- External directory insertion preserved the selected `.second` directory, filter and collapsed group. Three filesystem-backed regression tests cover hidden-scope edits, directory/skill identity (including identical display names), and invalidated/replaced targets.
- Cached Save rejected externally changed configuration with exit 3; bytes were unchanged and no destination was published. Successful save created a copied skill; a second save removed it, followed by an observed clean asynchronous reload.
- Native discard-to-Library initially exposed edited rows cached as clean. The corrected scenario returned Alpha unchecked immediately (7 ms), without a pending action or write. Selecting the same target after discard also reloaded saved state. These cross-loop transitions were verified through the real PTY; the existing permanent terminal tests do not drive the navigation/effect loop.
- Owned Git/HOME fixtures and PTY processes were removed/closed; temporary diagnostic logging was removed. Native Linux/WSL checks were not performed. The new changelog entry is an unnumbered draft awaiting an implementing PR reference.
- Final `bash scripts/preflight.sh` passed after the discard/reset correction: 319 unit/integration/repository-check tests, formatting, strict Clippy, shell/workflow checks, filename/OKF validation, all 12 main specifications and release safeguards. Separate strict validation of this change and all main specifications passed.
