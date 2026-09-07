# cli-contract Specification

## Purpose
Defines Skillator's small public command surface, non-interactive synchronization behavior, compact machine-readable output, and stable process outcomes for scripts.
## Requirements
### Requirement: The CLI exposes interactive defaults and explicit command groups
Skillator SHALL expose top-level `init`, `library`, `target`, `targets`, `user`, and `sync` commands. It SHALL NOT expose a top-level `worktree` command. Unqualified `skillator [DIRECTORY]` SHALL launch the Target TUI, and unqualified `skillator library` SHALL launch the Library TUI. `init`, `library add`, `library remove`, `library prune`, `library locations`, `library list`, `target list`, `target link`, `target copy`, `target remove`, `targets list`, `targets remove`, `targets prune`, `user list`, `user link`, `user copy`, and `user remove` SHALL run without an interactive terminal. `skillator sync target [directory]` SHALL run ordinary Target reconciliation. `skillator sync worktree [directory]` SHALL project primary-worktree Target state into a linked worktree. Both explicit sync forms SHALL default the directory to `.`.

#### Scenario: Default root invocation
- **WHEN** the user runs `skillator` in a Git worktree with interactive input and output
- **THEN** Skillator opens the normal Library workspace with its first-run welcome when Library Configuration is absent, otherwise launches the Target TUI for the current worktree root

#### Scenario: Bare Library invocation
- **WHEN** the user runs `skillator library` with interactive input and output
- **THEN** Skillator launches the Library TUI without requiring a Target Repository

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
### Requirement: Interactive commands require terminals
The Target and Library TUI commands SHALL require both interactive input and output terminals. A non-TTY invocation SHALL fail with guidance and MUST NOT silently run synchronization. `-h`, `--help`, and root `-V` or `--version` SHALL render text to stdout and exit successfully.

#### Scenario: Target TUI piped from stdin
- **WHEN** the root command lacks an interactive input or output terminal
- **THEN** Skillator reports the requirement and performs no writes
### Requirement: Sync has a bounded option set
`skillator sync` and `skillator worktree sync` SHALL support only `--check`, `--force`, `--format <text|json|yaml>`, and `--color <auto|always|never>` in addition to one optional Target directory. Format SHALL default to `text`. `--check --force` SHALL be invalid, and explicit color SHALL conflict with JSON or YAML.

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
Sync SHALL load current local Target configuration and reconcile only filesystem state. It MUST NOT create missing Repository Configuration, change Repository Configuration, or register Sources or Skills. `worktree sync` is the sole exception: it MAY replace the current linked worktree's local Target configuration with the validated primary worktree configuration. Missing Repository Configuration for ordinary sync SHALL direct the user to the TUI. Missing Library configuration SHALL behave as an empty Library, leaving existing Source references Unresolved while permitting independent work that does not require Source content.

#### Scenario: Missing Repository Configuration
- **WHEN** sync targets a repository without `.agents/skillator.yaml`
- **THEN** it performs no reconciliation writes and directs the user to run the Target TUI

#### Scenario: Missing Library configuration
- **WHEN** valid Repository Configuration exists but Library configuration is absent
- **THEN** sync treats the Library as empty, preserves desired state, and reports unresolved references rather than malformed input

#### Scenario: Worktree command is not ordinary sync
- **WHEN** `skillator worktree sync` has a valid primary worktree configuration and the current linked worktree has none
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

#### Scenario: Equivalent machine formats
- **WHEN** the same deterministic result is rendered as JSON and YAML
- **THEN** both deserialize to the same logical value with the same stable array ordering

#### Scenario: Advisory discovery
- **WHEN** sync discovers an Unregistered Source or Skill
- **THEN** the machine report includes a diagnostic rather than a separate Library inventory
### Requirement: Machine output is deterministic and ANSI-free
Machine fields and enums SHALL use lowercase `snake_case`; inapplicable fields SHALL be omitted; timestamps, durations, hostnames, and random run identifiers MUST NOT be emitted. JSON SHALL be one valid UTF-8 document. YAML SHALL be one UTF-8 document beginning `---`, ending with one newline, using only JSON-compatible value types and double-quoted string keys and values, with no tags, anchors, aliases, merge keys, directives, comments, BOM, or non-finite numbers. Machine output MUST NOT contain ANSI escapes.

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
Skillator SHALL NOT provide commands that clone a remote repository or create, remove, or prune Git worktrees. Users and agents SHALL use Git for those operations, then use Skillator to register Library Locations, initialize Targets, synchronize linked worktrees, and clean Skillator registries.

#### Scenario: Acquire a remote Skill source
- **WHEN** an agent needs a remote Skills repository
- **THEN** it runs `git clone` and then registers the resulting directory with `skillator library add`

#### Scenario: Create a linked worktree
- **WHEN** an agent needs a linked worktree
- **THEN** it runs `git worktree add` and then runs `skillator sync worktree` in or against that worktree
