# library-management Specification

## Purpose
Defines the user-scoped Library that discovers local Skill Sources live from configured Locations while keeping identity portable and machine-local paths outside repository configuration.
## Requirements
### Requirement: Library configuration is strict and user-scoped
Skillator SHALL read and write one Library configuration at `~/.skillator/library.yaml`. The MVP SHALL accept only a single YAML document with numeric `version: 1`, a required `locations` list, and the documented Location fields. Source and Skill inventory MUST NOT be persisted in Library configuration. Unknown fields, duplicate keys, unsupported versions, missing required values, invalid types, or failed structural validation MUST yield no partially trusted Library and MUST prevent Library or reconciliation writes.

#### Scenario: Valid Library configuration loads
- **WHEN** `~/.skillator/library.yaml` contains a structurally valid version 1 document
- **THEN** Skillator loads one logical Library and discovers Sources and Skills from its Locations

#### Scenario: Unsupported Library version is preserved
- **WHEN** the Library document declares a version other than `1`
- **THEN** Skillator diagnoses the encountered and supported versions, preserves the document byte-for-byte, and performs no configuration or reconciliation writes

### Requirement: First run opens the normal Library workspace
When Library configuration is absent, any root TUI invocation SHALL open the normal Library workspace before loading a Repository workspace. It SHALL show a welcome modal titled `I AM SKILLATOR!` explaining that the user must configure the Library before using `Ctrl+L` to manage the current Target. The normal Library table SHALL stage the first Location as selected with editable expression `./library` relative to `library.yaml`, display its resolved default as `~/.skillator/library`, and discover its local Source as `local/library`. It MUST NOT open a blocking path editor on entry; editing the staged default SHALL be an explicit action from the Location row. No file or directory SHALL be created until final confirmation.

#### Scenario: First-run save
- **WHEN** a user confirms the normal Library save with the default first Library Location
- **THEN** Skillator creates `~/.skillator/library.yaml` and `~/.skillator/library/`

#### Scenario: First-run cancellation
- **WHEN** a user exits the welcome or normal Library workspace without confirming
- **THEN** Skillator leaves the filesystem unchanged

### Requirement: Locations resolve machine-local paths
A Library Location path SHALL support expressions relative to `library.yaml`, absolute paths, home-relative paths, and `${VARIABLE}` interpolation. Skillator SHALL preserve the original expression in configuration while using a resolved path without redundant current-directory components for review, discovery, and overlap checks. Existing Locations SHALL use their canonical path for filesystem comparison. Failed expansion or unreadable content SHALL make the Location unavailable with a diagnostic; its live inventory is absent until it can be discovered again.

#### Scenario: Relative Location resolution
- **WHEN** a Location uses `./library`
- **THEN** Skillator resolves it relative to the directory containing `~/.skillator/library.yaml` and displays `~/.skillator/library` without an embedded `/./` segment

#### Scenario: Failed variable expansion
- **WHEN** a Location references an unavailable environment variable
- **THEN** Skillator reports the Location unavailable and retains its configured expression

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

### Requirement: Source identity is discovered and portable
Each discovery pass SHALL derive a canonical lowercase, slash-separated Source Key. Git Sources SHALL derive `owner/repository` from `origin`; non-Git Sources SHALL derive `local/name`. A suggested-key collision SHALL be surfaced rather than automatically suffixed or silently resolved.

#### Scenario: Git Source discovery
- **WHEN** discovery finds a Git repository with origin `elastic/agent-skills`
- **THEN** Skillator identifies it as `elastic/agent-skills` for that Snapshot

#### Scenario: Source Key collision
- **WHEN** a suggested Source Key equals an existing key case-insensitively
- **THEN** Skillator keeps both discovered rows visible with a collision diagnostic and does not offer ambiguous new Enablements

### Requirement: Skill inventory is live
A Skill SHALL be identified by its Source Key plus slash-normalized directory path relative to the Source. Every discovered valid Skill SHALL appear as a choice for new Enablements on the next Library Snapshot. Invalid Skills SHALL remain visible with diagnostics but MUST NOT receive new Enablements.

#### Scenario: Newly discovered Skill
- **WHEN** a user adds a valid Skill directory beneath a configured Library Location
- **THEN** the Skill appears in the Library and Target inventories on the next discovery pass without a Library Configuration edit

#### Scenario: Invalid Skill
- **WHEN** a discovered directory has missing or invalid required `SKILL.md` metadata
- **THEN** Skillator displays its validation diagnostic but prevents a new Enablement

### Requirement: Missing inventory leaves declarations unresolved
Missing or unreadable Sources and Skills SHALL be absent from the live Snapshot. Existing Enablements SHALL retain their Skill Keys and become unresolved until discovery finds matching content again. Non-interactive synchronization MUST NOT create Library inventory entries.

#### Scenario: Source missing on another machine
- **WHEN** a configured Location is absent on the current machine
- **THEN** Skillator reports the Location unavailable and leaves matching Target references unresolved

#### Scenario: Skill moved
- **WHEN** a Skill directory moves within its Source
- **THEN** the old Enablement becomes unresolved and the new relative path appears as a newly discovered Skill

### Requirement: The local Library accepts explicit acquisition modes
The first configured Library Location's local Source SHALL be the only acquisition destination. Valid Skills in additional Locations MAY be acquired into that local Library with mode `move`, `copy`, or `link`; Skillator MUST NOT acquire content into additional Locations. `move` SHALL be the default and preferred mode when the user explicitly selects acquisition, SHALL transfer the physical Skill into the local Library, and SHALL remove the original only after the destination is verified. `copy` SHALL publish a verified physical duplicate while preserving the original. `link` SHALL publish a symbolic link in the local Library to the canonical original. A blank mode leaves the live Skill in place.

#### Scenario: External Skill moved into the local Library
- **WHEN** a user selects a valid Skill from an additional Location with default `move` mode and confirms Save
- **THEN** Skillator publishes the Skill beneath the first Location's local Source and removes the original only after verified publication

#### Scenario: External Skill copied or linked
- **WHEN** a user chooses `copy` or `link` for a valid Skill from an additional Location and confirms Save
- **THEN** Skillator creates the selected representation beneath the local Library and preserves the original Skill

### Requirement: Library acquisition preserves content on failure
Library acquisition SHALL validate the local destination and Source immediately before mutation, reject collisions without replacement, stage copied content on the destination filesystem, retain recoverable originals until configuration publication succeeds, and roll back every acquisition in the confirmed batch when any acquisition or Library Configuration save fails. Source-root Git Skills and unsupported or uninspectable entries SHALL be Blocked rather than moving an enclosing repository implicitly.

#### Scenario: Acquisition destination collision
- **WHEN** the local Library already contains the selected Skill name
- **THEN** Skillator leaves both source and destination untouched and reports the blocked acquisition

