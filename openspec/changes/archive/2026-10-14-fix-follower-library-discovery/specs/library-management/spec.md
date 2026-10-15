## ADDED Requirements

### Requirement: Implicit Library discovery is shared across scopes
When Library configuration is absent, inventory discovery, User and Repository saves, selector validation, and materialization observation SHALL discover the implicit `~/.skillator/library` Location without persisting Library configuration. Explicit Library configuration SHALL take precedence, including an empty Locations list. Library registration mutations, repository updates, and leader exports SHALL retain their explicit configuration requirements.

#### Scenario: First user activation from a delivered replica
- **WHEN** an owned replica exists below the default Location and both Library and User configuration are absent
- **THEN** a Skill shown in the User TUI can be linked or copied and saved without first saving Library configuration
- **AND** its saved selector remains resolved during subsequent inspection and reconciliation, with Library configuration still absent

#### Scenario: Explicit empty Library
- **WHEN** Library configuration contains an empty Locations list despite content below the default Location
- **THEN** discovery does not use the implicit default and saved selectors remain unresolved

## MODIFIED Requirements

### Requirement: Discovery respects Source boundaries
Skillator SHALL recursively discover Skills beneath every available Location, SHALL always prune entries named `.git`, SHALL not recurse through directory symlinks, and SHALL apply configured Gitignore-style exclusion patterns relative to the Location. As the sole exception needed by `link` acquisition, Skillator SHALL recognize a direct child symlink of the first Location's root as one Skill in `local/library` when that link resolves to a valid Skill, without traversing beyond that one Skill boundary. Each nearest enclosing Git repository, including worktrees, submodules, and repositories represented by a `.git` file, SHALL be a distinct Source. Content outside nested Git Sources SHALL belong to that Location's local Source.

#### Scenario: Mixed local and Git discovery
- **WHEN** a Location contains local Skills and multiple nested Git repositories
- **THEN** Skillator assigns each nested repository its own Source and assigns remaining Skills to the Location's local Source

#### Scenario: Excluded or linked directory
- **WHEN** a candidate subtree is excluded by the Location or reached through a directory symlink
- **THEN** Skillator does not traverse that subtree during discovery

#### Scenario: Owned replica discovery
- **WHEN** a registered Location is an owned rsync replica or contains one
- **THEN** Skillator discovers each `<SourceKey>/_skills` subtree as a separate non-Git Source, preserves the leader Source Key and original Skill paths, and does not flatten replica skills into an enclosing local Source
- **AND** discovery applies Location exclusions, refuses invalid ownership markers with diagnostics, does not traverse linked source containers, and keeps real Source Key collisions unresolved
- **AND** an empty enclosing local Source does not collide with a delivered Source of the same name

### Requirement: Source identity is discovered and portable
Each Snapshot SHALL use canonical lowercase, slash-separated Source Keys. Git Sources SHALL derive `owner/repository` from `origin`; ordinary non-Git Sources SHALL derive `local/name`. Owned replica Sources SHALL retain the leader Source Key from the replica layout, with Skill paths relative to `_skills`. Key collisions SHALL be diagnosed, never automatically suffixed or silently resolved.

#### Scenario: Git Source discovery
- **WHEN** discovery finds a Git repository with origin `elastic/agent-skills`
- **THEN** Skillator identifies it as `elastic/agent-skills` for that Snapshot

#### Scenario: Source Key collision
- **WHEN** a suggested Source Key equals an existing key case-insensitively
- **THEN** Skillator keeps both discovered rows visible with a collision diagnostic and does not offer ambiguous new Enablements
