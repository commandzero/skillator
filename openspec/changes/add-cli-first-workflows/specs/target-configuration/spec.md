## ADDED Requirements

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

## MODIFIED Requirements

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
