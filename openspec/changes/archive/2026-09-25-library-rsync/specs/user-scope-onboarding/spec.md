## MODIFIED Requirements

### Requirement: User Scope Configuration is strict and machine-local
Skillator SHALL read and write User Scope Configuration at `~/.agents/skillator.yaml` using the same version 1 desired-state shape and strict YAML rules as Repository Configuration. User Scope Skill Directory paths SHALL be relative to the user's home directory. The primary first-run directory SHALL use key `agents`, label `User`, and path `.agents/skills`. User Scope Configuration SHALL not be treated as Repository Configuration, tracked by Git, or subject to Repository control-file rules.

Explicit `library rsync` SHALL exchange User Scope desired state across selected hosts. Stored paths and materializations SHALL remain local to each receiving home. Ordinary User Scope commands and first-run behavior SHALL remain machine-local.

#### Scenario: First User Scope configuration
- **WHEN** a user opens the User tab and saves its implicit default directory without an existing User Scope Configuration
- **THEN** Skillator writes canonical `~/.agents/skillator.yaml` with the primary `.agents/skills` directory and the User Scope Enablements selected by the user

## ADDED Requirements

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
