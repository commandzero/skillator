## Purpose

Defines the keyboard-oriented onboarding, Library, User Scope, and Target Repository workflows that let users curate available Skills and stage Enablements without writes until an explicit save.

## ADDED Requirements

### Requirement: The Library uses one hierarchical table
The Library workspace SHALL present one unlabeled checkbox column followed by `Mode`, `Location`, `Description`, and `Action`. Library Locations SHALL be full-width dividers; Sources SHALL be selectable dividers beneath Locations; and Skills SHALL be indented children beneath Sources. Adding a Location or registering a Source SHALL append it within this same inventory rather than move it to another list. A valid Skill's `Description` SHALL be the `description` value from its `SKILL.md` frontmatter; Location and Source descriptions SHALL be concise metadata summaries. `Description` MUST NOT contain a pending operation. `Action` SHALL describe only work that Save will attempt, SHALL use a concise one-to-three-word phrase, and SHALL remain blank when Save has no work for that row.

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

### Requirement: The TUI uses a consistent 256-color visual hierarchy
The TUI SHALL use indexed 256-color palette values by default. Main workspace borders SHALL be purple, modal and input-overlay borders SHALL be blue, and titles plus persistent hotkey labels SHALL use an off-white bone color. Every modal title SHALL be capitalized, describe the modal's action or purpose rather than repeat the application name, and include one space between the left border and title text. Modal confirmation controls SHALL appear in the bottom border rather than as body text. Warning states SHALL use yellow accents and error states SHALL use red accents. Structural child-tree glyphs, divider lines, and unchecked `[ ]` markers SHALL use a visible dark gray without the terminal dim modifier. The selected row SHALL add only a dark-blue background and MUST NOT replace its existing foreground color.

#### Scenario: Selecting a warning row
- **WHEN** the user selects a row carrying a warning state
- **THEN** its yellow foreground accent remains visible over the dark-blue selection background

#### Scenario: Opening an editor
- **WHEN** the user opens any modal or input overlay
- **THEN** the overlay uses a blue border while the underlying workspace retains its purple border

#### Scenario: Opening a confirmation modal
- **WHEN** the user opens a save, discard, delete, or retry confirmation
- **THEN** its padded capitalized title names the action and its confirmation hotkeys appear only in the bottom border

### Requirement: The Target shows one Skill Directory at a time
The Target workspace SHALL show one flat horizontal strip beginning with the primary `User` Scope tab, followed by any additional tabs labeled `User · <label>`, then Repository Skill Directory tabs. The primary User tab SHALL represent `~/.agents/skills`; additional User tabs SHALL represent other home-relative User Scope Skill Directories. When root `skillator` is launched for a Git Target, it SHALL select the first Repository Skill Directory rather than the User tab; if no Repository Skill Directory exists, it SHALL select the User tab. One table for the selected tab SHALL use an unlabeled checkbox column followed by `Mode`, `Skill`, `Description`, and `Action`. The `Mode` column SHALL contain compact `link`, `copy`, or inherited `user` values. `Description` SHALL remain Skill metadata rather than action text. `Action` SHALL contain only work Save will attempt and SHALL be blank for rows requiring no change. Source dividers SHALL be selectable and sorted by Source Key; Registered valid Skills and preserved Unresolved Enablements SHALL appear as indented child rows. Unregistered and Invalid Skills SHALL remain in the Library workspace.

#### Scenario: Launch from a Git Target
- **WHEN** the user launches root `skillator` from a Git worktree with one or more Repository Skill Directories
- **THEN** the first Repository Skill Directory is selected while the User tabs remain visible before it in the tab strip

#### Scenario: Directory switch
- **WHEN** the user presses `Tab` or `Shift+Tab`
- **THEN** the selected User or Repository Skill Directory changes while table-row focus remains stable where possible

### Requirement: User Scope inheritance is explicit and read-only in Repository tabs
On a User Scope tab, desired Enablements SHALL remain editable and display ordinary `[x] link` or `[x] copy` state. On a Repository tab, a Skill active only through User Scope SHALL display `[u] user`, SHALL not persist a Repository Enablement, and SHALL be read-only. Attempting to toggle or change mode on such a row SHALL direct the user to its User tab. When the same Skill also has an explicit Repository Enablement, the explicit `[x] link` or `[x] copy` state SHALL remain visible with an `also active in User Scope` warning.

#### Scenario: Inherited User Skill
- **WHEN** a Skill is enabled in User Scope but has no Enablement in the selected Repository Skill Directory
- **THEN** its Repository row displays `[u] user` and cannot be changed from that tab

#### Scenario: Explicit and inherited Skill
- **WHEN** the same Skill is enabled in User Scope and explicitly enabled in the selected Repository Skill Directory
- **THEN** its Repository row displays the explicit Repository mode and warns that the Skill is also active in User Scope

### Requirement: Target bulk actions preserve intended modes
Source rows SHALL show tri-state enabled rollups and child counts. Toggling a mixed or disabled Source SHALL enable every currently available child, including filtered or collapsed children, while preserving modes already assigned and using `link` for newly enabled Skills. Toggling an all-enabled Source SHALL disable every child, including preserved Unresolved Enablements. An unavailable Skill MUST NOT receive a new Enablement.

#### Scenario: Bulk enable mixed Source
- **WHEN** a Source contains enabled, disabled, filtered, and collapsed valid Skills
- **THEN** toggling its divider enables all available children, preserves existing modes, and assigns `link` only to newly enabled children

### Requirement: Desired, observed, and pending action remain distinguishable
Checkboxes SHALL represent staged desired state. Mode SHALL display compact `link`, `copy`, `user`, or Library-acquisition `move`. The Action column SHALL distinguish pending Enable, Disable, Convert, Repair, Register, Unregister, Move, Copy, and Link work while remaining blank for no-op rows. Observed states including In Sync, Missing, Diverged Copy, and Unresolved SHALL remain available in the contextual inspector instead of occupying Description or Action. Non-Skill directory diagnostics SHALL appear as selectable diagnostics above the Skill rows rather than as fake Skills. An absent Skill Directory staged during first Repository setup SHALL be ordinary pending Save work and MUST NOT produce an initialization or missing-control-file Diagnostic row; existing malformed, uninspectable, or unexpectedly incomplete directories SHALL retain their diagnostics.

Ordinary table entries SHALL receive the yellow pending-change accent only when their Action is non-empty. Skill names, descriptions, inspector details, and other free-form metadata MUST NOT determine row color. Structured Diagnostic rows SHALL retain warning or error accents, Invalid entries SHALL retain the error accent, and unavailable entries with no pending Action MAY remain structurally dimmed.

#### Scenario: Diverged copy selected
- **WHEN** the user selects a Diverged Copy row
- **THEN** the table shows desired Copy mode and divergent observed state while the inspector explains the conflict and available action

#### Scenario: First Repository setup
- **WHEN** the staged default Skill Directory and its control file do not yet exist
- **THEN** the Target table presents normal staged Save work without an initialization Diagnostic row

#### Scenario: Description contains status-like prose
- **WHEN** an In-Sync Skill has a blank Action and its frontmatter description contains a word such as `unresolved`, `missing`, or `failed`
- **THEN** its row uses the normal foreground because metadata prose is not a warning state

#### Scenario: Skill has a pending action
- **WHEN** an ordinary Skill row has a non-empty Action
- **THEN** its row uses the yellow pending-change accent while the Action remains pending

### Requirement: Target navigation follows the approved key contract
The Target workspace SHALL support `j/k` for rows, `J/K` for Sources, `h/l` to collapse or expand Sources, `Space` to toggle, `m` to switch Link or Copy, `Tab/Shift+Tab` for Skill Directories, `/` to filter, `Esc` to clear or close, `s` for confirmed Save and Exit, `Ctrl+S` for safe fast Save and Exit, `q` to quit, `t` to change Target, `a/e/d` to add/edit/delete a Skill Directory, `Ctrl+L` to toggle Library and Target workspaces, and `?` for help. Plain arrow keys SHALL mirror `h/j/k/l`; Shift+Up and Shift+Down SHALL mirror `K/J` Source movement, while Shift+Left and Shift+Right SHALL retain collapse and expand. Ctrl-modified arrows SHALL remain unmapped. In Library management, `m` SHALL cycle the available acquisition modes. The persistent action legend SHALL identify `s` as Save and Exit and `Ctrl+S` as quick Save, SHALL be right-aligned within the main table's bottom border, and SHALL NOT create a separate horizontal footer rule. The special filters `/pending` and `/pending actions` SHALL show only rows whose Action is non-empty while preserving their containing dividers.

#### Scenario: Filtering collapsed Sources
- **WHEN** a filter matches children inside collapsed Sources
- **THEN** matching children are temporarily visible and clearing the filter restores prior collapse state

#### Scenario: Group navigation
- **WHEN** the user presses `J` or `K`
- **THEN** selection moves between Source dividers and skips directory diagnostics and individual Skills

### Requirement: Skill Directory edits use one validated overlay
Adding, editing, and deleting Skill Directories SHALL use one compact overlay with the Generic/Codex preset, Claude preset, and a custom option for addition. The same configuration validation and collision rules SHALL apply before save. Library first run SHALL use the dedicated onboarding workflow; after onboarding, an absent Repository Configuration SHALL stage the Generic/Codex Repository directory in the normal Target workspace.

#### Scenario: Existing recognized path
- **WHEN** first run detects a recognized agent path not yet configured
- **THEN** Skillator presents it as an unchecked recommendation and does not activate it automatically

#### Scenario: First onboarding screen
- **WHEN** Library onboarding opens with the default first Location staged
- **THEN** Skillator shows the normal onboarding table with `./library` selected, identifies `e` as the location-edit action, and opens the path editor only after that explicit action

### Requirement: Workspace and Target changes do not carry staged edits
Switching Target, toggling between Target and Library, or crossing between User Scope and Repository tabs while edits are staged SHALL offer Save, Discard and Continue, or Return to Editing as appropriate. User Scope and Repository edits SHALL remain separate and MUST NOT be written to the other configuration.

#### Scenario: Toggle Library with staged Target edits
- **WHEN** the user presses `Ctrl+L` after changing Enablements
- **THEN** Skillator requires discard or return and performs no write unless the user separately saves

#### Scenario: Cross configuration tabs with staged edits
- **WHEN** the user attempts to leave a dirty User Scope tab for a Repository tab
- **THEN** Skillator requires save, discard, or return and does not carry those edits into Repository Configuration

#### Scenario: Implicit first-run default directory
- **WHEN** a Repository Configuration is absent and Skillator has only staged its implicit default Skill Directory without an explicit user edit
- **THEN** switching between User and Repository tabs does not require save or discard; returning to the Repository tab retains the implicit default

### Requirement: All interactive edits remain staged until save
Checking, unchecking, changing modes, registration changes, and directory edits SHALL remain in memory until an explicit save. Quitting or discarding SHALL leave configuration and Materializations unchanged. Library save SHALL write only Library configuration, User Scope save SHALL write only `~/.agents/skillator.yaml`, and Repository save SHALL write only the current repository's `.agents/skillator.yaml` before its respective reconciliation.

#### Scenario: Quit after staging Target changes
- **WHEN** a user stages Enablement edits and quits without saving
- **THEN** Repository Configuration and filesystem Materializations remain unchanged

### Requirement: Save confirmation respects safety classification
Pressing `s` SHALL always show a confirmation, even for a clean or Safe-only plan. `Ctrl+S` SHALL skip confirmation only when every planned change is Safe. Confirmation questions and confirmation hotkeys SHALL use the off-white title color. Ordinary desired-action lines SHALL use the normal foreground and MUST NOT be presented as warnings; only actual warning or error content SHALL receive yellow or red semantic accents. Any Guarded plan SHALL show one complete batch confirmation with only Proceed with all Guarded Changes or Return to Editing. Blocked Changes SHALL be listed but MUST NOT be authorizable.

#### Scenario: Fast save with Guarded work
- **WHEN** the user presses `Ctrl+S` and the prepared plan contains a Guarded Change
- **THEN** Skillator shows the same guarded batch confirmation instead of bypassing it

#### Scenario: Reviewing desired onboarding actions
- **WHEN** onboarding asks the user to confirm moves, copies, links, or configuration writes
- **THEN** the question and hotkey prompt are off-white while ordinary desired-action rows retain the normal foreground

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
