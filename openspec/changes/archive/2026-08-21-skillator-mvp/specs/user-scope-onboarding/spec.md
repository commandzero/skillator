## Purpose

Defines machine-local User Scope desired state inherited by repository views. User Scope is a Target scope; it is not Library inventory and is never an automatic first-run workflow.

## ADDED Requirements

### Requirement: User Scope Configuration is strict and machine-local
Skillator SHALL read and write User Scope Configuration at `~/.agents/skillator.yaml` using the same version 1 desired-state shape and strict YAML rules as Repository Configuration. User Scope Skill Directory paths SHALL be relative to the user's home directory. The primary first-run directory SHALL use key `agents`, label `User`, and path `.agents/skills`. User Scope Configuration SHALL not be treated as Repository Configuration, tracked by Git, or subject to Repository control-file rules.

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
