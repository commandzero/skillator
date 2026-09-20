# Verification report: library-rsync

## Summary

| Dimension | Result |
| --- | --- |
| Completeness | 26/26 in-scope tasks complete; implementation evidence mapped to all 22 requirements |
| Correctness | Both reproduced failures fixed; direct tests added for both uncovered scenarios |
| Coherence | Initiating branch provenance now retained separately from revision compatibility |

The user removed task 7.5 because repository management is outside spec implementation. It is not an outstanding verification issue and has not been reintroduced. No repository archival or PR work is part of these fixes.

## Resolved warnings

### W1. Missing-file ignore and absent receiving locations

The coordinator now registers an incoming location only when the receiver observed it, successfully prepared content there, or would prepare it in check mode. Ignoring absent content no longer schedules an impossible registration.

Regression: `src/remote/coordinator.rs:1935`, `ignored_absent_location_is_successful_in_check_and_apply`. It runs check, apply, and check again, asserting status 0, no content/configuration changes, preserved receiving absence, and the intentional-ignore diagnostic.

### W2. Nested directory-to-file conflicts

`src/remote/state.rs:161` provides observation-only path resolution. It recognizes absence below a verified regular-file ancestor after checking physical home containment. Ordinary destination resolution still rejects that ancestor. Historical observation and acknowledgement use the observation path; publication retains strict destination checks.

Regressions:

- `src/remote/coordinator.rs:1957`, `nested_directory_replacement_resolves_and_retries_with_independent_work`: a synchronized multi-level tree is replaced with a file while the remote edits a nested child. Both local and remote policies select the correct complete value, independent content propagates, and a subsequent run makes no changes.
- `src/remote/state.rs:335`, `obstructed_observation_preserves_strict_write_containment`: observing an obstructed descendant reports absence, writing through the same ancestor fails, and a file link escaping home remains rejected.

### W3. Initiating branch provenance

Source observation now includes the initiating branch name or absence for detached HEAD. Verified acknowledgement persists source identity, origin, exact commit, and optional branch in `History.provenance`. Receiving checkouts still use detached HEAD. Branch metadata does not participate in Git revision compatibility or file-baseline identity.

Regression: `src/remote/coordinator.rs:2153`, `bootstrap_retains_initiating_branch_provenance_without_branch_alignment`. Named and detached initiating checkouts both bootstrap at the exact commit, persist the appropriate provenance on each participant, and remain idempotent despite detached receivers.

### W4. Direct integration coverage

- `src/remote/coordinator.rs:2012`, `first_contact_tracked_conflicts_preserve_indexes_under_each_policy`: two aligned Git checkouts have different dirty tracked contents before their first synchronization. Ask preserves both, local selects the initiating edit, and remote selects the changed remote edit. The checked-out references and index bytes remain unchanged under every policy.
- `src/remote/coordinator.rs:2105`, `first_contact_independent_user_selections_form_a_union`: two homes independently enable different valid skills before synchronization. Both receive the semantic union and links to their own library paths. The next run is idempotent.

## Validation

- All 36 synchronization module tests passed, including six new regression/integration tests.
- Strict OpenSpec validation passed.
- OpenSpec apply instructions report `all_done`, with 26 complete and zero remaining tasks.
- Full pinned-toolchain preflight passed: formatting, strict Clippy, 279 Rust tests, doctests, documentation, shell/workflow, main-spec, repository-tool, and release safeguards.
- Both original CLI reproductions now return status 0. Missing ignore preserves absence; nested replacement publishes the selected file. Fresh reports are retained at `/var/folders/s2/8v9rgs8n2m70c5qxy0rz97nc0000gn/T/skillator-verify-rsync-qgoyoela/results.json`.

The review covered all 22 requirements and 38 scenarios; the follow-up tests address its reported coverage gaps. Existing real SSH/Linux acceptance evidence is retained in `validation.md`; that environment was not recreated for this follow-up. The new CLI reproductions use disposable local homes, a process adapter for SSH, and real rsync. They are not described as a new real-network acceptance run.

Assessment: all four verification warnings are resolved. No outstanding issue remains from this review.

## PR review follow-up

Copilot's first review of PR #32 identified four implementation defects:

- Busy errors lost status 4. Participant lock reservation now precedes identity creation and staging, and errors retain their status. Regressions cover a participant becoming busy after observation and failure while reserving the later participant's lock, including release of earlier locks without writes.
- Broad skill roots could include machine-local control files. Collection, historical path planning, and transfer authorization exclude Git metadata, Skillator configuration and registries, synchronization state, and user/project configuration. Authorization also checks physical paths to prevent aliases from bypassing these exclusions.
- Configuration formatting changes were reported only by preview. Application now compares the same rendered-byte fingerprint and reports canonicalization writes. The regression checks preview preservation, application reporting, and subsequent convergence.
- User reconciliation diagnostics were omitted from the outer report. They now retain their fields and gain the receiving alias. Guarded materializations also carry their reason, covered by the unmanaged-entry regression.

The fifth finding claimed recovery leaves stale observations. The coordinator already re-inspects every participant after Begin and bootstrap, before file planning and acknowledgement. `recovered_exchange_is_reobserved_before_planning_and_acknowledgement` exercises a pending exchange journal through that complete path and verifies convergence in the recovery run, followed by an unchanged retry. No recovery implementation change was needed.

The second Copilot review accepted those resolutions and identified two further transfer-boundary issues. Refreshed Git source identities and commits are now compared against the original reference before file planning. A regression advances the initiating checkout after Begin and verifies that the receiver stays clean at the original commit. Default and configured User Scope skill directories are excluded from collection, planning, and authorization, including physical aliases; their content remains the responsibility of local user reconciliation.

Four additional review observations were addressed: every coordinator exit attempts Finish, missing-ignore diagnostics identify the absent participant, physical aliases publish once per destination, and a failed SSH write waits at most 250 ms for an already returning diagnostic instead of the full response timeout. Focused regressions cover early-return cleanup, alias publication counts and convergence, remote absence attribution, and failed-write response timing. Aliased publication was already idempotent in the participant, so its change eliminates redundant transfers and reporting rather than repairing the claimed stale-write failure.

The third review accepted the preceding fixes and found four location/recovery cases. Registration now checks the final physical destination for home containment, compares existing registrations by canonical destination, and rechecks destinations before saving. Tests preserve an existing symlink or `..` expression byte-for-byte and reject a final link outside the home. Recovery now refuses to move or clean a directory containing later-added entries, preserving the live directory, backup, and journal for manual recovery. Home-rooted locations receive an explicit preflight error directing users to register subdirectories; this first-version limitation is documented in the path contract and playbook. Three added regressions cover these cases without changing the 26 implementation tasks.
