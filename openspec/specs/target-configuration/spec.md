# target-configuration Specification

## Purpose
Defines portable, strict desired state for one Git Target Repository, including its managed Skill Directories, per-directory Enablements, presets, and validation boundaries.
## Requirements
### Requirement: Targets are Git worktree roots
A Target SHALL be selected from one existing directory within a Git worktree. Skillator SHALL canonicalize the supplied directory and operate on the resolved worktree root. Files, missing paths, bare repositories, and non-Git directories MUST be rejected without writes.

#### Scenario: Nested directory selects repository root
- **WHEN** the user supplies an existing subdirectory of a Git worktree
- **THEN** Skillator reports the supplied path and operates on the containing worktree root

#### Scenario: Non-Git target is rejected
- **WHEN** the user supplies a directory outside a Git worktree
- **THEN** Skillator reports invalid Target input and performs no writes
### Requirement: Repository Configuration has one version 1 schema
Each Git worktree SHALL keep its clone-local Target desired state in `.agents/skillator.yaml`. The document SHALL use numeric `version: 1` followed by zero or more top-level Skill Directory mappings keyed by Skill Directory Key. Each directory mapping SHALL contain required `path`, optional `label`, and `skills`. `skills` SHALL map the materialized Skill name to an entry with required `source`, optional Source-relative `path`, and optional `type` equal to `linked` or `copied`. Omitted `path` defaults to the Skill mapping key; omitted `type` defaults to `linked`. Presence of a Skill entry SHALL mean enabled; no `enabled` field exists in version 1.

The configuration is local to the checkout and MUST NOT be tracked by Git. The generated `.agents/.gitignore` SHALL ignore both `skillator.yaml` and itself. Skillator MUST NOT edit the repository root `.gitignore` or change the Git index. If the local configuration is already tracked, Skillator SHALL preserve its contents, block configuration writes, and report the exact `git rm --cached -- .agents/skillator.yaml` remediation.

#### Scenario: Minimal empty configuration
- **WHEN** a version 1 document contains no Skill Directory mappings
- **THEN** Skillator treats the document as valid intentional configuration and does not restage defaults

#### Scenario: Default Linked materialization
- **WHEN** the TUI creates a new Enablement without a prior mode choice
- **THEN** it stages `linked` and omits `type` from canonical saved YAML

#### Scenario: Clone-local configuration is ignored
- **WHEN** a worktree saves `.agents/skillator.yaml`
- **THEN** `.agents/.gitignore` leaves the local configuration untracked and available only in that checkout while the root `.gitignore` remains unchanged

#### Scenario: Legacy tracked configuration
- **WHEN** `.agents/skillator.yaml` is already tracked
- **THEN** Skillator preserves the file, performs no configuration write, and reports the required index-only removal command
### Requirement: Repository Configuration validation is strict
The Repository Configuration SHALL reject unknown fields, missing required fields, nulls, incorrect types, unsupported enum values, duplicate YAML mapping keys, anchors, aliases, tags, merge keys, and multiple YAML documents. Validation SHALL collect every independently detectable problem with stable field paths and line and column when available. Invalid configuration SHALL yield no trusted desired state and MUST prevent configuration and reconciliation writes.

#### Scenario: Multiple validation errors
- **WHEN** a parseable document contains several independent structural errors
- **THEN** Skillator reports all detectable errors and exposes no partially trusted desired state
### Requirement: Portable identities and paths are canonical
Skill Directory Keys SHALL match `[a-z0-9]+(?:-[a-z0-9]+)*`. Source Keys SHALL contain at least two lowercase slash-separated canonical segments. Stored paths SHALL use `/`, SHALL be relative, and MUST reject empty segments, backslashes, `..`, redundant `.` segments, control characters, and line breaks. `skill.path: .` SHALL be the only dot-path exception and SHALL identify a Skill rooted at its Source.

#### Scenario: Noncanonical identity
- **WHEN** a key contains uppercase characters but otherwise has a valid canonical lowercase form
- **THEN** Skillator rejects it and suggests the canonical replacement without silently changing tracked intent

#### Scenario: Escaping path
- **WHEN** a Skill Directory or Skill path contains `..` or is absolute
- **THEN** Skillator rejects the configuration before filesystem inspection or mutation
### Requirement: Skill Directory declarations do not overlap protected paths
A Skill Directory SHALL resolve inside the Target and MUST NOT be the repository root, `.git`, a descendant of `.git`, the Repository Configuration path, or an ancestor or descendant that contains the Repository Configuration. Configured Skill Directory paths MUST NOT overlap one another exactly or by ancestry. Their roots and repository-relative parent components MUST NOT be directory symlinks.

#### Scenario: Overlapping Skill Directories
- **WHEN** two declarations use exact or ancestor-descendant paths
- **THEN** Skillator rejects the Repository Configuration

#### Scenario: Symlinked ancestor
- **WHEN** an otherwise valid Skill Directory path traverses a directory symlink on the current machine
- **THEN** Skillator preserves the valid declared intent but blocks reconciliation for that directory with a containment diagnostic
### Requirement: Enablement relationships are unambiguous
Skill Directory Keys SHALL be unique case-insensitively. Each Enablement's `directory` SHALL reference a declared key. Duplicate Enablements with the same directory, Source Key, and Skill path MUST be rejected even if their Materialization values differ. The same Skill MAY be enabled in multiple distinct Skill Directories.

#### Scenario: Duplicate Enablement
- **WHEN** two Enablements identify the same Skill in the same Skill Directory
- **THEN** Skillator rejects the configuration as ambiguous
### Requirement: Skill Directory presets are creation-time suggestions
The MVP SHALL offer exactly two built-in presets: `agents`, labeled `.agents`, at `.agents/skills`; and `claude`, labeled `.claude`, at `.claude/skills`. A preset SHALL prefill editable key, label, and path values but MUST NOT persist a preset identifier or built-in/custom flag. A saved preset-created directory SHALL behave identically to a custom directory.

#### Scenario: First-run Target default
- **WHEN** Repository Configuration is absent and the Target TUI opens
- **THEN** Skillator stages the `agents` preset without writing until save

#### Scenario: Preset path edited
- **WHEN** a user changes a preset's path before saving
- **THEN** Skillator persists only the explicit key, path, and optional label and treats it as an ordinary Skill Directory
### Requirement: Agent compatibility is advisory
Skillator SHALL derive agent compatibility from a normalized documented Skill Directory path, not from its key, label, or preset history. Compatibility SHALL be informational and MUST NOT promise exclusive exposure. Enabling the same Skill in directories with overlapping compatible agents SHALL produce a non-blocking warning rather than invalid configuration.

#### Scenario: Generic compatibility
- **WHEN** a Skill Directory path is `.agents/skills`
- **THEN** Skillator identifies Codex, Copilot, Cursor, and Gemini CLI as known compatible agents without claiming exclusivity
### Requirement: Local resolution does not alter portable intent
A structurally valid Skill reference SHALL remain valid when its Source or Skill cannot be resolved through the current machine's Library. Skillator SHALL preserve it exactly as an Unresolved Enablement and MUST NOT delete, disable, or rewrite it automatically. The normal picker SHALL create new Enablements only for currently Registered valid Skills.

#### Scenario: Fresh clone with missing Source mapping
- **WHEN** a tracked Repository Configuration references a Source not mapped in the current Library
- **THEN** Skillator displays the Enablement as Unresolved, preserves it, and reconciles independent resolvable work where safe
### Requirement: Canonical Repository Configuration is deterministic
Saving valid Repository Configuration SHALL emit one YAML document with `version` first, then Skill Directory mappings sorted by key; two-space indentation; lowercase `type` values; and a final newline. Each directory SHALL emit `path`, optional `label`, then `skills`; Skill entries SHALL have deterministic directory, Source Key, and Skill-path ordering. Derived or machine-local state MUST NOT be persisted.

#### Scenario: Semantically reordered input
- **WHEN** a user saves valid configuration whose lists are in arbitrary order
- **THEN** Skillator writes the same deterministic canonical ordering without adding local paths, availability, diagnostics, agent names, or observed state
### Requirement: Unsupported versions are read-only
The MVP SHALL read and write only numeric `version: 1`. Any other version SHALL be diagnosed and preserved byte-for-byte. Skillator SHALL provide no migration prompt, command, backup convention, upgrade, downgrade, or compatibility workflow in the MVP.

#### Scenario: Future version encountered
- **WHEN** `.agents/skillator.yaml` declares an unsupported version
- **THEN** Skillator may show a diagnostic-only interface but performs no configuration or reconciliation writes
### Requirement: A Target Repository can be initialized without the TUI
`skillator init [repository]` SHALL resolve a Git Target Repository and prepare version 1 Repository Configuration with the `agents` Skill Directory at `.agents/skills` and no Enablements. The repository argument SHALL default to `.`. It SHALL establish the parent Skill Directory control-file contract without enabling a Skill or editing the repository root `.gitignore`. Initialization SHALL obey existing tracking, containment, validation, confirmation, rollback, and stale-write rules.

#### Scenario: Initialize a fresh Target
- **WHEN** the selected Git Target has no Repository Configuration and its control paths are safe
- **THEN** Skillator creates the initial clone-local configuration and required control files with no Enablements

#### Scenario: Initialize an existing Target
- **WHEN** the selected Target already has valid Repository Configuration
- **THEN** Skillator reports `unchanged` and preserves the configuration

#### Scenario: Legacy tracked configuration
- **WHEN** initialization finds `.agents/skillator.yaml` already tracked by Git
- **THEN** Skillator preserves it and reports the existing index-only remediation command
### Requirement: Individual Target Enablements can be edited without the TUI
`skillator target link <selector> [repository]` and `skillator target copy <selector> [repository]` SHALL add or update one Enablement in a selected Skill Directory and request the named Materialization. `skillator target remove <selector> [repository]` SHALL remove the matching Enablement and reconcile only the Materialization managed by that declaration. `--directory <key>` SHALL select a configured Skill Directory. When omitted, Skillator SHALL use `agents` if present, otherwise the sole configured Skill Directory, and SHALL reject an ambiguous choice.

#### Scenario: Link a Skill in the default directory
- **WHEN** the Target has an `agents` Skill Directory and the user links a valid canonical Skill selector without `--directory`
- **THEN** Skillator saves a Linked Enablement in `agents` and reconciles its Expected Entry

#### Scenario: Select one of several directories
- **WHEN** the Target has several Skill Directories and the user supplies a valid `--directory` key
- **THEN** Skillator changes only the Enablement for that Skill Directory

#### Scenario: Ambiguous directory selection
- **WHEN** the Target has several Skill Directories, none is keyed `agents`, and the user omits `--directory`
- **THEN** Skillator lists the available keys and performs no write

#### Scenario: Remove a diverged copy
- **WHEN** removal would delete a Diverged Copy and the user did not authorize Guarded Changes
- **THEN** Skillator preserves the Enablement and copy and reports that force is required
### Requirement: Target Enablements can be inspected without the TUI
`skillator target list [repository]` SHALL list saved Repository Enablements for the selected Target, defaulting the repository to `.`. Results SHALL be grouped by Skill Directory and include its key and path, the canonical Source Key and Skill path, materialized name, requested `linked` or `copied` mode, current resolution state, and observed Materialization state or diagnostics. The command SHALL list only Repository Enablements from Repository Configuration; it SHALL NOT mix in inherited User Scope Skills or repository-owned physical Skills.

#### Scenario: List a configured Target
- **WHEN** a Target has saved Enablements across one or more Skill Directories
- **THEN** Skillator reports each Enablement under its saved directory in deterministic order

#### Scenario: List unresolved Target state
- **WHEN** a saved Target Enablement no longer resolves through the Library
- **THEN** Skillator includes the declaration and reports it as unresolved
