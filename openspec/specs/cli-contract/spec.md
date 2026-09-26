# cli-contract Specification

## Purpose
Defines Skillator's small public command surface, non-interactive synchronization behavior, compact machine-readable output, and stable process outcomes for scripts.

## Requirements

### Requirement: The CLI exposes interactive defaults and explicit command groups
Skillator SHALL expose top-level `init`, `library`, `target`, `targets`, `user`, `sync`, and `hook` commands. It SHALL NOT expose a top-level `worktree` command. Unqualified `skillator [DIRECTORY]` SHALL launch the Target TUI, and unqualified `skillator library` SHALL launch the Library TUI. `init`, `library add`, `library remove`, `library prune`, `library locations`, `library list`, `library rsync`, `library update`, `target list`, `target link`, `target copy`, `target remove`, `targets list`, `targets remove`, `targets prune`, `user list`, `user link`, `user copy`, and `user remove` SHALL run without an interactive terminal. `skillator sync target [directory]` SHALL run ordinary Target reconciliation. `skillator sync worktree [directory]` SHALL project primary-worktree Target state into a linked worktree. Both explicit sync forms SHALL default the directory to `.`. The non-interactive `skillator hook install`, `skillator hook status`, and `skillator hook uninstall` commands SHALL manage the repository-local Git integration. The MVP MUST NOT expose aliases or command-line registration CRUD.

#### Scenario: Default root invocation

- **WHEN** the user runs `skillator` in a Git worktree with interactive input and output
- **THEN** Skillator opens the normal Library workspace with its first-run welcome when Library Configuration is absent, otherwise launches the Target TUI for the current worktree root

#### Scenario: Bare Library invocation
- **WHEN** the user runs `skillator library` with interactive input and output
- **THEN** Skillator launches the Library TUI without requiring a Target Repository

#### Scenario: Library invocation outside Git
- **WHEN** the user runs `skillator library` from a non-Git directory with interactive input and output
- **THEN** Skillator launches the Library workspace without requiring a Target

#### Scenario: Library subcommand without a terminal
- **WHEN** the user runs `skillator library list elastic` without interactive input or output
- **THEN** Skillator lists matching live Library inventory without opening the TUI

#### Scenario: Explicit synchronization commands
- **WHEN** the user runs `skillator sync target` or `skillator sync worktree`
- **THEN** Skillator runs only the requested workflow against `.` and emits the selected report format

#### Scenario: Bare sync discovers a linked worktree
- **WHEN** the user runs `skillator sync` from a linked worktree
- **THEN** Skillator runs worktree synchronization

#### Scenario: Bare sync discovers a Target
- **WHEN** the user runs `skillator sync` from a primary worktree or ordinary Git checkout
- **THEN** Skillator runs Target synchronization

#### Scenario: Hook commands do not require a terminal
- **WHEN** the user runs any `skillator hook` command with redirected input or output
- **THEN** Skillator performs the requested inspection or mutation without launching a TUI

### Requirement: Hook mutations follow shared CLI safety rules
`skillator hook install` and `skillator hook uninstall` SHALL support `--check` and the text, JSON, and YAML formats. Installation SHALL support `--force` only for an inspected, readable existing hook that can be preserved and chained. `--check --force` SHALL be invalid. `skillator hook status` SHALL be read-only and SHALL reject `--check`.

#### Scenario: Hook installation conflict
- **WHEN** hook installation encounters an unrelated existing hook without `--force`
- **THEN** it emits a trustworthy guarded report and performs no write

#### Scenario: Invalid hook options
- **WHEN** the user supplies `--check --force` to a hook mutation
- **THEN** command parsing fails with exit status `2`

#### Scenario: Machine-readable hook report
- **WHEN** the user requests JSON or YAML from a hook mutation or status command
- **THEN** Skillator emits ANSI-free deterministic output using the same format and stable exit-status meanings as other non-interactive commands

### Requirement: Interactive commands require terminals
The Target and Library TUI commands SHALL require both interactive input and output terminals. A non-TTY invocation SHALL fail with guidance and MUST NOT silently run synchronization. `-h`, `--help`, and root `-V` or `--version` SHALL render text to stdout and exit successfully.

#### Scenario: Target TUI piped from stdin
- **WHEN** the root command lacks an interactive input or output terminal
- **THEN** Skillator reports the requirement and performs no writes

### Requirement: Sync has a bounded option set
`skillator sync` and its explicit `target` and `worktree` modes SHALL support only `--check`, `--force`, `--format <text|json|yaml>`, and `--color <auto|always|never>` in addition to one optional Target directory. Format SHALL default to `text`. `--check --force` SHALL be invalid, and explicit color SHALL conflict with JSON or YAML.

#### Scenario: Equals and separated format syntax
- **WHEN** the user supplies either `--format=json` or `--format json`
- **THEN** Skillator selects the same JSON renderer

#### Scenario: Conflicting sync options
- **WHEN** the user supplies `--check --force` or requests color with a machine format
- **THEN** command parsing fails with exit status `2`

### Requirement: Check mode never writes
Check mode SHALL run the same loading, discovery, observation, validation, and planning behavior as sync without configuration, lock-file, recovery, control-file, or Materialization writes. It SHALL report Safe work as Would Apply, Guarded work as Would Require Force, Blocked work as Blocked, and conforming state as No Change.

#### Scenario: Drift found by check
- **WHEN** check observes a missing Materialization that ordinary sync could safely create
- **THEN** it reports Would Apply and leaves the filesystem unchanged

### Requirement: Sync does not create or edit desired state
Sync SHALL load current local Target configuration and reconcile only filesystem state. It MUST NOT create missing Repository Configuration, change Repository Configuration, or register Sources or Skills. `sync worktree` is the sole exception: it MAY replace the current linked worktree's local Target configuration with the validated primary worktree configuration. Missing Repository Configuration for ordinary sync SHALL direct the user to the TUI. Missing Library configuration SHALL behave as an empty Library, leaving existing Source references Unresolved while permitting independent work that does not require Source content.

#### Scenario: Missing Repository Configuration
- **WHEN** sync targets a repository without `.agents/skillator.yaml`
- **THEN** it performs no reconciliation writes and directs the user to run the Target TUI

#### Scenario: Missing Library configuration
- **WHEN** valid Repository Configuration exists but Library configuration is absent
- **THEN** sync treats the Library as empty, preserves desired state, and reports unresolved references rather than malformed input

#### Scenario: Worktree command is not ordinary sync
- **WHEN** `skillator sync worktree` has a valid primary worktree configuration and the current linked worktree has none
- **THEN** it copies the primary configuration before reconciling the current worktree

### Requirement: Completed reports use stdout and diagnostics use stderr
A completed trustworthy sync or check report SHALL be written entirely to stdout, including a non-converged report returning `1`. Parser errors and fatal pre-report failures SHALL be written entirely to stderr with stdout empty. Help and version SHALL always use text stdout.

#### Scenario: Partial synchronization
- **WHEN** sync applies some work but returns a trustworthy non-converged result
- **THEN** the complete selected-format report is on stdout and the process exits `1`

### Requirement: Text reports are concise
Text output SHALL print `In sync.` when nothing needs attention. Otherwise it SHALL list only changes, problems, discoveries, and necessary remediation, without enumerating every conforming Enablement or rendering a comprehensive internal state snapshot. Color auto SHALL require terminal stdout, `TERM` other than `dumb`, and absence of `NO_COLOR`.

#### Scenario: Clean Target
- **WHEN** sync or check completes with no required change or Drift
- **THEN** text output is `In sync.` with no per-Enablement inventory

### Requirement: JSON and YAML encode one compact report
JSON and YAML SHALL encode the same logical object containing only `format_version`, `status`, `exit_status`, `mode`, `target`, `changes`, and `diagnostics`. `format_version` SHALL be `1`; the CLI SHALL expose no format-version selector. Changes SHALL carry only the path, action, safety classification, and outcome needed to understand proposed or attempted work. Diagnostics SHALL carry stable code, severity, message, and only relevant optional structured data.

`library rsync` SHALL retain this outer report envelope, use `mode: "library_rsync"`, and add a `host` field to its change entries and relevant diagnostic data. Its `target` SHALL identify the initiating user home. Existing command reports SHALL retain their schemas.

#### Scenario: Equivalent machine formats
- **WHEN** the same deterministic result is rendered as JSON and YAML
- **THEN** both deserialize to the same logical value with the same stable array ordering

#### Scenario: Advisory discovery
- **WHEN** sync discovers an Unregistered Source or Skill
- **THEN** the machine report includes a diagnostic rather than a separate Library inventory

### Requirement: Machine output is deterministic and ANSI-free
Machine fields and enums SHALL use lowercase `snake_case`; inapplicable fields SHALL be omitted; timestamps, durations, network hostnames, and random run identifiers MUST NOT be emitted. Remote library reports SHALL use configured host aliases and the reserved participant label `local` to attribute outcomes; aliases SHALL NOT be expanded to network destinations in machine output. JSON SHALL be one valid UTF-8 document. YAML SHALL be one UTF-8 document beginning `---`, ending with one newline, using only JSON-compatible value types and double-quoted string keys and values, with no tags, anchors, aliases, merge keys, directives, comments, BOM, or non-finite numbers. Machine output MUST NOT contain ANSI escapes.

#### Scenario: YAML string-like scalar
- **WHEN** a diagnostic message or path resembles a boolean, null, date, or number
- **THEN** YAML emits it as a double-quoted string preserving the same value as JSON

### Requirement: Exit statuses have stable meanings
Skillator SHALL return `0` for a completed acceptable result, `1` for a trustworthy completed result that is not converged, `2` for invalid invocation, `3` for invalid or unavailable required input, `4` for an actively busy Target, and `5` for fatal failure before a trustworthy report. Sync and check SHALL return `0` only when final state is In Sync; Drift, Unverifiable state, Not Authorized, Blocked, failed rollback, Recovery Required, or partial apply SHALL return `1`.

#### Scenario: Advisory warning on success
- **WHEN** a Target is In Sync but has an advisory compatibility or discovery warning
- **THEN** Skillator may report the warning and still return `0`

#### Scenario: Active Target lock
- **WHEN** sync or check finds an active mutation owner
- **THEN** Skillator returns `4` without a reconciliation report or writes

### Requirement: Platform support is Unix-compatible
The MVP SHALL support macOS, Linux, and WSL running against its Linux filesystem. On capability-dependent or Windows-mounted filesystems, Skillator SHALL either complete supported operations or return a trustworthy Blocked or capability result without corruption or silent Materialization fallback. Native Windows outside WSL is not supported.

#### Scenario: Unsupported mounted-filesystem operation
- **WHEN** a required filesystem capability is unavailable on a WSL-mounted path
- **THEN** Skillator preserves desired and existing state and reports the blocked capability without switching Materialization kind

### Requirement: CLI Skill selectors have one canonical form
A CLI Skill selector SHALL use `<source-key>:<skill-path>`, where the final colon separates the canonical Source Key from the slash-normalized path relative to that Source. Skillator MUST resolve the selector to exactly one currently Registered valid Skill before adding a new Enablement. Machine-readable output SHALL return the canonical Source Key and Skill path as separate fields.

#### Scenario: Canonical selector resolves
- **WHEN** the user supplies `elastic/agent-skills:skills/esdiag` and that Skill is Registered and valid
- **THEN** Skillator selects that exact Skill without consulting its frontmatter name or directory basename

#### Scenario: Unresolved selector
- **WHEN** the selector does not identify one currently Registered valid Skill
- **THEN** Skillator reports a stable diagnostic and performs no configuration or Materialization write

### Requirement: CLI-first mutations share preview and reporting rules
Every CLI-first mutation SHALL support `--check` and `--format <text|json|yaml>`. Check mode SHALL perform the same loading, discovery, validation, and planning as application mode without writing configuration, control files, locks, recovery artifacts, or Materializations. Repeating an already satisfied command SHALL succeed with an `unchanged` outcome. Completed reports SHALL be deterministic, ANSI-free in machine formats, and use the existing stable process-status meanings. Commands that can authorize Guarded Changes SHALL support `--force`; `--check --force` SHALL be invalid.

#### Scenario: Mutation preview
- **WHEN** a user runs a valid `target link` command with `--check`
- **THEN** Skillator reports the desired-state and Materialization changes it would make and leaves every file unchanged

#### Scenario: Idempotent mutation
- **WHEN** a user repeats a mutation whose requested desired and observed state already conform
- **THEN** Skillator reports `unchanged` and exits successfully

#### Scenario: Guarded mutation without authorization
- **WHEN** a CLI-first mutation plans a Guarded Change and the user did not pass `--force`
- **THEN** Skillator reports that the change requires force and leaves the guarded state unchanged

### Requirement: Read-only CLI reports are deterministic and scriptable
`library locations`, `library list`, `target list`, `targets list`, and `user list` SHALL support `--format <text|json|yaml>`. Their machine formats SHALL be ANSI-free, encode the same logical value, and use deterministic ordering. Read-only commands SHALL NOT accept `--check` or modify configuration, registries, control files, or Materializations.

#### Scenario: Inspect scope state as JSON
- **WHEN** a user runs `skillator target list --format json`
- **THEN** Skillator emits a deterministic versioned report without changing Target state

### Requirement: Skillator composes with Git repository operations
Skillator SHALL NOT provide general-purpose commands that clone a remote repository or create, remove, or prune Git worktrees. The sole clone exception SHALL be `library rsync`, which bootstraps an absent matching Library Git source at the initiating host's exact commit. Rsync SHALL NOT update existing checkouts. The separate `library update` command SHALL be limited to fast-forwarding clean Git checkouts in registered Library Locations under its own requirements; it SHALL NOT be a general repository-update command. Users and agents SHALL use Git for general repository and worktree operations, then use Skillator to register Library Locations, initialize Targets, synchronize linked worktrees, and clean Skillator registries.

#### Scenario: Acquire a remote Skill source
- **WHEN** an agent needs a remote Skills repository
- **THEN** it runs `git clone` and then registers the resulting directory with `skillator library add`

#### Scenario: Create a linked worktree
- **WHEN** an agent needs a linked worktree
- **THEN** it runs `git worktree add` and then runs `skillator sync worktree` in or against that worktree

### Requirement: Library rsync exposes explicit policies

`skillator library rsync` SHALL accept `--hosts <alias,...>`, `--conflict <local|remote|ask>`, `--missing <copy|remove|ignore>`, `--check`, `--format <text|json|yaml>`, and `--color <auto|always|never>`. Defaults SHALL be all configured hosts, `ask`, `copy`, text, and automatic color. Explicit color SHALL conflict with machine formats. Unsupported policy values, empty host selectors, and unknown aliases SHALL return `2`. This command SHALL NOT expose `--force`; its policy flags authorize only the defined synchronization actions. It SHALL run without a terminal and only ask for skill-content conflicts in interactive text application mode. The alias `local` SHALL be reserved for the initiating participant. Help SHALL describe library-content synchronization; user selections and materializations remain host-local.

#### Scenario: Default command

- **WHEN** the user runs `skillator library rsync` without flags
- **THEN** it selects every configured host and uses interactive conflict resolution when a terminal is available, with missing-file copying

### Requirement: Remote synchronization reports partial and blocked work

Remote reports SHALL identify affected configured aliases, source and relative path where relevant, planned or applied action, conflict candidates, and actionable diagnostics. Pre-report required-input failures, including missing or incompatible remote Skillator, SHALL return `3` with diagnostics on stderr and no stdout report. Parser failures SHALL return `2`; an actively busy participant detected before work SHALL return `4`; fatal pre-report failures SHALL return `5`. Trustworthy partial, blocked, unresolved-conflict, recovery-required, or unverified results SHALL return `1` with a report on stdout. Fully completed policy outcomes SHALL return `0`. An intentional `--missing ignore` outcome SHALL be reported without causing failure by itself. First-contact unmatched entries under `remove` SHALL return `1` until resolved. Check mode SHALL return `1` for required or unverified work and `0` only when no work remains under the selected policy.

#### Scenario: Source mismatch after host preflight

- **WHEN** every selected host passes preflight but one source has a commit mismatch
- **THEN** the command can synchronize independent sources, reports the blocked source and affected aliases, and returns `1`

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
