## ADDED Requirements

### Requirement: Library update is a non-interactive command

`skillator library update` SHALL work outside a Target and without a terminal. It SHALL accept `--check`, `--format <text|json|yaml>` defaulting to text, and `--timeout <seconds>` defaulting to 30. Timeout SHALL accept positive whole seconds; invalid, zero, negative, fractional, and out-of-range values SHALL return 2 before mutation. Positional selectors and `--force` SHALL be rejected with exit 2. Git SHALL NOT prompt for credentials, editor input, or confirmation; authentication requiring interaction SHALL fail with a report. Raw subprocess output MUST NOT bypass the renderer.

#### Scenario: Non-TTY invocation
1. **WHEN** update runs outside Git with redirected streams
2. **THEN** it updates the configured Library without TUI or Target requirements

#### Scenario: Invalid options
1. **WHEN** a selector, force, invalid format, or invalid timeout is supplied
2. **THEN** parsing returns 2 and no pull starts

#### Scenario: Authentication failure
1. **WHEN** authentication requires interaction
2. **THEN** the pull fails without prompting and independent work continues

#### Scenario: Conflicting SSH batch settings
1. **WHEN** the configured SSH executable arguments disable batch mode or permit password prompts
2. **THEN** update enforces batch mode and zero password prompts while preserving the executable and other arguments

### Requirement: Library update preview inspects only local state

Preview SHALL use normal discovery and local eligibility checks without fetching, contacting remotes, refreshing the Git index, or writing files or metadata. Eligible checkouts SHALL have action `pull` and outcome `would_apply`. Text SHALL say "Would attempt pull; remote state not checked." Machine output SHALL include advisory `remote_state_not_checked`. Cached tracking refs MUST NOT be presented as proof that a repository is current. Preview SHALL return 1 for any planned pull or blocking problem, and 0 for an empty successful plan.

#### Scenario: Local-only preview
1. **WHEN** an eligible checkout is previewed, even with HEAD matching cached tracking refs
2. **THEN** the report describes an attempted pull with unknown remote state, returns 1, and leaves files, metadata, and remotes untouched

### Requirement: Library update reports aggregate and per-checkout results

The command SHALL use the existing version 1 report envelope and machine-format rules. Mode SHALL be `library_update` or `library_update_check`; target SHALL be the Library configuration path. Each selected checkout SHALL have 1 canonical-path-ordered change row with action `pull`, safety `safe` or `blocked`, and outcome `applied`, `unchanged`, `would_apply`, `blocked`, or `failed`. Diagnostics SHALL have stable reason codes and affected paths or Locations. Skipped submodules SHALL appear only in diagnostics. Text SHALL show updates, preview attempts, skips, and problems. When stdout is a terminal, text SHALL list every unchanged repository with the label `up-to-date`, including in mixed-result batches. Redirected text SHALL retain a concise unchanged count; an empty plan SHALL say `No repositories to update.`

Completed reports SHALL use stdout, including partial reports. Machine output SHALL be deterministic and ANSI-free without raw Git progress. Application SHALL return 0 for complete success, 1 for trustworthy partial or blocked results, 3 for invalid or unreadable configuration before any pull, and 5 for missing Git or other fatal pre-report failure. Pre-report errors SHALL leave stdout empty and use stderr. Invalid Skill metadata, Source Key collisions, and skipped submodules alone SHALL NOT cause exit 1.

#### Scenario: Partial machine report
1. **WHEN** 1 checkout updates, 1 is unchanged, and 1 fails
2. **THEN** JSON and YAML encode equivalent complete reports with exit 1

#### Scenario: Unchanged repositories in an interactive terminal
1. **WHEN** update evaluates repositories that need no changes and stdout is a terminal
2. **THEN** text lists each unchanged repository path with the label `up-to-date`

#### Scenario: Redirected text remains compact
1. **WHEN** stdout is redirected and evaluated repositories need no changes
2. **THEN** text reports the unchanged count without listing unchanged paths

#### Scenario: Invalid configuration
1. **WHEN** Library configuration is invalid
2. **THEN** no pull runs and stderr contains the diagnostic with exit 3 and empty stdout

### Requirement: Library pulls have a timeout and support batch cancellation

Timeout SHALL measure elapsed time from pull subprocess start through completion, including transport and hooks. On expiry, Skillator SHALL stop the pull and subprocess group with bounded cleanup, reap the child without hanging on output pipes, report `failed` with `pull_timeout` and the path and configured limit, and continue. It MUST NOT retry or roll back automatically. Preview SHALL start no pull or pull timer.

Ctrl+C SHALL stop the active pull and its subprocess group and prevent subsequent pulls. Skillator SHALL preserve completed outcomes, mark the active interrupted pull failed and unattempted selected pulls blocked with cancellation diagnostics, emit a partial selected-format report, and exit 130. Cancellation SHALL be an explicit exception to normal completed-report exit statuses.

#### Scenario: Default timeout
1. **WHEN** a pull exceeds 30 seconds without an override
2. **THEN** it and its subprocesses stop, the report contains `pull_timeout`, remaining repositories run, and the completed batch exits 1

#### Scenario: Timeout override
1. **WHEN** `--timeout 60` is supplied
2. **THEN** every attempted pull has a separate 60-second limit

#### Scenario: Cancellation
1. **WHEN** Ctrl+C arrives during a pull after an earlier success
2. **THEN** no later pulls start, the earlier update remains, and a partial report is emitted with exit 130
