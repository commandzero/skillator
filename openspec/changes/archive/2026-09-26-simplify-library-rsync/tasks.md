## 1. Content-only synchronization

- [x] 1.1 Remove remote user-state snapshots, history, protocol operations, planner selection mode, and application helpers; verify normal user commands remain and sync leaves differing or malformed host-local user configuration/materializations untouched.

## 2. Shared safety and implementation mechanics

- [x] 2.1 Consolidate stage descriptors and guarded transfer operations while preserving both participants' containment/identity checks; verify stage-swap and outside-home regressions.
- [x] 2.2 Deduplicate hashing/copying and Git comparison logic without dropping observation checkpoints; verify existing content, executable-mode, alias, and Git mismatch behavior.
- [x] 2.3 Consolidate library/skill test fixtures without dropping distinct surviving safety cases; run the existing suite after integration.

## 3. Batched payloads

- [x] 3.1 Batch explicitly selected verified exports per source/destination peer through the initiator; verify multi-file bidirectional transfers and exact-commit bootstrap.
- [x] 3.2 Preserve per-entry publication, acknowledgement, and recovery across failed batches; verify partial failure, conflicts, retry, and read-only preview.

## 4. Integration

- [x] 4.1 Update main specs, CLI help, usage docs, and changelog for host-local selections and the protocol cutover; validate OpenSpec and repository documentation.
- [x] 4.2 Run pinned-toolchain preflight and an actual multi-host SSH/rsync smoke with isolated homes; record outcomes and archive the completed change.

## Verification evidence

- Pinned-toolchain `bash scripts/preflight.sh` passed: Rust 1.97.1 formatting and Clippy checks, 364 tests, documentation tests, shell/workflow checks, OKF documentation validation, all 11 main OpenSpec specifications, and release safeguards.
- Strict validation of `simplify-library-rsync` passed.
- Actual SSH/rsync smoke passed from macOS arm64 to two isolated Linux arm64 accounts in a disposable OpenSSH container. A receiving Linux account also initiated a successful GNU rsync push/pull against the other account.
- Exact-commit bootstrap retained commit A after the origin advanced to B. Dirty tracked skill content synchronized without changing either index.
- Multi-file push used one payload transfer per peer; remote-to-local-to-peer relay used two transfers. Seventy new supporting files used four bounded transfers across two peers. A converged retry used none.
- Paths containing spaces and apostrophes, empty files, executable mode, and internal relative links survived transfer.
- An injected exit 23 after rsync transferred bytes left the failed peer's originals unchanged while the independent peer completed. The existing divergent-history guard blocked an unrestricted retry; `--hosts a` recovered the failed peer, after which the complete cohort converged.
- Competing remote edits remained unresolved under `--conflict remote`; `--conflict local` resolved them. Proven deletion propagated with `--missing remove`.
- Different malformed user configurations and copied materializations remained unchanged. Read-only preview performed no payload transfers or persistent writes.
- Removed the disposable runner, SSH keys, isolated homes, and smoke scripts after verification.
