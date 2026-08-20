## Purpose

Defines the user-scoped Library that discovers local Skill Sources while keeping registration curated, identity portable, and machine-local paths outside repository configuration.

## ADDED Requirements

### Requirement: Library configuration is strict and user-scoped
Skillator SHALL read and write one Library configuration at `~/.skillator/library.yaml`. The MVP SHALL accept only a single YAML document with numeric `version: 1`, a required `locations` list, and the documented fields for Locations, Registered Sources, and Registered Skills. Unknown fields, duplicate keys, unsupported versions, missing required values, invalid types, or failed structural validation MUST yield no partially trusted Library and MUST prevent Library or reconciliation writes.

#### Scenario: Valid Library configuration loads
- **WHEN** `~/.skillator/library.yaml` contains a structurally valid version 1 document
- **THEN** Skillator loads one logical Library and resolves its registered identities from that document

#### Scenario: Unsupported Library version is preserved
- **WHEN** the Library document declares a version other than `1`
- **THEN** Skillator diagnoses the encountered and supported versions, preserves the document byte-for-byte, and performs no configuration or reconciliation writes

### Requirement: First run enters Library onboarding
When Library configuration is absent, any root TUI invocation SHALL enter the Library onboarding workflow before loading a Repository workspace. Onboarding SHALL stage the first Library Location as a selected table row with editable expression `./library` relative to `library.yaml`, display its resolved default as `~/.skillator/library`, and stage registration of its local Source under `local/library`. It MUST NOT open a blocking path editor on entry; editing the staged default SHALL be an explicit action from the Location row. No file or directory SHALL be created until final confirmation.

#### Scenario: First-run save
- **WHEN** a user confirms onboarding with the default first Library Location
- **THEN** Skillator creates `~/.skillator/library.yaml`, `~/.skillator/library/`, and the registered `local/library` Source before opening the User Scope tab

#### Scenario: First-run cancellation
- **WHEN** a user exits onboarding without confirming
- **THEN** Skillator leaves the filesystem unchanged

### Requirement: Locations resolve machine-local paths
A Library Location path SHALL support expressions relative to `library.yaml`, absolute paths, home-relative paths, and `${VARIABLE}` interpolation. Skillator SHALL preserve the original expression in configuration while using a resolved path without redundant current-directory components for review, discovery, and overlap checks. Existing Locations SHALL use their canonical path for filesystem comparison. Failed expansion or unreadable content SHALL make the Location unavailable with a diagnostic rather than removing its registrations.

#### Scenario: Relative Location resolution
- **WHEN** a Location uses `./library`
- **THEN** Skillator resolves it relative to the directory containing `~/.skillator/library.yaml` and displays `~/.skillator/library` without an embedded `/./` segment

#### Scenario: Failed variable expansion
- **WHEN** a Location references an unavailable environment variable
- **THEN** Skillator reports the Location unavailable and retains its configured expression and registered identities

### Requirement: Discovery respects Source boundaries
Skillator SHALL recursively discover Skills beneath every available Location, SHALL always prune entries named `.git`, SHALL not recurse through directory symlinks, and SHALL apply configured Gitignore-style exclusion patterns relative to the Location. As the sole exception needed by `link` acquisition, Skillator SHALL recognize a direct child symlink of the first Location's root as one Skill in `local/library` when that link resolves to a valid Skill, without traversing beyond that one Skill boundary. Each nearest enclosing Git repository, including worktrees, submodules, and repositories represented by a `.git` file, SHALL be a distinct Source. Content outside nested Git Sources SHALL belong to that Location's local Source.

#### Scenario: Mixed local and Git discovery
- **WHEN** a Location contains local Skills and multiple nested Git repositories
- **THEN** Skillator assigns each nested repository its own Source and assigns remaining Skills to the Location's local Source

#### Scenario: Excluded or linked directory
- **WHEN** a candidate subtree is excluded by the Location or reached through a directory symlink
- **THEN** Skillator does not traverse that subtree during discovery

### Requirement: Overlapping Locations require deliberate authorization
Skillator SHALL detect canonical exact and ancestor-descendant overlaps between configured Locations, warn about them, and reject saving the overlap by default. An explicit override MAY retain both Locations as distinct discovery boundaries, in which case affected Enablements SHALL carry an advisory overlap warning.

#### Scenario: Overlap rejected by default
- **WHEN** a user adds a Location nested beneath another configured Location and does not override the warning
- **THEN** Skillator refuses to save the overlapping configuration

### Requirement: Source identity is explicit and portable
A discovered Source SHALL remain Unregistered until explicitly accepted. Registration SHALL persist an immutable, canonical lowercase, slash-separated Source Key independent of local path and future Git remote changes. Git Sources SHALL suggest `owner/repository` from `origin`; non-Git Sources SHALL suggest `local/name`. A suggested-key collision SHALL require a deliberate alternate key rather than automatic suffixing.

#### Scenario: Git Source registration
- **WHEN** a user registers a discovered Git repository with origin `elastic/agent-skills`
- **THEN** Skillator suggests `elastic/agent-skills`, persists the accepted key, and does not later change it when the path or remote changes

#### Scenario: Source Key collision
- **WHEN** a suggested Source Key equals an existing key case-insensitively
- **THEN** Skillator stops registration and requires the user to provide a distinct canonical key

### Requirement: Skill registration is independent from Source registration
A Skill SHALL be identified by its Source Key plus slash-normalized directory path relative to the Source. Registering a Source SHALL not automatically register all Skills, though the interactive workflow SHALL offer a deliberate select-all action. Only Registered, valid Skills SHALL appear as choices for new Enablements. Invalid Skills SHALL remain visible with diagnostics but MUST NOT receive new registrations or Enablements.

#### Scenario: Newly discovered Skill
- **WHEN** discovery finds a valid Skill in a Registered Source
- **THEN** the Skill appears Unregistered until the user explicitly selects it

#### Scenario: Invalid Skill
- **WHEN** a discovered directory has missing or invalid required `SKILL.md` metadata
- **THEN** Skillator displays its validation diagnostic but prevents registration and new Enablement

### Requirement: Registrations survive local unavailability
Missing or unreadable Registered Sources and Skills SHALL retain their Source Keys, Skill Keys, and registration state as Unavailable. A moved Registered Skill SHALL remain Unavailable at its old Skill Key and appear as an Unregistered Skill at its new path. Non-interactive synchronization SHALL report newly discovered or unavailable content but MUST NOT change registration.

#### Scenario: Source missing on another machine
- **WHEN** a Registered Source path is absent on the current machine
- **THEN** Skillator retains the Source and Skill registrations as Unavailable and leaves Target references unresolved

#### Scenario: Registered Skill moved
- **WHEN** a Skill directory moves within its Source
- **THEN** Skillator keeps the old Skill Key unavailable and presents the new relative path as an unregistered Skill

### Requirement: Unregistration does not rewrite Targets
Unregistering a Source or Skill SHALL update only Library configuration after explicit confirmation. Skillator SHALL identify known affected Target references when practical, MUST NOT rewrite Repository Configuration, and SHALL allow those existing references to become Unresolved Enablements.

#### Scenario: Registered Skill removed from Library
- **WHEN** the user confirms unregistration of a Skill referenced by a Target Repository
- **THEN** Skillator removes only the Library registration and preserves the Target's declarative Enablement unchanged

### Requirement: The local Library accepts explicit acquisition modes
The first configured Library Location's `local/library` Source SHALL be the only acquisition destination. Valid Skills in additional Locations MAY be acquired into that local Library with mode `move`, `copy`, or `link`; Skillator MUST NOT acquire content into additional Locations. `move` SHALL be the default and preferred mode for a newly selected external Skill, SHALL transfer the physical Skill into the local Library, and SHALL remove the original only after the destination is verified. `copy` SHALL publish a verified physical duplicate while preserving the original. `link` SHALL publish a symbolic link in the local Library to the canonical original. A blank mode SHALL register the Skill in place without acquisition.

#### Scenario: External Skill moved into the local Library
- **WHEN** a user selects a valid Skill from an additional Location with default `move` mode and confirms Save
- **THEN** Skillator publishes and registers the Skill beneath the first Location's `local/library` Source and removes the original only after verified publication

#### Scenario: External Skill copied or linked
- **WHEN** a user chooses `copy` or `link` for a valid Skill from an additional Location and confirms Save
- **THEN** Skillator creates the selected representation beneath the local Library and preserves the original Skill

### Requirement: Library acquisition preserves content on failure
Library acquisition SHALL validate the local destination and Source immediately before mutation, reject collisions without replacement, stage copied content on the destination filesystem, retain recoverable originals until configuration publication succeeds, and roll back every acquisition in the confirmed batch when any acquisition or Library Configuration save fails. Source-root Git Skills and unsupported or uninspectable entries SHALL be Blocked rather than moving an enclosing repository implicitly.

#### Scenario: Acquisition destination collision
- **WHEN** the local Library already contains the selected Skill name
- **THEN** Skillator leaves both source and destination untouched and reports the blocked acquisition
