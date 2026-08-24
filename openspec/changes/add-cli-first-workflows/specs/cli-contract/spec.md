## MODIFIED Requirements

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
