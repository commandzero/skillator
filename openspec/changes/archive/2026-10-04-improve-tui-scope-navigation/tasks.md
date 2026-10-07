# Tasks

## 1. PR38 baseline and host configuration

- [x] 1.1 Integrate PR38's replacement leader-authoritative delivery contracts before implementation; verify `src/remote/config.rs` and the synchronized `library-rsync` spec are available and document ordinary SSH read-only replica inspection in the design.
- [x] 1.2 Extend the existing strict host entry with optional validated `hostname` metadata; verify destination-only entries still load, differing alias/hostname values retain the original SSH destination, and malformed metadata and unknown fields are rejected.
- [x] 1.3 Add staged host-registry preparation and atomic publication using existing containment and stale-write safeguards; verify an actual isolated-HOME save/reload preserves other entries and that stale, invalid, canceled, and unsaved cases preserve configuration bytes and skill state.
- [x] 1.4 Update the existing library-rsync documentation with the optional metadata, destination/hostname distinction, and older-build downgrade limitation; verify the examples round-trip through the updated configuration reader.

## 2. Scope navigation and rendering

- [x] 2.1 Replace flattened scope/directory selection with explicit Library/User/Repo and stable per-scope sub-tab identities while retaining configuration-owned workflow sessions; verify saved `.agents` and `.claude` directories restore without changes to keys, paths, labels, enablements, or inherited-state semantics.
- [x] 2.2 Implement Ctrl+Left/Right scope cycling, Tab/Shift+Tab scoped sub-tab cycling, and Ctrl+L Library/last-directory-scope return; verify overlay precedence, wrap behavior, remembered selection, and empty/unavailable Repo navigation through actual reducer/session interactions.
- [x] 2.3 Render the first-line tabs and right-aligned title, second-line sub-tabs, scope-colored borders, and bottom scope/path status; exercise the actual TUI at normal and narrow sizes to verify no header overlap, selected-tab visibility, full inspector paths, and Library/User/Repo border colors.
- [x] 2.4 Apply dirty-transition guards to scope, repository and Library-host changes while preserving staged edits across directories in the same configuration; verify save/discard/return and implicit first-run defaults never route edits into another configuration or remote host.
- [x] 2.5 Remove obsolete mixed-tab rendering and old header paths, and update README, Help, welcome text, and the Unreleased changelog for the new navigation; verify help and first-run screens describe the same keys and scope destinations as the live TUI.

## 3. User and Repo directory chooser

- [x] 3.1 Replace the hard-coded `.claude` Ctrl+T editor with the shared filter-as-you-type chooser for Ctrl+T and directory addition; verify label/path matching, literal `j/k/q` input, Up/Down scrolling, stable selection, Enter acceptance, and Esc cancellation.
- [x] 3.2 Reuse existing Generic/Codex and Claude presets and scope-relative validation for custom paths, unique keys, and editable labels; verify unavailable configured suggestions, duplicate/overlapping paths, containment violations, and no-match custom acceptance produce the specified state and errors.
- [x] 3.3 Connect chooser acceptance to only the active scope's staged directory configuration; run isolated User and Repo save/reload scenarios with both `.agents` and `.claude` and prove independent enablements, no eager filesystem writes, and unchanged other-scope configuration.
- [x] 3.4 Replace obsolete chooser-default/wording tests with behavior coverage and document presets/custom paths in README and Help; verify the documented interaction in the actual TUI.

## 4. Library follower creation

- [x] 4.1 Add scope-aware Ctrl+T follower setup with `Follower name`, the SSH-config credentials/host-trust hint, unique nonreserved alias validation, and pending registration state; verify invalid or duplicate names spawn no SSH process and the literal input field captures workspace keys.
- [x] 4.2 Run an argument-separated, bounded and cancellable SSH hostname probe using PR38's authentication/trust safeguards; verify actual probe execution through controlled executable fixtures covers timeout, cancellation, oversized output, nonzero exit, and late-result rejection without freezing the TUI.
- [x] 4.3 Parse stdout independently from sanitized stderr and stage only a successful unambiguous hostname; verify hostname/SSH-alias differences, valid stderr warnings, banners on stdout, blank/multiline/malformed/control-sequence output, LF/CRLF, and nonzero exit with plausible stdout.
- [x] 4.4 Bind successful probes to explicit host-registry save and pending Library sub-tabs; verify save/restart persistence, discard and quit preservation, stale-write rejection, and independent outcome reporting when Library inventory and host plans both have work.
- [x] 4.5 Document follower setup and failure remediation in README/Help and the existing remote guide; verify a real trusted-key SSH setup succeeds with a differing alias/hostname and does not modify SSH configuration, trust files, or synchronize content.

## 5. Follower Library inspection

- [x] 5.1 Expose genuine read-only inspection of the owned follower replica through ordinary SSH/POSIX tools without remote Skillator or Git; verify Source/Skill metadata and diagnostics while configuration and skill content remain unchanged.
- [x] 5.2 Connect Local/follower sub-tabs to host-specific inventories, host-qualified status/inspector paths, read-only actions, bounded loading, and resource cleanup; verify switching and cancellation cannot publish a late response into another host's view or substitute local rows for remote rows.
- [x] 5.3 Preserve offline, absent, and unsafe follower replicas with actionable diagnostics; exercise missing/unmarked replicas and unreachable hosts while confirming Local, User, and Repo remain usable and mutation actions cannot alter inventory.
- [x] 5.4 Update the follower-view documentation and relevant behavior tests; verify docs accurately distinguish SSH verification from delivered replica availability and leader-authoritative rsync from read-only tab browsing.

## 6. End-to-end acceptance

- [x] 6.1 Run the repository preflight after the complete implementation and record only observed checks; all changed behavior scenarios and existing reconciliation/inheritance contracts must pass without wording-only or implementation-detail assertions.
- [x] 6.2 Exercise the actual TUI using an isolated HOME, temporary Git repository, and disposable SSH follower: navigate every scope, add both directory presets and a custom path, register a differing alias/hostname, browse follower inventory, resize, discard staged edits, save/restart, and quit; verify file-state isolation and terminal restoration and record host/terminal evidence.
- [x] 6.3 Review every delta requirement against the implementation, synchronize main specs, archive this change with completed task evidence, and record the repository-required semantic review receipt; verify strict OpenSpec validation and the scoped PR contract before merge.

## Verification evidence

- Jetfire, macOS arm64: `cargo build --locked` and `bash scripts/preflight.sh` passed with the pinned Rust toolchain. Preflight covered Clippy, all-target tests, documentation tests, filenames, OKF, all 12 main specs, workflow/shell checks, and release safeguards.
- Native PTY (`TERM=xterm-256color`, 100×28, 140×35, 55×18 and 22×16) with isolated HOME and temporary Git checkout: scope colors/header/status, directory presets/custom paths, User/Repo save isolation, inheritance override, scoped Tab wrap, Ctrl+L restoration, save/undo scope restoration, narrow header, help and full inspector paths observed.
- Disposable trusted-key OpenSSH daemon: `scope-peer` reports `Jetfire`; explicit registry save/restart restored both names. Replica inspection succeeded with a PATH containing only required POSIX tools and no Skillator/Git. Follower and SSH credential/trust/config byte fingerprints were unchanged.
- Warning, stdout banner, cancellation and late-result window, stale registry, invalid registry, missing/unmarked replica and offline host exercised. Process regression fixtures covered deadlines, oversized output, nonzero exit and cancellation. Terminal echo, canonical mode, signals, cursor and alternate screen restored on exit.
- Reproduced and corrected User save returning to Repo, invisible probe warnings, host-only undo and target-transition guards, interrupted inspection not resuming, stale Local status, false follower pending actions and missing remote inspector path components. Acquisition save-and-switch reproduced an unreviewed move before the fix; after the fix cancellation moved nothing and explicit acquisition confirmation alone moved the disposable skill and switched scope.
- Standards and semantic reviews closed actionable findings. Remaining optional maintainability heuristics do not change the specified behavior. Main-spec synchronization preserved unrelated requirements and scenarios.
