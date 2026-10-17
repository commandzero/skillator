# Design

## Context

See `proposal.md` for motivation and `specs/tui-workflows/spec.md` for the behavior contract. This is a cross-cutting interaction change, so a design artifact is required.

- `src/tui/mod.rs` owns one `TerminalSession` across scope navigation. Setup enables raw mode and the alternate screen; Drop and setup-error branches restore them. The shared event loop handles keys and resize but discards other events.
- `input.rs` maps keys to `Action`; `reducer.rs` changes in-memory state and returns `Effect` values. Library/Target handlers own I/O and guarded navigation. Reusing these boundaries avoids a second persistence path.
- `render.rs` computes the two tab strips, table, legend and overlays. `render` creates a local `TableState` and lets Ratatui adjust its offset to the selected visible row. No hit-test geometry is currently retained.
- Visible rows differ from inventory indices because of filtering and collapsed Sources. Sub-tab labels retain underlying directory indices across User/Repo filtering. Labels and footers can be clipped, right-aligned or contain non-ASCII text.
- Overlays include confirmations, editors, a chooser, Help/details/diagnostics and asynchronous follower states. `Overlay::Notice` is status text, not an occluding modal. Existing keyboard confirmation messages sometimes embed footer prose in strings.

## Goals / Non-Goals

**Goals:**

- Produce one render-derived, frame-local interaction map; never independently reconstruct screen geometry in input decoding.
- Route addressed actions through the same reducer guards and effect handlers as keyboard actions.
- Keep frame work proportional to rendered content and avoid scanning inventory or performing I/O per pointer event.
- Preserve keyboard contracts, scope state ownership, presentation hierarchy and async refresh responsiveness.

**Non-Goals:**

- No general widget/focus framework, new persistence model or dependency.
- No second mouse-specific save, synchronization, validation or navigation workflow.
- No additional gestures, editor text model or terminal capability negotiation beyond the proposal's scope.

## Decisions

### 1. Capture mouse input for the lifetime of TerminalSession

Use existing Crossterm mouse capture commands when entering the alternate screen, before the first interactive frame. Disable capture before leaving the alternate screen during Drop. Attempt the same restoration in every setup-error branch, including partially executed terminal command batches and terminal backend construction failure. Cleanup remains best-effort so it cannot replace the original error. Keep this inside `TerminalSession`; scope changes must not repeatedly enable/disable capture.

Do not enable capture for non-interactive commands. Terminals that send no mouse events continue using keys. No new setting or terminal probe is needed. Restoration covers normal exits and handled errors; uncatchable process termination cannot be guaranteed.

**Alternative rejected:** capturing separately in Library and Target loops duplicates cleanup and risks leaving capture enabled between workflows.

### 2. Render controls and their hit regions from the same geometry

Add a small interaction-map value owned by the shared event loop and rebuilt by the renderer for each successful frame. It holds the frame rectangle, visible target rectangles, typed target payloads and scrollable content bounds. Reuse its backing storage between frames. Payloads use existing scope values and current row/directory/host indices rather than copying Skill metadata or paths.

Compute and consume tab spans, column geometry, control-label widths and footer alignment once for drawing and registration. Use terminal-cell width measurement, not byte lengths. Intersect target rectangles with the actual visible area; hidden/zero-width controls have no targets. Preserve existing table layout constraints and spacing, resolving them consistently for painting and hit testing.

After `render_stateful_widget`, use the final `TableState` offset, table inner area and header height to associate displayed row lines with `visible_indices()`. Do not assume the first visible inventory item is the first on-screen row. Only register the actual checkbox glyph cells, the Skill Mode cell, the Source disclosure glyph and the remaining selectable row body. More specific cell targets win over the body target, with one resolved operation per event.

Clear or replace the map when model state, scope, filter, collapse, overlay, viewport or async results change; redraw before processing another mouse action. A resize invalidates the old map immediately. If geometry is absent or its terminal size is no longer current, ignore the event until rendering restores it. Never retain addressed payloads across frames or navigation.

**Alternative rejected:** hard-coded x/y tests in `input.rs` drift from right-aligned footers, clipped Unicode labels and Ratatui's selected-row scroll offset. Persisting an unrelated scroll offset is unnecessary for selection-following table navigation.

### 3. Decode pointer events into one semantic reducer action

`input.rs` accepts the event and current interaction map. Accept only unmodified left-button presses and vertical scroll events. Ignore release, drag, motion, middle/right buttons, modified events and horizontal wheel events. A double press has no special meaning; there is no timing or double-click state.

Use a typed addressed action for row activation with an intent of Select, Toggle, SwitchMode or ToggleDisclosure. The reducer validates that the row is still visible, selects it and executes the existing semantic operation with its existing restrictions. Keep the compound operation inside one reducer invocation rather than emitting unrelated Select and Toggle actions that could be separated by an overlay transition. Do not reject all unavailable rows in hit testing: keyboard actions sometimes provide an explanation or allow removal of an existing unresolved enablement.

Introduce direct scope, directory and host selection actions. Share guarded destination-selection logic with Next/Previous actions, rather than simulating repeated key presses or mutating model indices in the input layer. Validate the scope/host/directory relationship and short-circuit the current destination. Emit the established `DirectoryChanged` or `SwitchToScope` effects and retain the exact requested destination in the existing dirty-switch overlay. Overflow arrows use existing one-step scoped navigation.

Existing `Action` variants continue representing ordinary commands. Follow repository LSP reference checks before changing exported action/render interfaces, then migrate all callers and tests without compatibility shims.

**Alternatives rejected:** simulating key events hides intent; cycling repeatedly to a clicked tab can trigger intermediate guards. Executing mouse I/O directly bypasses save-time safety and retained-state ownership.

### 4. Use semantic control descriptors, not prose parsing

Represent each clickable legend or overlay choice with its label, keyboard hint and semantic action. Render the label and register its rectangle together. Reuse existing palette and bottom-border placement. Separate Local Library's `a/e/d location` into Add, Edit and Remove controls, retaining their keys. Add a compact Details control mapped to existing Confirm behavior for the selected non-diagnostic row.

Workspace legend commands keep existing scope-sensitive semantics: Save and Save and Exit prepare ordinary plans; follower Sync requests only the selected saved follower. No pending Action cell becomes an immediate execution button. Clipped controls cannot activate beyond their visible cells; preserve the keyboard alternatives when width is too small to show every workspace command.

Build modal controls from the typed `Overlay` variant, not by interpreting words such as Enter, discard or save in rendered messages. Keep warning/plan body content intact. Cover Welcome continue/exit; save and Guarded proceed/return; discard-target choices; dirty-scope save/discard/return; delete/cancel; Busy retry/return; follower probe cancel; follower sync/initialization sync/skip; follower results/details/diagnostics/Help close; and save-result acknowledgement/exit. Preserve exit status and follower initialization effects, not just visual dismissal.

Editors expose Apply/Cancel, choosers Stage/Cancel, and Filter Apply/Clear. Show Complete only for existing Location/Target path completion. Suggestion clicks only select; Stage validates and applies that selection or the existing custom input. Input-body clicks are consumed without submitting or changing the append/backspace cursor model. Compact modal labels as needed so actionable choices remain distinct within the available border; keyboard access remains available at sizes too small to display controls safely.

**Alternative rejected:** parsing footer strings couples behavior to wording and can turn a harmless return action into destructive confirmation.

### 5. Enforce overlay isolation before hit testing

If a real overlay is rendered, its interaction layer replaces background targets for dispatch, including the top-level tabs and workspace footer. Outside clicks and wheel events are consumed as no-ops. Resolve an event against one pre-action layer only: closing or replacing an overlay cannot reuse the event on the next layer. Treat status-only Notice as the normal workspace, preserving existing notice dismissal semantics through the reducer.

No outside-click dismissal is added: explicit controls avoid discarding editor input or accidentally authorizing save/sync actions. The same isolation applies to Filter even though its rectangle is small.

**Alternative rejected:** hit-testing all targets back-to-front without explicit modal blocking allows background clicks through empty portions of a dialog.

### 6. Wheel navigation follows existing movement semantics with bounded scrolling

Register content rectangles as wheel targets. Over a main table body, vertical wheel events request one visible-row movement and selection remains in view. Over the chooser list they move one matching suggestion. Over Help, Details, Diagnostic and FollowerSyncResult content they move one rendered line. All other surfaces ignore the wheel, including confirmation dialogs and tab strips.

For scrollable modal views, calculate maximum offset from content height and visible inner height using the same wrapping/row layout as drawing. Clamp offsets after movement, content change and resize, including existing keyboard movement so mixed input cannot leave the view blank. Do not rely only on `u16` saturation, which currently allows scrolling beyond content. Empty chooser/table input remains a no-op.

**Alternative rejected:** moving background selection whenever a wheel event arrives ignores modal focus and the surface under the pointer; unbounded offsets lose the content entirely.

## Risks / Trade-offs

- [Ratatui layout and viewport mismatch] → Record actual rendered offsets and shared geometry; verify clicks after keyboard scrolling, filtering, collapse and resize, not just at initial offset zero.
- [Stale indices after asynchronous refresh] → Rebuild before accepting mouse events and discard invalid geometry; exercise inventory insertion and destination changes during the terminal smoke run.
- [Accidental destructive activation] → One press, one semantic action; no click-through, outside dismissal or string-derived action decoding. Reuse all existing confirmation and read-only guards.
- [Legend width pressure] → Compact individual labels and register only displayed cells. Keyboard bindings remain available; do not add a second footer rule or redesign the screen.
- [Mouse capture changes terminal text selection] → Document terminal-specific bypass gestures without promising a universal modifier. Modified mouse events are ignored by the application.
- [Wrapped text height differs from scroll bounds] → Use the renderer's content width and wrapping semantics for bounds and check long documents at narrow sizes.
- [Lifecycle failure testing is limited by real terminal errors] → Verify captured terminal command cleanup in deterministic lifecycle tests and exercise normal/handled-error exits in an isolated PTY; do not claim restoration after SIGKILL.

## Verification Plan

- Use existing `src/tui/tests.rs` and `tests/tui.rs` patterns to drive coordinates from rendered frames, then assert observable selection, desired state, destination, guard choices, effects or scroll behavior. Avoid tests that only compare forwarding functions or source text.
- Cover an addressed checkbox different from the prior selection; press/release exactly-once behavior; mode/disclosure distinction; filtered/collapsed/scrolled inventories; Unicode/clipped tabs; active-tab no-op; dirty scope/host destinations; read-only/inherited/repository rows; chooser validation; editor text capture; modal no-click-through; wheel bounds and keyboard coexistence.
- Run focused TUI tests and the repository's normal preflight after integration.
- Launch the actual TUI with an isolated HOME and temporary Git repository containing enough skills to scroll, multiple directory tabs and representative modal content. Send real terminal mouse reports or use a mouse-capable terminal; observe click selection/toggle/mode, tab and dirty-switch behavior, chooser/editor controls, Save cancel/confirm, Help/details scrolling and resize alignment. Inspect persisted state to prove unsaved clicks do not write and confirmed save writes only the intended scope. Verify exit disables reporting. Remove throwaway fixtures afterward.
- No network follower transfer is required solely to prove input dispatch: existing follower tests can check semantic confirmation/cancellation and duplicate-operation guards without live network dependencies. Report any real-terminal or terminal-emulator limitations in implementation verification.

## Migration Plan

Ship as an additive interaction feature using existing dependencies and configuration. Update every affected public render/action caller at cutover, plus Help, README, contributor terminal checks and CHANGELOG. No user data migration is needed. Reverting the feature restores keyboard-only operation without changing saved formats; the reverted terminal lifecycle must still disable any capture enabled by its session.
