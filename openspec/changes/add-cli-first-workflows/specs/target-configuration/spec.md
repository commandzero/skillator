## ADDED Requirements

### Requirement: A Target Repository can be initialized without the TUI
`skillator target init [repository]` SHALL resolve a Git Target Repository and prepare version 1 Repository Configuration with the `agents` Skill Directory at `.agents/skills` and no Enablements. It SHALL establish the existing root ignore and Skill Directory control-file contract without enabling a Skill. Initialization SHALL obey existing tracking, containment, validation, confirmation, rollback, and stale-write rules.

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

