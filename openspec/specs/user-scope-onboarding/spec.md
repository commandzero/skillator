# user-scope-onboarding Specification

## Purpose
Defines machine-local User Scope desired state inherited by repository views. User Scope is a Target scope; it is not Library inventory and is never an automatic first-run workflow.
## Requirements

### Requirement: User Scope Configuration is strict and machine-local
Skillator SHALL read and write User Scope Configuration at `~/.agents/skillator.yaml` using the same version 1 desired-state shape and strict YAML rules as Repository Configuration. User Scope Skill Directory paths SHALL be relative to the user's home directory. The primary first-run directory SHALL use key `agents`, label `User`, and path `.agents/skills`. User Scope Configuration SHALL not be treated as Repository Configuration, tracked by Git, or subject to Repository control-file rules.

Explicit `library rsync` SHALL exchange User Scope desired state across selected hosts. Stored paths and materializations SHALL remain local to each receiving home. Ordinary User Scope commands and first-run behavior SHALL remain machine-local.

#### Scenario: First User Scope configuration
- **WHEN** a user opens the User tab and saves its implicit default directory without an existing User Scope Configuration
- **THEN** Skillator writes canonical `~/.agents/skillator.yaml` with the primary `.agents/skills` directory and the User Scope Enablements selected by the user

### Requirement: Existing user-scoped entries remain Target observations
Skillator SHALL observe existing immediate children of `~/.agents/skills` when rendering the User tab. It SHALL not automatically import, move, copy, or link them into a Library. A user chooses Library Locations in Library management and chooses User Scope Enablements in the User Target tab through the same explicit staged-save workflow as Repository targets.

#### Scenario: Existing physical and linked Skills
- **WHEN** `~/.agents/skills` contains a valid physical Skill, a valid Skill symlink, and an unrelated file
- **THEN** the User tab surfaces their observed state without changing any entry until the user explicitly stages and saves a supported User Scope action

### Requirement: User Scope reconciliation is not Git reconciliation
User Scope save SHALL reconcile Linked or Copied Materializations under configured home-relative Skill Directories using the same containment, staging, confirmation, rollback, and recovery principles as Repository reconciliation, but SHALL not create `.gitignore` control files, inspect Git tracking, or mutate any Git index. Repository tabs SHALL derive inherited User Enablements from the saved User Scope configuration and current User Scope observation.

#### Scenario: Saving a User Enablement
- **WHEN** the user enables a registered Skill from the User tab and confirms save
- **THEN** Skillator writes User Scope desired state and materializes the selected mode without creating a Skill Directory control file

### Requirement: Individual User Scope Enablements can be edited without the TUI
`skillator user link <selector>` and `skillator user copy <selector>` SHALL add or update one User Scope Enablement and reconcile the requested Materialization. `skillator user remove <selector>` SHALL remove the matching Enablement and reconcile only its managed Materialization. When User Scope Configuration is absent, the first link or copy SHALL prepare version 1 configuration with the `agents` directory at `.agents/skills`. User Scope commands SHALL apply existing home-relative containment and reconciliation rules without Git inspection or control-file changes.

#### Scenario: First User Scope link
- **WHEN** User Scope Configuration is absent and the user links a valid Skill
- **THEN** Skillator creates the default User Scope Configuration, adds the Linked Enablement, and reconciles it under `~/.agents/skills`

#### Scenario: Remove a User Scope Enablement
- **WHEN** the selected User Scope Enablement and its conforming Materialization exist
- **THEN** Skillator removes the declaration and managed Materialization without changing repository files

#### Scenario: Existing unmanaged User Scope entry
- **WHEN** the Expected Entry is occupied by content that Skillator cannot prove it manages
- **THEN** Skillator preserves the entry and blocks the mutation

### Requirement: User Scope Enablements can be inspected without the TUI
`skillator user list` SHALL list saved User Scope Enablements grouped by Skill Directory. Results SHALL include the directory key and path, canonical Source Key and Skill path, materialized name, requested `linked` or `copied` mode, current resolution state, and observed Materialization state or diagnostics. An absent User Scope Configuration SHALL produce a successful empty result without creating configuration.

#### Scenario: List User Scope state
- **WHEN** User Scope Configuration contains saved Enablements
- **THEN** Skillator reports them in deterministic Skill Directory and Enablement order

#### Scenario: User Scope is not initialized
- **WHEN** User Scope Configuration is absent
- **THEN** `user list` succeeds with an empty result and performs no write

### Requirement: Remote synchronization reconciles user selections semantically

`library rsync` SHALL merge Skill Directory definitions and Enablements by their stable keys, including linked or copied mode, rather than choosing an entire YAML file by timestamp. First-contact non-conflicting selections SHALL form a union. After a shared baseline exists, an explicit deselection SHALL propagate independently of `--missing`; it SHALL NOT delete the library skill. Competing mode, directory, or enablement edits SHALL use the conflict policy. A missing or unreadable configuration file SHALL NOT alone prove explicit deselection. A directory removal SHALL remain coupled to its enablements so application never publishes structurally invalid desired state.

#### Scenario: Remote deselection

- **WHEN** a remote host removes a previously synchronized Enablement while other hosts leave it unchanged
- **THEN** the next run removes that Enablement and its conforming managed materializations from participating hosts even with `--missing copy`, retaining library content

#### Scenario: First-contact independent selections

- **WHEN** hosts have different non-conflicting Enablements with no common history
- **THEN** synchronization combines those Enablements without treating absence as a deselection

### Requirement: Received selections use local reconciliation

The receiving Skillator SHALL rebuild user materializations from merged desired state and its live Library. It SHALL NOT copy user materialization symlinks or synchronize project configuration. Existing unmanaged entries and guarded materialization drift SHALL remain protected; file-conflict choices SHALL NOT imply force authorization. A blocked library source SHALL not receive new dependent Enablements. Unresolved pre-existing Enablements SHALL be retained. Reports and synchronization history SHALL distinguish saved desired state from successful materialization, and a run with failed materializations SHALL not claim convergence.

#### Scenario: Rebuild a user link

- **WHEN** a selected skill transfers between hosts with different home paths
- **THEN** the receiving link points at the receiving host's skill location

#### Scenario: Unmanaged user entry

- **WHEN** a desired user skill path contains unrelated unmanaged content
- **THEN** the command preserves that content, reports the blocked materialization, and does not mark it successfully synchronized
