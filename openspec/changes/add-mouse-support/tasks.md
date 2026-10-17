# Tasks

## 1. Render-derived interaction geometry

- [x] 1.1 Use LSP references for the exported render/action interfaces before changing them; introduce the frame-local typed interaction map and migrate affected callers without shims. Verify all known render entry points and existing public TUI tests compile against the new contract.
- [x] 1.2 Compute drawing and hit regions together for scope/sub-tab spans, table columns and visible row bodies/cells; use the final rendered TableState offset, filtered inventory mapping, terminal-cell widths and clipped rectangles. Add behavioral coordinate tests proving a click targets the displayed row after keyboard scrolling, filtering and collapse, including Unicode labels, narrow sizes and empty table space.
- [x] 1.3 Rebuild or invalidate the map on resize, model changes, scope/host/directory changes and asynchronous refresh before accepting another mouse event. Verify a resize and a refresh inserting an earlier row cannot redirect a click to an obsolete inventory index; document frame ownership and invalidation beside the interaction-map boundary.

## 2. Mouse lifecycle and addressed table/tab actions

- [x] 2.1 Enable mouse reporting once in TerminalSession and disable it during Drop and every partial-setup cleanup path while retaining the original error. Add lifecycle coverage that observes restoration commands for normal exit and injected setup failure; verify non-interactive entry points never enable capture and document terminal-specific selection bypass in README.
- [x] 2.2 Decode unmodified left presses and vertical wheel events in input.rs and dispatch them through the shared event loop; reject release, motion, drag, other buttons, modified input and horizontal wheel events. Add behavioral tests proving press/release toggles once and unsupported events cannot accept a destructive choice.
- [x] 2.3 Implement atomic addressed row selection/toggle/mode/disclosure actions through the reducer's existing guards. Add tests for clicking a row other than the prior selection, body selection without edits, Source bulk modes including hidden children, filtered disclosure state, inherited overrides, saved repository-owned rows, invalid/unavailable entries and read-only followers.
- [x] 2.4 Add direct scope/directory/host actions sharing the existing navigation and dirty-switch logic with keyboard cycling. Test Library-to-Repo selection without intermediate User navigation, exact guarded destinations, save/discard/return outcomes, active-tab no-op, same-scope staged-edit retention, Repo unavailable state and scoped overflow arrows.
- [ ] 2.5 Extend Help and README with row/cell/tab gestures and preserved keyboard alternatives. Verify rendered Help retains the full existing key and mode reference and its mouse descriptions match the implemented actions.

## 3. Clickable commands, modal choices and editors

- [ ] 3.1 Render workspace commands from typed label/action descriptors and register their actual visible bottom-border regions. Split Library Add/Edit/Remove labels and expose Details. Test selected-row Details/Edit behavior, distinct location commands, right-aligned/clipped label targeting, ordinary Save versus safe fast Save and scope-sensitive follower Sync without bypassing confirmation.
- [ ] 3.2 Give every overlay its semantic choice descriptors: Welcome; ordinary/Guarded save; dirty-target/scope navigation; delete; Busy; follower probe/sync/initialization/results; Help/details/diagnostics; and save-result exit. Verify representative affirmative/negative/cancel actions, nonzero failed-save acknowledgement and follower initialization sequencing with reducer/effect tests, plus explicit Blocked-work non-authorization.
- [ ] 3.3 Make editor Apply/Cancel, chooser Stage/Cancel, Filter Apply/Clear and supported path Complete controls clickable; suggestion clicks only select. Test selected-suggestion staging, duplicate and invalid custom-path rejection, cancellation preserving unrelated edits, path completion without submit and literal keyboard q after input clicks.
- [ ] 3.4 Enforce overlay-only hit resolution, including Filter, with non-modal Notice as the exception. Add tests for outside click/wheel suppression, cancel above an underlying checkbox, overlay replacement without event reuse and navigation through a status-only notice.
- [ ] 3.5 Document clickable commands, explicit modal choices and keyboard-entered text in Help/README. Verify the documented controls are visible and actionable at normal terminal sizes and hidden clipped regions remain inert at narrow sizes.

## 4. Surface-routed wheel navigation and bounded content

- [ ] 4.1 Route each vertical wheel event over table or chooser content to one visible-row or suggestion movement, without cycling tabs or editing desired state. Test first/last boundaries, empty/filtered/collapsed inventories, selected-row visibility and chooser matches; verify background movement remains blocked by overlays.
- [ ] 4.2 Calculate scroll limits from rendered content and viewport geometry for Help, Details, Diagnostic and FollowerSyncResult; clamp both keyboard and wheel movement after scroll, resize and content changes. Add tests using short and long wrapped documents proving content stays visible at both ends and resizes cannot leave a blank over-scrolled view.
- [ ] 4.3 Add wheel gestures and terminal verification scenarios to Help, README and docs/contributing.md, and record the feature in CHANGELOG.md following repository policy. Verify documentation distinguishes wheel selection from checkbox edits, unsupported gestures and terminal-native selection from application behavior.

## 5. End-to-end integration verification

- [ ] 5.1 Run the focused internal/public TUI suites and normal repository preflight after integrating the feature. Verify existing keyboard, staging, save classification and follower cancellation tests remain valid; remove incidental wording/wiring assertions instead of repinning them to new labels.
- [ ] 5.2 Launch the real TUI with isolated HOME and a temporary Git repository containing multiple scopes/directories and enough Skills to scroll. Exercise real mouse reports for select/toggle/mode/disclosure, direct/dirty tab switches, chooser and editor choices, Save cancel/confirm, Help/details wheel bounds, keyboard continuation, refresh and resize; observe the surface and inspect files to prove no writes before save and only intended scope writes after confirmation.
- [ ] 5.3 Observe terminal mouse reporting being disabled on normal and handled-error exits, verify keyboard-only use remains possible, and validate the completed change with `openspec validate add-mouse-support --strict`. Record only exercised commands/scenarios and any terminal-emulator limitations in the implementation delivery.
