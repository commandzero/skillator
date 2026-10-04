# Spec Delta

## MODIFIED Requirements

### Requirement: Missing inventory leaves declarations unresolved
Missing or unreadable Sources and Skills SHALL be absent from the live Snapshot. Existing Enablements SHALL retain their Skill Keys and become unresolved until discovery finds matching content again. Ordinary Target and worktree synchronization MUST NOT create Library inventory entries. Explicit `library rsync` SHALL deliver leader skill content into a dedicated follower replica, by leader push or follower pull, without creating or merging Library Location registrations. Incomplete leader inventory SHALL NOT authorize mirror deletion; both directions SHALL require complete usable leader input.

#### Scenario: Source missing on another machine
- **WHEN** a configured Location is absent on the current machine
- **THEN** Skillator reports the Location unavailable and leaves matching Target references unresolved

#### Scenario: Skill moved
- **WHEN** a Skill directory moves within its Source
- **THEN** the old Enablement becomes unresolved and the new relative path appears as a newly discovered Skill

### Requirement: The local Library accepts explicit acquisition modes
The first configured Library Location's local Source SHALL be the only acquisition destination. Valid Skills in additional Locations MAY be acquired into that local Library with mode `move`, `copy`, or `link`; Skillator MUST NOT acquire content into additional Locations. `move` SHALL be the default and preferred mode when the user explicitly selects acquisition, SHALL transfer the physical Skill into the local Library, and SHALL remove the original only after the destination is verified. `copy` SHALL publish a verified physical duplicate while preserving the original. `link` SHALL publish a symbolic link in the local Library to the canonical original. A blank mode leaves the live Skill in place.

These local acquisition modes SHALL remain independent of `library rsync`, which delivers leader skill files into a dedicated follower replica by push or pull without changing acquisition choices or either host's Library Configuration.

#### Scenario: External Skill moved into the local Library
- **WHEN** a user selects a valid Skill from an additional Location with default `move` mode and confirms Save
- **THEN** Skillator publishes the Skill beneath the first Location's local Source and removes the original only after verified publication

#### Scenario: External Skill copied or linked
- **WHEN** a user chooses `copy` or `link` for a valid Skill from an additional Location and confirms Save
- **THEN** Skillator creates the selected representation beneath the local Library and preserves the original Skill

