## ADDED Requirements

### Requirement: Library Locations can be edited without the TUI
`skillator library add <location>` SHALL add one Library Location while preserving the supplied path expression and applying existing resolution, validation, overlap, and stale-write rules. It SHALL register the Location without acquiring or enabling any Skill. `skillator library remove <location>` SHALL remove exactly one configured Location without deleting its directory or removing Enablements that depend on it. `skillator library locations` SHALL report configured expressions and their current resolution state.

#### Scenario: Add a new Location
- **WHEN** the user adds a valid non-overlapping Location
- **THEN** Skillator saves it and leaves every discovered Skill inactive

#### Scenario: Add an existing Location
- **WHEN** the supplied expression resolves to an already configured canonical Location
- **THEN** Skillator reports `unchanged` without adding a duplicate

#### Scenario: Remove a Location with dependent Enablements
- **WHEN** the user removes a Location that supplies Skills named by User Scope or available registered Target Enablements
- **THEN** Skillator unregisters the Location, preserves those Enablements, and reports their scope and canonical Skill identity as unresolved

#### Scenario: Remove does not delete content
- **WHEN** a Library Location is removed successfully
- **THEN** Skillator leaves the Location directory and its contents unchanged

### Requirement: Live Library inventory has an optional Source filter
`skillator library list [filter]` SHALL list discovered Skills grouped by Source. With no filter it SHALL include every discovered Source. A filter SHALL match Source Keys case-insensitively from the beginning, so `elastic`, `mattpocock`, and `elastic/agent-skills` can select the corresponding owner or complete Source Key. Filtering SHALL NOT search Skill names or descriptions. Results SHALL include canonical Source Key, Skill path, display name, validity, and relevant diagnostics in deterministic order.

#### Scenario: Owner filter
- **WHEN** the user runs `skillator library list elastic`
- **THEN** the result includes discovered Sources whose keys begin with `elastic/` and excludes nonmatching Sources

#### Scenario: Complete Source filter
- **WHEN** the user runs `skillator library list elastic/agent-skills`
- **THEN** the result includes that Source and its discovered Skills

#### Scenario: Empty filtered result
- **WHEN** no discovered Source Key begins with the supplied filter
- **THEN** Skillator emits a successful empty result

#### Scenario: Invalid discovered Skill
- **WHEN** a matching Source contains an Invalid Skill
- **THEN** the listing includes the Skill with its invalid state and diagnostics
