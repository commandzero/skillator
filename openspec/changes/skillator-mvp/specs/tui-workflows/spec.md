## Purpose

Defines the keyboard-oriented Library and Target workspaces that let users curate available Skills and stage repository Enablements without writes until an explicit save.

## ADDED Requirements

### Requirement: The Library uses one hierarchical table
The Library workspace SHALL present one `Status`, `Name`, and `Description` table. Library Locations SHALL be full-width dividers; Sources SHALL be selectable dividers beneath Locations; and Skills SHALL be indented children beneath Sources. Adding a Location or registering a Source SHALL append it within this same inventory rather than move it to another list.

#### Scenario: Multiple Locations and Sources
- **WHEN** discovery finds local content and Git Sources across several Locations
- **THEN** one table preserves the Location, Source, and Skill hierarchy without repeating Source identity on each Skill row

### Requirement: Library selection separates Source and Skill registration
Skill rows SHALL show Registered, valid Unregistered, or non-selectable Invalid status. A Source checkbox SHALL roll up valid child Skill registration and toggle all or none of those Skills, excluding Invalid Skills. A separate registration action SHALL register or unregister the selected Source so Source identity and Skill curation remain distinct.

#### Scenario: Partial Source registration
- **WHEN** only some valid Skills in a Source are selected
- **THEN** the Source divider shows a mixed rollup while the Source's own registration state remains independent

### Requirement: The Library inspector exposes contextual diagnostics
Selecting a Location, Source, or Skill SHALL expose relevant details in a contextual inspector, including original and resolved paths, Source kind and key, Git facts, registration state, validation errors, unavailability, and overlap warnings. Unavailable registrations and Invalid Skills SHALL remain visible rather than disappear.

#### Scenario: Unavailable Registered Source
- **WHEN** a Registered Source is missing locally
- **THEN** its row and identity remain visible with an unavailable diagnostic in the inspector

### Requirement: The Target shows one Skill Directory at a time
The Target workspace SHALL show a horizontally selectable strip of configured Skill Directories and one table for the selected directory with columns `Enabled`, `Mode`, `Skill`, `Description`, and `State`. Source dividers SHALL be selectable and sorted by Source Key; Registered valid Skills and preserved Unresolved Enablements SHALL appear as indented child rows. Unregistered and Invalid Skills SHALL remain in the Library workspace.

#### Scenario: Directory switch
- **WHEN** the user presses `Tab` or `Shift+Tab`
- **THEN** the selected Skill Directory changes while table-row focus remains stable where possible

### Requirement: Target bulk actions preserve intended modes
Source rows SHALL show tri-state enabled rollups and child counts. Toggling a mixed or disabled Source SHALL enable every currently available child, including filtered or collapsed children, while preserving modes already assigned and using `link` for newly enabled Skills. Toggling an all-enabled Source SHALL disable every child, including preserved Unresolved Enablements. An unavailable Skill MUST NOT receive a new Enablement.

#### Scenario: Bulk enable mixed Source
- **WHEN** a Source contains enabled, disabled, filtered, and collapsed valid Skills
- **THEN** toggling its divider enables all available children, preserves existing modes, and assigns `link` only to newly enabled children

### Requirement: Desired and observed state remain distinguishable
Checkboxes SHALL represent staged desired state. Mode SHALL display compact `link` or `copy`. State SHALL distinguish at least In Sync, Missing, Diverged Copy, Unresolved, and staged Enable, Disable, or Convert outcomes. Non-Skill directory diagnostics SHALL appear as selectable diagnostics above the Skill rows rather than as fake Skills.

#### Scenario: Diverged copy selected
- **WHEN** the user selects a Diverged Copy row
- **THEN** the table shows desired Copy mode and divergent observed state while the inspector explains the conflict and available action

### Requirement: Target navigation follows the approved key contract
The Target workspace SHALL support `j/k` for rows, `J/K` for Sources, `h/l` to collapse or expand Sources, `Space` to toggle, `m` to switch Link or Copy, `Tab/Shift+Tab` for Skill Directories, `/` to filter, `Esc` to clear or close, `s` for confirmed Save and Exit, `Ctrl+S` for safe fast Save and Exit, `q` to quit, `t` to change Target, `a/e/d` to add/edit/delete a Skill Directory, `Ctrl+L` to toggle Library and Target workspaces, and `?` for help.

#### Scenario: Filtering collapsed Sources
- **WHEN** a filter matches children inside collapsed Sources
- **THEN** matching children are temporarily visible and clearing the filter restores prior collapse state

#### Scenario: Group navigation
- **WHEN** the user presses `J` or `K`
- **THEN** selection moves between Source dividers and skips directory diagnostics and individual Skills

### Requirement: Skill Directory edits use one validated overlay
Adding, editing, and deleting Skill Directories SHALL use one compact overlay with the Generic/Codex preset, Claude preset, and a custom option for addition. The same configuration validation and collision rules SHALL apply before save. First run SHALL use the normal Target workspace with the Generic/Codex directory staged, not a separate wizard.

#### Scenario: Existing recognized path
- **WHEN** first run detects a recognized agent path not yet configured
- **THEN** Skillator presents it as an unchecked recommendation and does not activate it automatically

### Requirement: Workspace and Target changes do not carry staged edits
Switching Target or toggling between Target and Library while edits are staged SHALL offer only Discard and Continue or Return to Editing. Staged edits MUST NOT follow the user into another Target or workspace.

#### Scenario: Toggle Library with staged Target edits
- **WHEN** the user presses `Ctrl+L` after changing Enablements
- **THEN** Skillator requires discard or return and performs no write unless the user separately saves

### Requirement: All interactive edits remain staged until save
Checking, unchecking, changing modes, registration changes, and directory edits SHALL remain in memory until an explicit save. Quitting or discarding SHALL leave configuration and Materializations unchanged. Library save SHALL write only Library configuration; Target save SHALL validate and write complete Repository Configuration before reconciliation.

#### Scenario: Quit after staging Target changes
- **WHEN** a user stages Enablement edits and quits without saving
- **THEN** Repository Configuration and filesystem Materializations remain unchanged

### Requirement: Save confirmation respects safety classification
Pressing `s` SHALL always show a confirmation, even for a clean or Safe-only plan. `Ctrl+S` SHALL skip confirmation only when every planned change is Safe. Any Guarded plan SHALL show one complete batch confirmation with only Proceed with all Guarded Changes or Return to Editing. Blocked Changes SHALL be listed but MUST NOT be authorizable.

#### Scenario: Fast save with Guarded work
- **WHEN** the user presses `Ctrl+S` and the prepared plan contains a Guarded Change
- **THEN** Skillator shows the same guarded batch confirmation instead of bypassing it

### Requirement: Busy and partial outcomes preserve user understanding
An active Target lock SHALL not prevent browsing or staging; Save SHALL report Target Busy with Retry or Return to Editing and MUST NOT discard edits or retry automatically. A fully successful save SHALL exit immediately. A partial or failed save SHALL remain on a concise result screen until acknowledged, identify applied, blocked, rolled-back, or Recovery Required work, and then exit nonzero.

#### Scenario: Partial save
- **WHEN** a confirmed save applies some changes while others remain blocked
- **THEN** the TUI presents the concise partial result until acknowledgement and exits with status `1`

### Requirement: Invalid configuration is diagnostic-only
Invalid or unsupported Repository or Library configuration SHALL open a read-only diagnostic screen that preserves the document exactly and provides no embedded YAML editor or save path.

#### Scenario: Unsupported Repository version
- **WHEN** the Target TUI opens a repository with unsupported Repository Configuration
- **THEN** it shows the version diagnostic, allows a normal read-only exit, and performs no writes
