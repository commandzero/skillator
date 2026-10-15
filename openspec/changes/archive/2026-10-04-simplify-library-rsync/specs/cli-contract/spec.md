# Spec Delta

## MODIFIED Requirements

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
Machine fields and enums SHALL use lowercase `snake_case`; inapplicable fields SHALL be omitted; timestamps, durations, network hostnames, and random run identifiers MUST NOT be emitted. Remote library reports SHALL use configured follower aliases for leader pushes and the label `leader` for follower pulls; neither SHALL be expanded to network destinations in machine output. JSON SHALL be one valid UTF-8 document. YAML SHALL be one UTF-8 document beginning `---`, ending with one newline, using only JSON-compatible value types and double-quoted string keys and values, with no tags, anchors, aliases, merge keys, directives, comments, BOM, or non-finite numbers. Machine output MUST NOT contain ANSI escapes.

#### Scenario: YAML string-like scalar
- **WHEN** a diagnostic message or path resembles a boolean, null, date, or number
- **THEN** YAML emits it as a double-quoted string preserving the same value as JSON

### Requirement: Skillator composes with Git repository operations
Skillator SHALL NOT provide general-purpose commands that clone a remote repository or create, remove, or prune Git worktrees. `library rsync` SHALL only deliver current skill files and SHALL NOT clone repositories or manipulate Git metadata. The separate `library update` command SHALL be limited to fast-forwarding clean Git checkouts in registered Library Locations under its own requirements; it SHALL NOT be a general repository-update command. Users and agents SHALL use Git for general repository and worktree operations, then use Skillator to register Library Locations, initialize Targets, synchronize linked worktrees, and clean Skillator registries.

#### Scenario: Acquire a remote Skill source
- **WHEN** an agent needs a remote Skills repository
- **THEN** it runs `git clone` and then registers the resulting directory with `skillator library add`

#### Scenario: Create a linked worktree
- **WHEN** an agent needs a linked worktree
- **THEN** it runs `git worktree add` and then runs `skillator sync worktree` in or against that worktree


## ADDED Requirements

### Requirement: Library rsync reports follower replica outcomes
Reports SHALL identify the follower alias for pushes or `leader` for pulls, affected replica path, transfer direction, planned or applied action, and actionable failure. Invalid invocation SHALL return `2`; invalid local required input SHALL return `3`; fatal pre-report failure SHALL return `5`. A trustworthy incomplete or failed transfer report SHALL return `1` on stdout. Successful application SHALL return `0`; check SHALL return `0` only when affected replicas match the current leader export.

#### Scenario: One destination fails
- **WHEN** one selected receiver fails while another receives the main-host export
- **THEN** the report identifies both outcomes and returns `1` without claiming cross-host rollback

#### Scenario: Successful no-op
- **WHEN** all selected receiver replicas already match the main-host export
- **THEN** application or check reports no required changes and returns `0`


### Requirement: Library rsync is non-interactive and leader-authoritative
`skillator library rsync` SHALL accept `--hosts <alias,...>` on leaders, `--check`, `--format <text|json|yaml>`, and `--color <auto|always|never>`. Leaders SHALL default to pushing all configured followers; followers SHALL pull their configured leader and reject `--hosts`. Defaults SHALL be text and automatic color; explicit color SHALL conflict with machine formats. `--conflict`, `--missing`, and `--force` SHALL be invalid. Help SHALL explain leader authority and scoped replacement/deletion.

#### Scenario: Default command
- **WHEN** the user runs `skillator library rsync` without flags or a terminal
- **THEN** configuration determines either a push from the leader to all its followers or a pull from the leader into this follower, without asking questions

#### Scenario: Follower host selector is invalid
- **WHEN** a follower invokes `skillator library rsync --hosts dev`
- **THEN** parsing or invocation validation returns `2` without connecting or changing the replica

#### Scenario: Obsolete policy flag
- **WHEN** the user supplies `--conflict remote` or `--missing copy`
- **THEN** argument parsing returns `2` rather than retaining a hidden reconciliation mode

