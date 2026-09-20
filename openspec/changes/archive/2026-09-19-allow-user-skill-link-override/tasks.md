## 1. TUI behavior

- [x] 1.1 Retain User Scope inheritance across staging and reload, allow `m` to stage a repository link, and make Space return explicit overrides to inherited state.
- [x] 1.2 Cover mode transitions, pending actions, discard, unavailable Skills, and existing link/copy behavior with focused TUI tests.

## 2. Save and documentation

- [x] 2.1 Verify save, reload, and removal through the repository workflow, including destination conflicts and unchanged User Scope files.
- [x] 2.2 Update Help, inherited-row notices, and the README to explain the override and its removal.

## 3. Validation and completion

- [x] 3.1 Run the repository preflight and strict OpenSpec validation for this change.
- [x] 3.2 After implementation, synchronize the delta into the main spec and archive this change before merge.
- [x] 3.3 Test the TUI in tmux with an isolated home and Git repository, including save, reload, removal, cancellation, conflicts, and terminal restoration.
- [x] 3.4 Verify implementation completeness, correctness, and coherence against the archived OpenSpec artifacts.

Validation results and scenario coverage are recorded in [verification.md](verification.md).
