## 1. Project Foundation

- [x] 1.1 Add minimal semver-compatible runtime and test dependencies plus an application lockfile for CLI parsing, TUI rendering, serialization, YAML handling, filesystem locking, terminal detection, and temporary test repositories.
- [x] 1.2 Convert the package to a library plus thin binary and create the agreed `domain`, `config`, `git`, `library`, `target`, private `materialization`, `reconcile`, `app`, `cli`, and `tui` module boundaries.
- [x] 1.3 Add isolated test-home, temporary filesystem, and real temporary Git repository fixtures that never read or write the developer's actual configuration.
- [x] 1.4 Add cross-platform CI jobs for macOS and Linux and document the WSL acceptance run against its Linux filesystem.

## 2. Validated Domain and Configuration

- [x] 2.1 Implement validated Source Key, Skill Key, Skill Directory Key, repository-relative path, Materialization kind, and Enablement value types with canonicality and relationship tests.
- [x] 2.2 Implement strict version 1 Repository Configuration parsing and collected validation, including unsupported-version byte preservation and malformed-input tests.
- [x] 2.3 Implement deterministic canonical Repository Configuration serialization with golden fixtures for ordering, quoting, and final-newline behavior.
- [x] 2.4 Implement strict version 1 Library Configuration parsing and validation for Locations, exclusions, registered Sources, and registered Skills.
- [x] 2.5 Implement deterministic canonical Library Configuration serialization and first-run default construction.
- [x] 2.6 Implement configuration fingerprints, stale-write refusal, sibling staging, conditional replacement, and cleanup tests for absent and existing documents.
- [x] 2.7 Prove the selected YAML library rejects duplicate keys, aliases, tags, merge keys, and multiple documents and can emit the constrained machine YAML contract without `serde_yaml`.

## 3. Git and Library Inventory

- [x] 3.1 Implement Git worktree-root resolution and structured facts for repository boundaries, worktrees, submodules, remotes, tracked paths, staged or unmerged state, and effective ignore rules.
- [x] 3.2 Implement Library Location expression expansion, canonical resolution, availability diagnostics, Gitignore-style exclusions, and overlap detection with explicit override state.
- [x] 3.3 Implement recursive discovery that prunes `.git`, does not follow directory symlinks, separates nested Git repositories into Sources, and assigns outside content to the local Source.
- [x] 3.4 Implement Agent Skill metadata parsing and validation, including Source-root Skills, invalid metadata diagnostics, and exact frontmatter-name checks.
- [x] 3.5 Implement Source Key suggestion and collision handling for Git and local Sources without changing persisted identity after path or remote changes.
- [x] 3.6 Implement Source and Skill registration state, moved or missing content continuity, and portable Skill resolution in immutable `LibrarySnapshot` results.
- [x] 3.7 Add Library fixture tests for first run, mixed Locations and Sources, registration, invalid Skills, unavailability, movement, exclusions, and non-interactive advisory discovery.

## 4. Target Selection and Observation

- [x] 4.1 Implement Target selection from an existing directory with canonical supplied-path reporting, Git worktree-root resolution, and rejection of files, bare repositories, missing paths, and non-Git directories.
- [x] 4.2 Implement Skill Directory root and immediate-child inspection without initial symlink traversal, including control-file, Git, case, inaccessible, and scan-instability facts.
- [x] 4.3 Implement Expected Entry derivation for nested and Source-root Skills plus collision detection and unknown-name handling for unresolved root Skills.
- [x] 4.4 Implement Linked observation for canonical, noncanonical, broken, misdirected, invalid, and wrong-kind entries.
- [x] 4.5 Implement the physical copied-tree walker and Copy-Ineligible validation for `.git`, internal symlinks, unsupported kinds, filename behavior, and executable state.
- [x] 4.6 Implement copied-tree equivalence and Diverged Copy detection using names, kinds, bytes, symlink text, and executable state while ignoring incidental metadata.
- [x] 4.7 Implement per-Enablement and per-directory Drifted, Unverifiable, and In Sync aggregation plus Unmanaged, Duplicate, compatibility, and Git exclusion diagnostics.
- [x] 4.8 Add observation fixtures covering every representative root, link, copy, unresolved, unmanaged, duplicate, collision, case, control-file, and unstable-scan state.

## 5. Reconciliation Planning and Execution

- [x] 5.1 Implement the pure reconciliation planner and table-driven Safe, Guarded, Blocked, and No Change classification tests.
- [x] 5.2 Implement canonical Skill Directory Control File planning, effective-ignore verification, index-read-only protection, and exact Git remediation arguments.
- [x] 5.3 Implement the exclusive Target mutation lock and non-cloneable prepared-plan capability, including active-owner, cancellation, drop, and check-mode behavior.
- [x] 5.4 Implement reserved Recovery Artifact parsing and deterministic safe restore, abandoned-stage cleanup, and ambiguous-recovery blocking.
- [x] 5.5 Implement sibling staging and complete validation for canonical absolute links and self-contained copied candidates.
- [x] 5.6 Implement publication, type-changing backup sequences, removals-last ordering, rollback, and process-local backup cleanup.
- [x] 5.7 Implement apply-time Source and destination revalidation without silent replanning, including changed-during-copy behavior.
- [x] 5.8 Implement the executor's independent partial-apply outcomes, failure isolation, Recovery Required retention, and mandatory fresh final observation.
- [x] 5.9 Add private fault-injection tests for staging failure, publication failure, successful rollback, failed rollback, backup deletion failure, and independent-operation continuation.
- [x] 5.10 Add convergence and idempotence tests proving repeated save, sync, and check perform no unnecessary writes.

## 6. Application Workflows

- [x] 6.1 Implement `LibraryWorkflow` loading, first-run staging, registration edits, affected-reference warnings, fingerprint checks, and confirmed Library save.
- [x] 6.2 Implement `TargetWorkflow` loading, typed staged edits, complete plan preparation, guarded authorization, configuration-first save, and partial-result handling.
- [x] 6.3 Implement `SyncWorkflow` for check, Safe-only apply, and all-Guarded force authorization without configuration or registration writes.
- [x] 6.4 Add end-to-end workflow tests for stale documents, invalid or unsupported configuration, absent Library, unresolved Enablements, busy Targets, partial apply, and successful convergence.

## 7. Command-Line Interface and Reports

- [x] 7.1 Implement the exact clap command tree, positional Target selection, TTY validation, sync option conflicts, help, and version behavior.
- [x] 7.2 Implement concise text reporting with color policy and stdout/stderr separation.
- [x] 7.3 Implement the compact semantic report DTO and deterministic JSON rendering.
- [x] 7.4 Implement constrained YAML rendering and golden tests that deserialize JSON and YAML fixtures into the same logical values.
- [x] 7.5 Implement stable process exit mapping for success, non-convergence, parser failure, invalid input, Target Busy, and fatal pre-report failure.
- [x] 7.6 Add CLI integration tests for every command and option, no-write check mode, machine-output integrity, absent Repository Configuration, and force boundaries.

## 8. Terminal User Interface

- [x] 8.1 Implement the pure TUI Model, Action, Effect reducer and terminal lifecycle with restoration on every exit path.
- [x] 8.2 Implement the Library hierarchy table, Source and Skill registration controls, expansion state, contextual inspector, first-run staging, and save review.
- [x] 8.3 Implement the Target Skill Directory strip and one-directory hierarchy table with Source rollups, checkbox, Mode, Skill, Description, Action, inspector state, and directory diagnostics.
- [x] 8.4 Implement the complete Vim-style navigation, Source group movement, collapse and expand, filters, bulk selection, mode switching, and help overlay.
- [x] 8.5 Implement validated Skill Directory add, edit, and delete overlays with Generic/Codex, Claude, and custom choices.
- [x] 8.6 Implement Target switching and `Ctrl+L` workspace toggling with discard-or-return handling for staged edits.
- [x] 8.7 Implement confirmed `s` save, Safe-only `Ctrl+S` fast save, Guarded batch confirmation, Target Busy retry choice, and partial-result acknowledgement.
- [x] 8.8 Add reducer transition tests, rendered-screen tests for representative states, and read-only invalid or unsupported configuration screens.

## 9. MVP Acceptance

- [x] 9.1 Build the shared multi-Location, multi-Source, multi-Skill acceptance fixture with valid, Invalid, Unavailable, Linked, Copied, Copy-Ineligible, unmanaged, and recovery states.
- [x] 9.2 Automate the twenty Wayfinder acceptance journeys across Library, Target, observation, reconciliation, recovery, Git protection, CLI, and portability behavior.
- [ ] 9.3 Run the core acceptance suite on macOS, Linux, and WSL's Linux filesystem and verify capability failures on mounted filesystems preserve content without fallback.
- [x] 9.4 Perform the short manual terminal smoke check for table readability, navigation feel, confirmations, result acknowledgement, and terminal restoration.
- [x] 9.5 Verify deferred features have no placeholder commands or accidental interfaces and validate the complete OpenSpec change in strict mode.

## 10. First-run Onboarding and User Scope

- [x] 10.1 Add User Scope terminology and strict `~/.agents/skillator.yaml` loading, validation, canonical serialization, fingerprints, and first-run defaults using home-relative Skill Directory paths.
- [x] 10.2 Implement User Scope observation, planning, locking, and reconciliation without Repository Git control-file or index behavior.
- [x] 10.3 Implement read-only onboarding inventory for physical Skills, existing Skill symlinks, invalid entries, inferred Git or local Sources, and collision diagnostics under `~/.agents/skills`.
- [x] 10.4 Implement the staged first-Library path prompt with editable `./library` default and a complete import, registration, move, and relink review.
- [x] 10.5 Implement transactional onboarding publication and rollback across Library content, `~/.skillator/library.yaml`, `~/.agents/skillator.yaml`, and user-scoped links, including deterministic fault-injection coverage.
- [x] 10.6 Route missing Library Configuration into onboarding and open the current Target's first Repository tab after success without showing expected initialization as a warning.
- [x] 10.7 Integrate User Scope Skill Directories as the first Target tabs, keep User and Repository staged saves separate, and support additional `User · <label>` tabs.
- [x] 10.8 Project inherited User Enablements into Repository tables as read-only `[u] user`, preserve explicit Repository modes, and warn on simultaneous User and Repository activation.
- [x] 10.9 Add reducer, rendered-screen, workflow, rollback, collision, existing-symlink, and end-to-end first-run acceptance tests.
- [x] 10.10 Re-run macOS and Linux validation, document pending native WSL verification, and validate the amended OpenSpec change in strict mode.
  - macOS and a read-only mounted `rust:latest` Linux container pass the complete 131-test suite; native WSL and mounted-filesystem capability checks remain tracked by 9.3.

## 11. Library Acquisition and Action-focused Tables

- [x] 11.1 Separate metadata Description, displayed Mode, observed inspector details, and pending Save Action in the TUI row model; use unlabeled checkbox, `Mode`, and final `Action` headers.
- [x] 11.2 Add `move`, `copy`, `link`, and blank in-place Library acquisition modes with `move` as the default for newly selected external Skills and `m` cycling modes.
- [x] 11.3 Implement collision-safe transactional acquisition into the first Location's `local/library` Source and integrate resulting registration changes.
- [x] 11.4 Apply move/copy/link choices to first-run onboarding while preserving the agreed User Scope materialization result.
- [x] 11.5 Add `/pending` and `/pending actions` filtering plus reducer, rendered-table, acquisition, collision, and rollback tests.
- [x] 11.6 Run formatting, strict Clippy, macOS and Linux suites, strict OpenSpec validation, and record native WSL as the remaining platform task.

## 12. TUI Copy and Save Affordance

- [x] 12.1 Populate onboarding Skill descriptions from `SKILL.md` frontmatter instead of filesystem-kind prose.
- [x] 12.2 Shorten Library and onboarding Action values to one-to-three-word phrases.
- [x] 12.3 Show Save and Exit keys persistently in the Library footer and add focused regression coverage.

## 13. Arrow-key Navigation

- [x] 13.1 Map plain and Shift-modified arrow keys to the equivalent `h/j/k/l` navigation actions while leaving Ctrl-modified arrows unmapped.
- [x] 13.2 Add exact key-event regression coverage and rerun TUI and strict validation suites.

## 14. First-run Location Affordance

- [x] 14.1 Start onboarding in the normal table with the staged `./library` Location selected instead of automatically opening the Location editor.
- [x] 14.2 Advertise the explicit `e` edit action and the editor's Enter/Escape controls, with startup-model and rendered-screen regression coverage.

## 15. TUI 256-color Palette

- [x] 15.1 Centralize the indexed palette and apply purple workspace borders, blue modal borders, bone titles and hotkeys, yellow warnings, red errors, dim structural glyphs, and dark-blue background-only selection.
- [x] 15.2 Add rendered-style regression coverage and rerun TUI, Rust, and strict OpenSpec validation.

## 16. TUI Footer and Header Refinement

- [x] 16.1 Brighten structural gray, move the right-aligned action legend into the main bottom border without a second rule, and label the Library hierarchy column `Location`.
- [x] 16.2 Add exact rendered-buffer regression coverage and rerun TUI, Rust, and strict OpenSpec validation.

## 17. Confirmation Semantics and Clean Paths

- [x] 17.1 Remove redundant current-directory components from resolved Library paths and render confirmation questions and hotkeys separately from normal desired-action rows.
- [x] 17.2 Add path and styled-confirmation regressions and rerun TUI, Rust, and strict OpenSpec validation.

## 18. Modal Titles and Confirmation Borders

- [x] 18.1 Give every modal a padded capitalized purpose title and move confirmation controls from modal bodies into bottom borders.
- [x] 18.2 Add rendered modal-chrome regressions and rerun TUI, Rust, and strict OpenSpec validation.

## 19. First Repository Diagnostic Noise

- [x] 19.1 Stop classifying an absent first-save Skill Directory and control file as a diagnostic while preserving the Safe creation plan and genuine existing-directory diagnostics.
- [x] 19.2 Add a first-save workflow regression and rerun TUI, Rust, and strict OpenSpec validation.

## 20. Action-driven Row Color

- [x] 20.1 Derive ordinary entry warning color only from non-empty Action and structured row classification, never from free-form Skill metadata or inspector text.
- [x] 20.2 Add rendered regressions for status-like description words and pending Actions, then rerun TUI, Rust, and strict OpenSpec validation.

## 21. Target-first Launch

- [x] 21.1 Select the first Repository Skill Directory when root Skillator opens a Git Target, while retaining User tabs at the start of the strip and falling back to User only when no Repository tab exists.
- [x] 21.2 Add launch-selection regression coverage and rerun TUI, Rust, and strict OpenSpec validation.

## 22. Untouched First-run Tab Switching

- [x] 22.1 Keep implicit first-run User and Repository defaults save-ready without classifying them as staged edits for tab navigation.
- [x] 22.2 Add an untouched-startup scope-switch regression and rerun TUI, Rust, and strict OpenSpec validation.
