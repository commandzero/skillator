## Purpose

Defines Skillator's small public command surface, non-interactive synchronization behavior, compact machine-readable output, and stable process outcomes for scripts.

## ADDED Requirements

### Requirement: The CLI exposes three entry points
The MVP SHALL expose `skillator [OPTIONS] [DIRECTORY]`, `skillator library [OPTIONS]`, and `skillator sync [OPTIONS] [DIRECTORY]`. The root command SHALL launch the Target TUI, `library` SHALL launch the user-scoped Library TUI from any directory, and `sync` SHALL be the only non-interactive workflow. The MVP MUST NOT expose aliases or command-line registration CRUD.

#### Scenario: Default root invocation
- **WHEN** the user runs `skillator` in a Git worktree with interactive input and output
- **THEN** Skillator launches the Target TUI for the current worktree root

#### Scenario: Library invocation outside Git
- **WHEN** the user runs `skillator library` from a non-Git directory with interactive input and output
- **THEN** Skillator launches the Library workspace without requiring a Target

### Requirement: Interactive commands require terminals
The Target and Library TUI commands SHALL require both interactive input and output terminals. A non-TTY invocation SHALL fail with guidance and MUST NOT silently run synchronization. `-h`, `--help`, and root `-V` or `--version` SHALL render text to stdout and exit successfully.

#### Scenario: Target TUI piped from stdin
- **WHEN** the root command lacks an interactive input or output terminal
- **THEN** Skillator reports the requirement and performs no writes

### Requirement: Sync has a bounded option set
`skillator sync` SHALL support only `--check`, `--force`, `--format <text|json|yaml>`, and `--color <auto|always|never>` in addition to one optional Target directory. Format SHALL default to `text`. `--check --force` SHALL be invalid, and explicit color SHALL conflict with JSON or YAML.

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
Sync SHALL load current configuration and reconcile only filesystem state. It MUST NOT create missing Repository Configuration, change Repository Configuration, or register Sources or Skills. Missing Repository Configuration SHALL direct the user to the TUI. Missing Library configuration SHALL behave as an empty Library, leaving existing Source references Unresolved while permitting independent work that does not require Source content.

#### Scenario: Missing Repository Configuration
- **WHEN** sync targets a repository without `.agents/skillator.yaml`
- **THEN** it performs no reconciliation writes and directs the user to run the Target TUI

#### Scenario: Missing Library configuration
- **WHEN** valid Repository Configuration exists but Library configuration is absent
- **THEN** sync treats the Library as empty, preserves desired state, and reports unresolved references rather than malformed input

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
