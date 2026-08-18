## 1. Project Foundation

- [ ] 1.1 Add the minimal pinned runtime and test dependencies for CLI parsing, TUI rendering, serialization, YAML handling, filesystem locking, terminal detection, and temporary test repositories.
- [ ] 1.2 Convert the package to a library plus thin binary and create the agreed `domain`, `config`, `git`, `library`, `target`, private `materialization`, `reconcile`, `app`, `cli`, and `tui` module boundaries.
- [ ] 1.3 Add isolated test-home, temporary filesystem, and real temporary Git repository fixtures that never read or write the developer's actual configuration.
- [ ] 1.4 Add cross-platform CI jobs for macOS and Linux and document the WSL acceptance run against its Linux filesystem.

## 2. Validated Domain and Configuration

- [ ] 2.1 Implement validated Source Key, Skill Key, Skill Directory Key, repository-relative path, Materialization kind, and Enablement value types with canonicality and relationship tests.
- [ ] 2.2 Implement strict version 1 Repository Configuration parsing and collected validation, including unsupported-version byte preservation and malformed-input tests.
- [ ] 2.3 Implement deterministic canonical Repository Configuration serialization with golden fixtures for ordering, quoting, and final-newline behavior.
- [ ] 2.4 Implement strict version 1 Library Configuration parsing and validation for Locations, exclusions, registered Sources, and registered Skills.
- [ ] 2.5 Implement deterministic canonical Library Configuration serialization and first-run default construction.
- [ ] 2.6 Implement configuration fingerprints, stale-write refusal, sibling staging, conditional replacement, and cleanup tests for absent and existing documents.
- [ ] 2.7 Prove the selected YAML library rejects duplicate keys, aliases, tags, merge keys, and multiple documents and can emit the constrained machine YAML contract without `serde_yaml`.

## 3. Git and Library Inventory

- [ ] 3.1 Implement Git worktree-root resolution and structured facts for repository boundaries, worktrees, submodules, remotes, tracked paths, staged or unmerged state, and effective ignore rules.
- [ ] 3.2 Implement Library Location expression expansion, canonical resolution, availability diagnostics, Gitignore-style exclusions, and overlap detection with explicit override state.
- [ ] 3.3 Implement recursive discovery that prunes `.git`, does not follow directory symlinks, separates nested Git repositories into Sources, and assigns outside content to the local Source.
- [ ] 3.4 Implement Agent Skill metadata parsing and validation, including Source-root Skills, invalid metadata diagnostics, and exact frontmatter-name checks.
- [ ] 3.5 Implement Source Key suggestion and collision handling for Git and local Sources without changing persisted identity after path or remote changes.
- [ ] 3.6 Implement Source and Skill registration state, moved or missing content continuity, and portable Skill resolution in immutable `LibrarySnapshot` results.
- [ ] 3.7 Add Library fixture tests for first run, mixed Locations and Sources, registration, invalid Skills, unavailability, movement, exclusions, and non-interactive advisory discovery.

## 4. Target Selection and Observation

- [ ] 4.1 Implement Target selection from an existing directory with canonical supplied-path reporting, Git worktree-root resolution, and rejection of files, bare repositories, missing paths, and non-Git directories.
- [ ] 4.2 Implement Skill Directory root and immediate-child inspection without initial symlink traversal, including control-file, Git, case, inaccessible, and scan-instability facts.
- [ ] 4.3 Implement Expected Entry derivation for nested and Source-root Skills plus collision detection and unknown-name handling for unresolved root Skills.
- [ ] 4.4 Implement Linked observation for canonical, noncanonical, broken, misdirected, invalid, and wrong-kind entries.
- [ ] 4.5 Implement the physical copied-tree walker and Copy-Ineligible validation for `.git`, internal symlinks, unsupported kinds, filename behavior, and executable state.
- [ ] 4.6 Implement copied-tree equivalence and Diverged Copy detection using names, kinds, bytes, symlink text, and executable state while ignoring incidental metadata.
- [ ] 4.7 Implement per-Enablement and per-directory Drifted, Unverifiable, and In Sync aggregation plus Unmanaged, Duplicate, compatibility, and Git exclusion diagnostics.
- [ ] 4.8 Add observation fixtures covering every representative root, link, copy, unresolved, unmanaged, duplicate, collision, case, control-file, and unstable-scan state.

## 5. Reconciliation Planning and Execution

- [ ] 5.1 Implement the pure reconciliation planner and table-driven Safe, Guarded, Blocked, and No Change classification tests.
- [ ] 5.2 Implement canonical Skill Directory Control File planning, effective-ignore verification, index-read-only protection, and exact Git remediation arguments.
- [ ] 5.3 Implement the exclusive Target mutation lock and non-cloneable prepared-plan capability, including active-owner, cancellation, drop, and check-mode behavior.
- [ ] 5.4 Implement reserved Recovery Artifact parsing and deterministic safe restore, abandoned-stage cleanup, and ambiguous-recovery blocking.
- [ ] 5.5 Implement sibling staging and complete validation for canonical absolute links and self-contained copied candidates.
- [ ] 5.6 Implement publication, type-changing backup sequences, removals-last ordering, rollback, and process-local backup cleanup.
- [ ] 5.7 Implement apply-time Source and destination revalidation without silent replanning, including changed-during-copy behavior.
- [ ] 5.8 Implement the executor's independent partial-apply outcomes, failure isolation, Recovery Required retention, and mandatory fresh final observation.
- [ ] 5.9 Add private fault-injection tests for staging failure, publication failure, successful rollback, failed rollback, backup deletion failure, and independent-operation continuation.
- [ ] 5.10 Add convergence and idempotence tests proving repeated save, sync, and check perform no unnecessary writes.

## 6. Application Workflows

- [ ] 6.1 Implement `LibraryWorkflow` loading, first-run staging, registration edits, affected-reference warnings, fingerprint checks, and confirmed Library save.
- [ ] 6.2 Implement `TargetWorkflow` loading, typed staged edits, complete plan preparation, guarded authorization, configuration-first save, and partial-result handling.
- [ ] 6.3 Implement `SyncWorkflow` for check, Safe-only apply, and all-Guarded force authorization without configuration or registration writes.
- [ ] 6.4 Add end-to-end workflow tests for stale documents, invalid or unsupported configuration, absent Library, unresolved Enablements, busy Targets, partial apply, and successful convergence.

## 7. Command-Line Interface and Reports

- [ ] 7.1 Implement the exact clap command tree, positional Target selection, TTY validation, sync option conflicts, help, and version behavior.
- [ ] 7.2 Implement concise text reporting with color policy and stdout/stderr separation.
- [ ] 7.3 Implement the compact semantic report DTO and deterministic JSON rendering.
- [ ] 7.4 Implement constrained YAML rendering and golden tests that deserialize JSON and YAML fixtures into the same logical values.
- [ ] 7.5 Implement stable process exit mapping for success, non-convergence, parser failure, invalid input, Target Busy, and fatal pre-report failure.
- [ ] 7.6 Add CLI integration tests for every command and option, no-write check mode, machine-output integrity, absent Repository Configuration, and force boundaries.

## 8. Terminal User Interface

- [ ] 8.1 Implement the pure TUI Model, Action, Effect reducer and terminal lifecycle with restoration on every exit path.
- [ ] 8.2 Implement the Library hierarchy table, Source and Skill registration controls, expansion state, contextual inspector, first-run staging, and save review.
- [ ] 8.3 Implement the Target Skill Directory strip and one-directory hierarchy table with Source rollups, Enabled, Mode, Skill, Description, State, and directory diagnostics.
- [ ] 8.4 Implement the complete Vim-style navigation, Source group movement, collapse and expand, filters, bulk selection, mode switching, and help overlay.
- [ ] 8.5 Implement validated Skill Directory add, edit, and delete overlays with Generic/Codex, Claude, and custom choices.
- [ ] 8.6 Implement Target switching and `Ctrl+L` workspace toggling with discard-or-return handling for staged edits.
- [ ] 8.7 Implement confirmed `s` save, Safe-only `Ctrl+S` fast save, Guarded batch confirmation, Target Busy retry choice, and partial-result acknowledgement.
- [ ] 8.8 Add reducer transition tests, rendered-screen tests for representative states, and read-only invalid or unsupported configuration screens.

## 9. MVP Acceptance

- [ ] 9.1 Build the shared multi-Location, multi-Source, multi-Skill acceptance fixture with valid, Invalid, Unavailable, Linked, Copied, Copy-Ineligible, unmanaged, and recovery states.
- [ ] 9.2 Automate the twenty Wayfinder acceptance journeys across Library, Target, observation, reconciliation, recovery, Git protection, CLI, and portability behavior.
- [ ] 9.3 Run the core acceptance suite on macOS, Linux, and WSL's Linux filesystem and verify capability failures on mounted filesystems preserve content without fallback.
- [ ] 9.4 Perform the short manual terminal smoke check for table readability, navigation feel, confirmations, result acknowledgement, and terminal restoration.
- [ ] 9.5 Verify deferred features have no placeholder commands or accidental interfaces and validate the complete OpenSpec change in strict mode.
