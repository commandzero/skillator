## 1. Context and scope controls

- [x] 1.1 Select Repo in Git and User in physical home, including home-as-Git and absent Library config; verify native root startup and explicit Library onboarding.
- [x] 1.2 Map Ctrl+H/Ctrl+L to directional scope cycling, remove Ctrl-arrow/toggle state, and migrate callers; verify native wrap/reverse navigation from root and library-only launches, dirty guards and editor Backspace.
- [x] 1.3 Use gray 245 for Library borders/accents without changing bone titles or other scopes; verify actual terminal palette cells.

## 2. Verification and contracts

- [x] 2.1 Update Help, README, contributor checks and changelog; exercise documented paths, run complete preflight, and validate/synchronize the modified main requirements without editing predecessor archives.

Verification: native macOS arm64 PTY, `TERM=xterm-256color`, isolated HOME and disposable Git fixtures. Passed absent/saved Library startup, home-as-Git and symlink HOME, actual Ctrl+H/Ctrl+L bytes, library-only wrap/reverse navigation, unmapped Ctrl-arrows, dirty guards/discard without writes, editor Backspace/capture, Help, User/Repo saves and scoped directory restoration, gray 245 borders/active accents and bone 230 title. Removed fixtures after proof.

Full `scripts/preflight.sh` passed, including the remaining 20 TUI tests; strict change and all 12 main-spec validations passed. Five modified requirements match the synchronized main spec and preserve prior scenarios. Two obsolete bone-frame/wording render tests were deleted rather than re-pinned. Changelog drafts await an implementing PR reference. No Linux/WSL surface check was run.
