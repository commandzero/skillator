## MODIFIED Requirements

### Requirement: The CLI exposes four entry points

The MVP SHALL expose `skillator [OPTIONS] [DIRECTORY]`, `skillator library [OPTIONS]`, `skillator sync [OPTIONS] [DIRECTORY]`, and `skillator worktree sync [OPTIONS] [DIRECTORY]`, together with the non-interactive `skillator hook install`, `skillator hook status`, and `skillator hook uninstall` commands. The root command SHALL launch the Target TUI, `library` SHALL launch the user-scoped Library TUI from any directory, `sync` SHALL reconcile one existing local Target configuration, `worktree sync` SHALL project the primary worktree's local Target configuration into the current linked worktree, and `hook` SHALL manage the repository-local Git integration. The MVP MUST NOT expose aliases or command-line registration CRUD.

#### Scenario: Default root invocation

- **WHEN** the user runs `skillator` in a Git worktree with interactive input and output
- **THEN** Skillator opens the normal Library workspace with its first-run welcome when Library Configuration is absent, otherwise launches the Target TUI for the current worktree root with the first Repository Skill Directory selected

#### Scenario: Library invocation outside Git

- **WHEN** the user runs `skillator library` from a non-Git directory with interactive input and output
- **THEN** Skillator launches the Library workspace without requiring a Target

#### Scenario: Worktree synchronization

- **WHEN** the user runs `skillator worktree sync` from a registered linked worktree
- **THEN** Skillator projects the primary worktree's local Target state and emits the selected report format

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
