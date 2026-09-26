## MODIFIED Requirements

### Requirement: Skillator composes with Git repository operations
Skillator SHALL NOT provide general-purpose commands that clone a remote repository or create, remove, or prune Git worktrees. The sole clone exception SHALL be `library rsync`, which bootstraps an absent matching Library Git source at the initiating host's exact commit. It SHALL NOT update existing checkouts or expose a general repository-update command. Users and agents SHALL use Git for those operations, then use Skillator to register Library Locations, initialize Targets, synchronize linked worktrees, and clean Skillator registries.

#### Scenario: Acquire a remote Skill source
- **WHEN** an agent needs a remote Skills repository
- **THEN** it runs `git clone` and then registers the resulting directory with `skillator library add`

#### Scenario: Create a linked worktree
- **WHEN** an agent needs a linked worktree
- **THEN** it runs `git worktree add` and then runs `skillator sync worktree` in or against that worktree

### Requirement: Machine output is deterministic and ANSI-free
Machine fields and enums SHALL use lowercase `snake_case`; inapplicable fields SHALL be omitted; timestamps, durations, network hostnames, and random run identifiers MUST NOT be emitted. Remote library reports SHALL use configured host aliases and the reserved participant label `local` to attribute outcomes; aliases SHALL NOT be expanded to network destinations in machine output. JSON SHALL be one valid UTF-8 document. YAML SHALL be one UTF-8 document beginning `---`, ending with one newline, using only JSON-compatible value types and double-quoted string keys and values, with no tags, anchors, aliases, merge keys, directives, comments, BOM, or non-finite numbers. Machine output MUST NOT contain ANSI escapes.

#### Scenario: YAML string-like scalar
- **WHEN** a diagnostic message or path resembles a boolean, null, date, or number
- **THEN** YAML emits it as a double-quoted string preserving the same value as JSON

### Requirement: JSON and YAML encode one compact report
JSON and YAML SHALL encode the same logical object containing only `format_version`, `status`, `exit_status`, `mode`, `target`, `changes`, and `diagnostics`. `format_version` SHALL be `1`; the CLI SHALL expose no format-version selector. Changes SHALL carry only the path, action, safety classification, and outcome needed to understand proposed or attempted work. Diagnostics SHALL carry stable code, severity, message, and only relevant optional structured data.

`library rsync` SHALL retain this outer report envelope, use `mode: "library_rsync"`, and add a `host` field to its change entries and relevant diagnostic data. Its `target` SHALL identify the initiating user home. Existing command reports SHALL retain their schemas.

#### Scenario: Equivalent machine formats
- **WHEN** the same deterministic result is rendered as JSON and YAML
- **THEN** both deserialize to the same logical value with the same stable array ordering

#### Scenario: Advisory discovery
- **WHEN** sync discovers an Unregistered Source or Skill
- **THEN** the machine report includes a diagnostic rather than a separate Library inventory

## ADDED Requirements

### Requirement: Library rsync exposes explicit policies

`skillator library rsync` SHALL accept `--hosts <alias,...>`, `--conflict <local|remote|ask>`, `--missing <copy|remove|ignore>`, `--check`, `--format <text|json|yaml>`, and `--color <auto|always|never>`. Defaults SHALL be all configured hosts, `ask`, `copy`, text, and automatic color. Explicit color SHALL conflict with machine formats. Unsupported policy values, empty host selectors, and unknown aliases SHALL return `2`. This command SHALL NOT expose `--force`; its policy flags authorize only the defined synchronization actions. It SHALL run without a terminal and only ask for file or selection conflicts in interactive text application mode. The alias `local` SHALL be reserved for the initiating participant.

#### Scenario: Default command

- **WHEN** the user runs `skillator library rsync` without flags
- **THEN** it selects every configured host and uses interactive conflict resolution when a terminal is available, with missing-file copying

### Requirement: Remote synchronization reports partial and blocked work

Remote reports SHALL identify affected configured aliases, source and relative path where relevant, planned or applied action, conflict candidates, and actionable diagnostics. Pre-report required-input failures, including missing or incompatible remote Skillator, SHALL return `3` with diagnostics on stderr and no stdout report. Parser failures SHALL return `2`; an actively busy participant detected before work SHALL return `4`; fatal pre-report failures SHALL return `5`. Trustworthy partial, blocked, unresolved-conflict, recovery-required, or unverified results SHALL return `1` with a report on stdout. Fully completed policy outcomes SHALL return `0`. An intentional `--missing ignore` outcome SHALL be reported without causing failure by itself. First-contact unmatched entries under `remove` SHALL return `1` until resolved. Check mode SHALL return `1` for required or unverified work and `0` only when no work remains under the selected policy.

#### Scenario: Source mismatch after host preflight

- **WHEN** every selected host passes preflight but one source has a commit mismatch
- **THEN** the command can synchronize independent sources, reports the blocked source and affected aliases, and returns `1`
