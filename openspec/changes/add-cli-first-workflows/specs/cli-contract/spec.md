## MODIFIED Requirements

### Requirement: The CLI exposes interactive defaults and explicit command groups
Skillator SHALL expose `library`, `target`, `user`, `sync`, and `worktree` command groups. Unqualified `skillator [DIRECTORY]` SHALL launch the Target TUI, and unqualified `skillator library` SHALL launch the Library TUI. `library add`, `library remove`, `library locations`, `library list`, `target init`, `target link`, `target copy`, `target remove`, `user link`, `user copy`, and `user remove` SHALL run without an interactive terminal. Existing `sync` and `worktree sync` behavior SHALL remain available.

#### Scenario: Default root invocation
- **WHEN** the user runs `skillator` in a Git worktree with interactive input and output
- **THEN** Skillator opens the normal Library workspace with its first-run welcome when Library Configuration is absent, otherwise launches the Target TUI for the current worktree root

#### Scenario: Bare Library invocation
- **WHEN** the user runs `skillator library` with interactive input and output
- **THEN** Skillator launches the Library TUI without requiring a Target Repository

#### Scenario: Library subcommand without a terminal
- **WHEN** the user runs `skillator library list elastic` without interactive input or output
- **THEN** Skillator lists matching live Library inventory without opening the TUI

#### Scenario: Existing synchronization command
- **WHEN** the user runs `skillator worktree sync` from a registered linked worktree
- **THEN** Skillator projects the primary worktree's local Target state and emits the selected report format

## ADDED Requirements

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

