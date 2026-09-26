## MODIFIED Requirements

### Requirement: User Scope Configuration is strict and machine-local

Skillator SHALL read and write User Scope Configuration at `~/.agents/skillator.yaml` using the same version 1 desired-state shape and strict YAML rules as Repository Configuration. User Scope Skill Directory paths SHALL be relative to the user's home directory. The primary first-run directory SHALL use key `agents`, label `User`, and path `.agents/skills`. User Scope Configuration SHALL not be treated as Repository Configuration, tracked by Git, or subject to Repository control-file rules.

User Scope desired state and materializations SHALL remain host-local, including during `library rsync`. Library synchronization SHALL NOT merge enablements or reconcile user materializations. Existing links may reflect changes in their library targets, but the command SHALL NOT create, replace, or remove user materialization entries.

#### Scenario: First User Scope configuration

- **WHEN** a user opens the User tab and saves its implicit default directory without an existing User Scope Configuration
- **THEN** Skillator writes canonical `~/.agents/skillator.yaml` with the primary `.agents/skills` directory and the User Scope Enablements selected by the user

#### Scenario: Different host selections remain independent

- **WHEN** hosts with different enablements synchronize library skills
- **THEN** each host retains its own configuration and materialization entries without propagating enablements or deselections

## REMOVED Requirements

### Requirement: Remote synchronization reconciles user selections semantically

**Reason**: Library synchronization is limited to library content.
**Migration**: Use the normal host-local User Scope commands to manage selections.

### Requirement: Received selections use local reconciliation

**Reason**: Library synchronization does not reconcile user materializations.
**Migration**: Use the normal host-local User Scope commands to materialize received skills.
