## Purpose

Defines first-run Library initialization, safe import of existing user-scoped Skills, and machine-local User Scope desired state inherited by repository views.

## ADDED Requirements

### Requirement: User Scope Configuration is strict and machine-local
Skillator SHALL read and write User Scope Configuration at `~/.agents/skillator.yaml` using the same version 1 desired-state shape and strict YAML rules as Repository Configuration. User Scope Skill Directory paths SHALL be relative to the user's home directory. The primary first-run directory SHALL use key `agents`, label `User`, and path `.agents/skills`. User Scope Configuration SHALL not be treated as Repository Configuration, tracked by Git, or subject to Repository control-file rules.

#### Scenario: First User Scope configuration
- **WHEN** onboarding completes successfully without an existing User Scope Configuration
- **THEN** Skillator writes canonical `~/.agents/skillator.yaml` with the primary `.agents/skills` directory and Enablements for successfully imported or registered Skills

### Requirement: Onboarding inventories existing user-scoped entries
Onboarding SHALL inspect each immediate child of `~/.agents/skills` without following symbolic links. Valid physical Skill directories SHALL be preselected for import. Invalid, unreadable, unsupported, or non-Skill entries SHALL remain unselected and untouched with a reason. Existing symbolic links SHALL remain unchanged and, when their canonical referent is a valid Skill, Skillator SHALL offer the enclosing Git Source or local Source Location for Library registration.

#### Scenario: Existing physical and linked Skills
- **WHEN** `~/.agents/skills` contains a valid physical Skill, a valid Skill symlink, and an unrelated file
- **THEN** onboarding preselects the physical Skill for import, offers registration for the symlink Source without moving its referent, and leaves the unrelated file untouched

### Requirement: Import destinations are explicit and collision-safe
Each selected physical Skill SHALL be imported beneath the selected first Library Location using its validated Skill name. Physical Skills SHALL default to `move`; the user MAY choose `copy` to retain physical content in both places or `link` to retain the original and create a local-Library symlink. A moved Skill SHALL be linked back into User Scope, while copied or Library-linked content SHALL retain the existing physical User Scope entry and record a copied User Scope Materialization. Onboarding SHALL show every source, destination, mode, and resulting action before confirmation. Existing destinations, duplicate names, Source Key collisions, escaping paths, or uninspectable entries SHALL block the affected import and MUST NOT be overwritten or automatically renamed.

#### Scenario: Library destination collision
- **WHEN** the selected Library Location already contains an entry at an imported Skill's destination
- **THEN** onboarding blocks that import, preserves both existing entries, and requires the user to resolve the collision

### Requirement: Final onboarding confirmation is transactional
Before final confirmation, onboarding MUST perform no configuration, directory, move, copy, removal, or symlink writes. After confirmation, Skillator SHALL stage and verify every destination and configuration before displacing original content. It SHALL preserve recoverable originals until all selected imports, registrations, User Scope links, Library Configuration, and User Scope Configuration are published. Any failure SHALL restore the pre-onboarding state; failed rollback SHALL retain exact recovery paths and report Recovery Required. Transactional behavior does not claim power-loss durability.

#### Scenario: Failure after one Skill is staged
- **WHEN** a later selected import fails after earlier work has been staged or published
- **THEN** Skillator restores the original `~/.agents/skills` entries and prior configuration files, or reports exact retained recovery artifacts if restoration cannot complete

### Requirement: Successful onboarding opens the current Target
After a successful final confirmation from a Git worktree, Skillator SHALL open the first Repository Skill Directory of the current Target workspace. Successfully imported physical Skills SHALL be Linked from `~/.agents/skills/<name>` to their canonical Library destinations. Existing valid symlinks SHALL retain their stored link text and filesystem identity while their registered desired state is recorded. Additional home-scoped Skill Directories MAY be added later and SHALL appear as `User · <label>` tabs.

#### Scenario: Default onboarding succeeds
- **WHEN** the user confirms valid default onboarding from a Git repository
- **THEN** Skillator creates the Library and User Scope configuration, links imported Skills back into `~/.agents/skills`, and opens the first Repository Skill Directory

### Requirement: User Scope reconciliation is not Git reconciliation
User Scope save SHALL reconcile Linked or Copied Materializations under configured home-relative Skill Directories using the same containment, staging, confirmation, rollback, and recovery principles as Repository reconciliation, but SHALL not create `.gitignore` control files, inspect Git tracking, or mutate any Git index. Repository tabs SHALL derive inherited User Enablements from the saved User Scope configuration and current User Scope observation.

#### Scenario: Saving a User Enablement
- **WHEN** the user enables a registered Skill from the User tab and confirms save
- **THEN** Skillator writes User Scope desired state and materializes the selected mode without creating a Skill Directory control file
