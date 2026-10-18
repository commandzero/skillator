# Proposal

## Why

Skillator renders tabs, checkboxes, mode cells and modal choices but currently ignores mouse input, forcing users to learn keyboard shortcuts for ordinary visible actions. Point-and-click navigation and wheel scrolling should complement the existing keyboard workflow without weakening staged edits or confirmation safeguards.

## What Changes

- Enable terminal mouse reporting for the interactive session and restore it on exit and setup failure.
- Add left-click row selection, checkbox toggles, Source disclosure controls and Skill mode cycling in Library, User and Repo views.
- Make visible scope tabs, directory/host sub-tabs, overflow navigation and individually labeled action-legend commands clickable.
- Support modal choice buttons, chooser selection, editor apply/cancel/path completion and scrolling of Help, details and diagnostic/result views that already support scrolling.
- Map vertical wheel input to row/chooser navigation or modal scrolling according to the surface under the pointer.
- Preserve keyboard bindings, read-only constraints, dirty-scope guards, explicit save/sync confirmation and background-loading responsiveness.
- Document mouse gestures and terminal-native selection considerations in Help and the README.

## Capabilities

### New Capabilities

None. Mouse interaction belongs to the existing TUI workflow capability.

### Modified Capabilities

- `tui-workflows`: Add mouse session lifecycle, render-aligned hit testing, table and tab interaction, clickable action controls, modal isolation and wheel navigation requirements alongside the existing keyboard contract.

## Impact

- `src/tui/mod.rs`: terminal mouse capture lifecycle and event dispatch.
- `src/tui/render.rs`: shared render geometry, actual table viewport tracking and visible control hit regions.
- `src/tui/input.rs`, `src/tui/model.rs`, `src/tui/reducer.rs`: pointer decoding, direct addressed navigation/selection and reuse of guarded semantic actions.
- Existing Library/Target effect handlers remain the persistence and navigation owners; changes there are limited to integrating direct navigation if necessary.
- `src/tui/tests.rs`, `tests/tui.rs` and isolated terminal smoke scenarios cover observable mouse behavior, keyboard coexistence and restoration.
- README, contributor terminal-check guidance and CHANGELOG receive implementation-time documentation updates. No configuration format, CLI command or external service changes; use existing Crossterm/Ratatui dependencies.

## Non-goals and Assumptions

- Single left-button press activates one control; button release, motion, dragging, middle/right clicks and modified clicks do not invoke application actions. No double-click gesture or multi-select range behavior.
- Clicking a row body selects it without toggling; checkbox, mode and disclosure targets perform their named operation on the clicked row.
- Text remains keyboard-entered using the existing append/backspace editor; mouse cursor positioning, clipboard integration, hover effects and context menus are excluded.
- Clicking outside an overlay does not dismiss it or reach the underlying workspace. Status-only notices remain non-modal.
- Mouse support is enabled by default in interactive sessions; terminals without mouse reporting retain the full keyboard workflow. No new configuration switch is introduced.
