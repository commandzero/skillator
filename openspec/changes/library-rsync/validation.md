# Implementation validation

Validated on 2026-09-19. The planning artifacts were committed as `2fa9b0b`, `docs: specify library rsync across SSH hosts`.

## Repository checks

- `bash scripts/preflight.sh` passed with pinned Rust 1.97.1. The run included formatting, strict Clippy, all-target locked tests, doctests, shell and workflow checks, OKF documentation validation, main-spec validation, repository-tool tests, and release safeguard tests.
- The complete Rust test run passed 273 tests. It includes 30 remote module tests and six CLI protocol/report tests.
- `OPENSPEC_TELEMETRY=0 .tools/node_modules/.bin/openspec validate library-rsync --strict --no-interactive` passed.
- Full preflight diagnostics were retained locally at `/tmp/skillator-rsync-full-preflight.log`.

## Real SSH and rsync acceptance

The initiator ran macOS 26.6.2. Two SSH accounts ran in a disposable Debian 13 aarch64 container with OpenSSH, Git, rsync, and a Skillator binary built using Rust 1.97.1. The server listened through a localhost-only published port and used disposable host and client keys. This exercised real SSH and rsync processes between macOS and Linux, not the in-process endpoint adapter. Linux also passed all 30 synchronization module tests.

One remote home and the initiating home contained spaces. The Git source path contained spaces, and a supporting filename contained an apostrophe. The fixture included a central skill library, an external Git source, and a user-linked skill.

Verified outcomes:

- Read-only check reported absent exact-commit clones and unverified follow-on work without creating receiving configuration, local history, or remote library files.
- Origin advanced from commit A to B before application. Both receiving checkouts stayed at A, received the initiating dirty tracked skill edit, and retained clean indexes. User links used receiving home paths.
- A partial initial run encountered a fixture Git ownership restriction. After correcting trust in the disposable fixture, retry completed without losing the earlier successful work. Subsequent runs had no content changes.
- Selecting one host left the other untouched. A later all-host run propagated the remote edit to the other host.
- Default missing-file copying restored an absent file. `--missing remove` subsequently removed a history-proven deletion everywhere.
- Different remote edits remained unresolved under both `ask` and `remote`. Explicit `local` selected initiating absence.
- Temporarily removing the receiving Skillator executable produced status 3, named the configured alias, and left participant file fingerprints unchanged. An incompatible version shim produced the same protection and reported its version. Both fixture executables were restored after each check.
- An interactive PTY session displayed conflict candidates, accepted Enter to skip, returned status 1, and retained every candidate.

The local acceptance driver and reports were retained under `/tmp/skillator-rsync-acceptance.py` and `/tmp/skillator-rsync-acceptance-n8svai4i/`. The disposable Docker container was removed after validation. This does not claim validation on native Windows, WSL, Ubuntu's release baseline, or Linux x86_64.

## Failure and scope coverage

The repository tests use disposable homes and real local Git repositories. They cover multi-host same-run convergence, restore and removal history, competing remote edits, source identity collisions, exact-commit cloning, an unobtainable commit, existing commit mismatches, worktrees and submodule roots, unmerged indexes, environment path expansion, invalid hidden skills, exclusions, internal links, physical acquisition aliases, file/directory conflicts, and unchanged unrelated content.

Transport, publication, staged verification, and acknowledgement failures are injected independently. Retry retains participant identity and converges. Tests also preserve intervening source/destination edits, escaped parents, interrupted-publication backups, unmanaged user entries, and copied user drift. User directory removal stays coupled to concurrent selection changes. Machine previews produce equivalent JSON and YAML, and failed all-host preflight writes nothing.

## Verification follow-up

The user removed task 7.5 because repository management is outside the spec's implementation tasks. The change now contains 26 tasks, all checked. No archival or repository-management work was added back.

All four warnings from the initial verification have been addressed:

- Ignoring missing content defers registration of a receiving location that was neither present nor prepared. Apply and check succeed without creating the location.
- Observation recognizes absent descendants beneath a verified regular-file ancestor. Mutation paths retain strict containment. Nested directory/file conflicts resolve under local and remote policies and converge on retry.
- Synchronization history retains the initiating source identity, origin, commit, and optional branch. Detached receiving checkouts do not affect revision compatibility or overwrite initiating branch provenance.
- Integration tests now cover conflicting first-contact tracked Git edits under every policy, byte-for-byte index preservation, and the union of independent first-contact user selections.

The follow-up synchronization suite passed 36 tests. Full pinned-toolchain preflight passed with 279 Rust tests, and strict OpenSpec validation passed. Both original CLI reproductions now return status 0. See `verification.md` for the regression cases and evidence.
