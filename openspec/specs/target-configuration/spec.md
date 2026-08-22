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
Each Target SHALL declare desired state in `.agents/skillator.yaml` using numeric `version: 1` followed by zero or more top-level Skill Directory mappings keyed by Skill Directory Key. Each directory mapping SHALL contain required `path`, optional `label`, and `skills`. `skills` SHALL map the materialized Skill name to an entry with required `source`, optional Source-relative `path`, and optional `type` equal to `linked` or `copied`. Omitted `path` defaults to the Skill mapping key; omitted `type` defaults to `linked`. Presence of a Skill entry SHALL mean enabled; no `enabled` field exists in version 1.

#### Scenario: Minimal empty configuration
- **WHEN** a version 1 document contains no Skill Directory mappings
- **THEN** Skillator treats the document as valid intentional configuration and does not restage defaults

#### Scenario: Default Linked materialization
- **WHEN** the TUI creates a new Enablement without a prior mode choice
- **THEN** it stages `linked` and omits `type` from canonical saved YAML

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

