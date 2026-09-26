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
- Full pinned-toolchain preflight passed: formatting, strict Clippy, all Rust tests, doctests, documentation, shell/workflow, main-spec, repository-tool, and release safeguards.
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

The fourth review accepted those fixes and identified a stale registration fingerprint. Registration now retains the fingerprint of the bytes checked against the observation and uses it through publication; a concurrent-edit regression verifies that later configuration bytes are preserved. Two additional findings were fixed: Git origin validation distinguishes external-helper syntax from bracketed IPv6 addresses, and rsync text output uses the shared color policy. Coverage includes IPv6/helper-origin cases and CLI output for always, never, automatic color, and NO_COLOR.

The fifth review accepted the fingerprint correction and identified incoming aliases resolving to the home itself. Incoming source roots and locations now pass final-component containment checks before inspection, registration rejects equality with the home, and transfer authorization revalidates source and skill roots. Regressions cover home/outside aliases even when incoming sources have no skills, unchanged configuration on rejected registration, and an observed skill redirected to the home before publication.

The sixth review identified three further observation races and discovery failures. Every participant refresh now rejects a changed identity before replacing its snapshot, including an identity appearing after first contact. Remote user saves retain the request's checked fingerprint through reload, preparation, and conditional publication. Library discovery preserves directory-read, entry-read, and file-type errors as diagnostics, which stop rsync preflight before absence can enter a removal plan. Three regression tests cover replacement identities with and without prior synchronization, concurrent creation/editing of user configuration, and unreadable discovery directories with an explicit-exclusion control.

The seventh review accepted those fixes and found that location canonicalization failures still shared the absent-location diagnostic. Discovery now reserves `location_unavailable` for NotFound and emits fatal discovery diagnostics for permission, non-directory, and other I/O failures. A regression covers a location beneath an inaccessible parent, genuine absence, and a regular file occupying the location. Body-only feedback also led to operation-specific clone/fetch remediation and host-correct unmatched-removal diagnostics; tests check the unavailable-commit guidance and remote-only content attribution.

The eighth review had no open threads but identified an overly broad directory-link exception in its review body. Export, publication, and acknowledgement now accept a directory link only when it was observed as a skill-root directory and still resolves to the snapshot's physical path. An ordinary child-link replacement regression verifies that application publishes a real directory and preserves the old link target. Historical evidence no longer presents older full-suite totals as current counts.

The ninth review found three security boundary failures. Git origin inspection now rejects credential-bearing URL userinfo and query strings before returning a reference that could be transmitted or persisted; ordinary SSH account names remain supported. Transfer authorization rechecks each path against its skill's physical boundary, including paths present only in synchronization history. Link publication resolves existing target components and rejects destinations outside that same boundary. Regressions cover a directory symlink redirected to an unrelated home path and a credential-bearing Git origin that never appears in an error or reference.

The next review identified encoded SSH userinfo and questioned two coordinator policies. Origin validation now rejects encoded userinfo before a Git reference can enter history. An end-to-end first-contact Git test confirms that a tracked file's committed comparison base does not authorize `--missing remove`; the coordinator's `prior_presence` uses only matching peer history. Existing directory/file conflict integration covers complete subtree selection under both explicit policies. Physical alias groups represent the same entry across names, so different root and child values do not compete in one group; comments make that distinction explicit.

The next clean review still flagged acquisition aliases in its review body. Snapshot now records direct-child Library symlinks and their home-relative physical targets. The coordinator synchronizes the registered physical skill once, registers its locations, then atomically creates the corresponding alias on hosts where it is absent. Existing occupied alias paths stay untouched and produce a blocked diagnostic; targets without a registered skill also produce an explicit diagnostic. The existing alias-convergence integration now asserts that the receiving host has a symlink to its own home-relative target on the first run.

The following review found a possible directory-symlink escape during collection. Each observed directory is now canonicalized and checked against the physical skill boundary before recursion, including a link that changes after discovery. A focused regression permits a link to a sibling directory within the skill and rejects a link to unrelated home content. The coordinator also excludes acquisition alias paths from file transfer when their targets are unregistered, reports the unavailable target, and leaves the receiver untouched. Interrupted alias staging names are ignored by library discovery.

The next review found that a conflicting acquisition alias could still be published using the first host's target. Alias mappings with differing targets now remain excluded from publication on every host while their paths stay excluded from ordinary file transfer. A three-host regression confirms both existing links remain intact, the third host receives no arbitrary link, and the conflict is reported.

The following review found two interrupted-operation paths. Library discovery now excludes all three reserved staging names (`.skillator-alias-`, `.skillator-rsync-`, and `.skillator-clone-` with valid identifiers); a scanner regression checks that staged directories containing skill manifests remain invisible. The SSH stderr worker now drains to the sink until EOF, even beyond the protocol output limit, so a chatty remote cannot fill the pipe and stall the session. A focused regression consumes more than that limit.

After integration with current `main`, further review found three selection and grouping issues. Snapshot protocol 3 carries invalid-skill paths separately from the full transfer inventory, preventing new Enablements for invalid skills while retaining existing selections. A failed Location registration now blocks dependent new Enablements. Physical grouping excludes internal symbolic links so a link and its target remain distinct entries. Regressions cover each behavior, including an injected registration failure and a file link followed by a remote edit. A separate alias-publication regression rejects administrative and reserved staging destinations before creating a link.

The next review identified two pre-transfer gaps. After Begin, the coordinator now refreshes all participants and checks initiating and existing Git references before any missing checkout is cloned. The existing commit-advance regression now asserts that the receiving checkout remains absent when the initiating commit moves during Begin. Begin responses must return a reserved staging directory directly beneath the observed home; a fault-injected outside path is rejected before any transfer.

The following review found two more selection and identity edges. Root-level invalid skills now compare the domain path `.` with the snapshot's empty relative path before propagating Enablements; a regression preserves the local selection without adding it remotely. A former receiver now checks selected participant IDs against its acknowledged peer histories when the selected cohort is no larger than the prior cohort. This detects a replaced or missing former initiator before mutation even when local SSH aliases differ. Regressions cover the replacement and a legitimate role reversal with the original peer.

The same review found that unresolved selections could travel to a new host even when no participant had their source or skill. Enablement reconciliation now requires the selected skill in a usable observed source before propagating it, reports an unavailable selection when hosts differ, and preserves each existing local entry. A two-case regression covers a missing source and a missing skill within a present source.

The next review found a stage-root substitution risk. Session stages now retain their creation identity and require physical revalidation before and after rsync pull/push, export, publication, and cleanup; the private wire protocol is version 4. One regression replaces the stage with both a symlink outside the home and a different same-path directory, then checks that export, publication, and cleanup reject it without writing outside. A transfer-level regression confirms push rejects a replaced stage before starting rsync. Alias publication also rechecks its physical destination and target around staging, and the playbook lists the reserved `.skillator-alias-` prefix.

The subsequent review found two cross-workflow boundaries. Library Configuration writes and `library update` now acquire the user-home lock already held by rsync during its apply phase. A contention regression confirms both writers return the existing busy status without changing configuration. Source observation excludes nested Git checkout roots from parent skill files, committed comparison content, and historical paths; transfer authorization also refuses to treat a nested path as its parent's. A nested-checkout regression confirms the parent owns none of the nested files while the nested source retains its skill content under its own Git reference.

The next clean inline review still noted selection availability in its overview. New Enablements now require their manifest to be acknowledged on every receiving participant after file reconciliation, or planned for copying in read-only preview. When `--missing ignore` leaves a host without the skill, the selection remains on its original host and the receiving host gets an unavailable-source diagnostic. A regression checks that preview still reports the coupled skill and selection changes, ignore leaves both absent remotely, and a later default-copy run propagates both.

The following review questioned coupled directory removal and identified a clone-path race. The coupled member set already included its `directory/<key>` entry through `user_directory`; it now names that entry explicitly. A regression removes a directory and its Enablement, verifies both hosts and acknowledged baselines record the removal, and checks an unchanged retry. Clone staging now opens the newly created directory without following symlinks and runs Git with its current directory bound to that open descriptor. Stage identity and home containment are rechecked before subsequent Git operations, publication, and cleanup. A redirected-parent regression confirms a descriptor-bound subprocess writes to the originally opened directory rather than through the new symlink.

The latest review identified a publication-parent race. Publication now opens each physical parent component beneath home without following symlinks and holds the final directory inode. Sibling creation, content copy, rename, verification, and cleanup operate relative to that handle, so replacing an ancestor with a link cannot redirect a write outside home. The logical destination is checked again before and after the rename. `opened_parent_keeps_publication_inside_home_after_ancestor_swap` moves an opened ancestor, replaces it with a link to an external directory, and verifies publication stays in the original home directory while a new traversal rejects the redirected path.

The next review accepted publication containment and found the corresponding race in stage creation and related alias/recovery operations. Session lock and stage creation, clone and alias staging, stage export and journal writes, stage cleanup, and recovery rename/backup cleanup now use opened, non-following parent directories. Recovery discovery and journal removal also use directory handles. `stage_creation_and_cleanup_follow_the_opened_parent` redirects `.skillator` after its parent is opened and verifies stage writes and recursive cleanup remain with the original home directory, leaving the external target empty.

The following review accepted stage containment and reported four further findings. Three are fixed: divergent usable file or selection acknowledgements now block the affected path instead of selecting the first differing baseline; source files are opened through a held parent descriptor with `O_NOFOLLOW` before copying; and synchronization history uses descriptor-relative conditional staging, rename, verification, and cleanup instead of path-based configuration saving. Regressions cover divergent three-host histories, a source replaced by an external symlink, and a history path linked outside home. The first-contact selection claim does not apply: whole-directory decisions occur only after an acknowledged directory removal; independent initial enablements are processed key-by-key, and `first_contact_independent_user_selections_form_a_union` verifies the resulting union. A code comment makes that branch condition explicit.

The next review accepted those resolutions and identified two remaining edges. An existing session lock now opens through a non-following parent handle and rejects a final symlink; a regression verifies an outside file cannot become the lock. History saving now verifies the live entry after exchange and before deleting the staged prior version. A deterministic intervening-edit regression confirms it reports an error, preserves the newer live bytes, and retains the prior history for recovery.

The subsequent review accepted both fixes and found three new cases. A common baseline now requires every selected peer to supply a matching history value. A three-host regression establishes two synchronized peers, introduces an independent first-contact edit on the third, and verifies the default policy reports a conflict without replacing either version. Library and User Scope configuration are read through non-following directory handles before snapshot serialization; an outside-linked final file is rejected. Incoming Library registration uses descriptor-relative conditional publication, with a regression confirming an outside-linked configuration cannot be overwritten.
