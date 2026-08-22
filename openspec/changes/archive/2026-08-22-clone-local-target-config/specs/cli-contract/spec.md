## MODIFIED Requirements

### Requirement: The CLI exposes three entry points
The MVP SHALL expose `skillator [OPTIONS] [DIRECTORY]`, `skillator library [OPTIONS]`, `skillator sync [OPTIONS] [DIRECTORY]`, and `skillator worktree sync [OPTIONS] [DIRECTORY]`. The root command SHALL launch the Target TUI, `library` SHALL launch the user-scoped Library TUI from any directory, `sync` SHALL reconcile one existing local Target configuration, and `worktree sync` SHALL project the primary worktree's local Target configuration into a linked worktree. The MVP MUST NOT expose aliases or command-line registration CRUD.

#### Scenario: Default root invocation
- **WHEN** the user runs `skillator` in a Git worktree with interactive input and output
- **THEN** Skillator opens the normal Library workspace with its first-run welcome when Library Configuration is absent, otherwise launches the Target TUI for the current worktree root with the first Repository Skill Directory selected

#### Scenario: Library invocation outside Git
- **WHEN** the user runs `skillator library` from a non-Git directory with interactive input and output
- **THEN** Skillator launches the Library workspace without requiring a Target

#### Scenario: Worktree synchronization
- **WHEN** the user runs `skillator worktree sync` from a registered linked worktree
- **THEN** Skillator performs the non-interactive worktree projection and emits the selected report format

### Requirement: Sync does not create or edit desired state
`skillator sync` SHALL load current local Target configuration and reconcile only filesystem state. It MUST NOT create missing Target configuration, change Target configuration, or register Sources or Skills. `skillator worktree sync` is the sole exception: it MAY replace the current linked worktree's local Target configuration with the validated primary worktree configuration. Missing Target configuration for ordinary sync SHALL direct the user to the TUI. Missing Library configuration SHALL behave as an empty Library, leaving existing Source references Unresolved while permitting independent work that does not require Source content.

#### Scenario: Missing Repository Configuration
- **WHEN** sync targets a repository without `.agents/skillator.yaml`
- **THEN** it performs no reconciliation writes and directs the user to run the Target TUI

#### Scenario: Missing Library configuration
- **WHEN** valid Repository Configuration exists but Library configuration is absent
- **THEN** sync treats the Library as empty, preserves desired state, and reports unresolved references rather than malformed input

#### Scenario: Worktree command is not ordinary sync
- **WHEN** `skillator worktree sync` has a valid primary worktree configuration and the current linked worktree has none
- **THEN** it copies the primary configuration before reconciling the current worktree
