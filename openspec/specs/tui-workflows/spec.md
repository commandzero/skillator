# tui-workflows Specification

## Purpose
Defines the keyboard-oriented Library, User Scope, and Target Repository workflows that let users curate available Skills and stage Enablements without writes until an explicit save.
## Requirements
### Requirement: The Library uses one hierarchical table
The Library workspace SHALL present one unlabeled checkbox column followed by `Mode`, `Location`, `Description`, and `Action`. Library Locations SHALL be full-width dividers; Sources SHALL be collapsible dividers beneath Locations; and Skills SHALL be indented children beneath Sources. Adding a Location SHALL append its live inventory within this same table rather than move it to another list. A valid Skill's `Description` SHALL be the `description` value from its `SKILL.md` frontmatter; Location and Source descriptions SHALL be concise metadata summaries. `Description` MUST NOT contain a pending operation. `Action` SHALL describe only work that Save will attempt, SHALL use a concise one-to-three-word phrase, and SHALL remain blank when Save has no work for that row.

#### Scenario: Multiple Locations and Sources
- **WHEN** discovery finds local content and Git Sources across several Locations
- **THEN** one table preserves the Location, Source, and Skill hierarchy without repeating Source identity on each Skill row
### Requirement: Library acquisition is explicit
Library inventory is live and has no register or unregister action. `Space` SHALL stage whether a valid Skill is visible in Target inventories without changing its discovered Library membership. A valid external Skill SHALL show no pending acquisition action until the user cycles its acquisition mode with `m`; that mode stages `move`, `copy`, or `link` into the first Library Location.

#### Scenario: Hide a Library Skill from Target inventories
- **WHEN** a user presses `Space` on a valid Library Skill
- **THEN** the Skill remains visible in the Library table and is hidden from new Target choices after save
### Requirement: The Library inspector exposes contextual diagnostics
Selecting a Location, Source, or Skill SHALL expose relevant details in a contextual inspector, including original and resolved paths, Source kind and key, Git facts, validation errors, unavailability, and overlap warnings. Invalid Skills SHALL remain visible rather than disappear. Skill details SHALL distinguish advisory naming warnings from blocking errors in Local Library, Targets and follower replicas; naming-only warnings MUST NOT produce inventory error markers or diagnostic rows.

#### Scenario: Unavailable Location
- **WHEN** a configured Location is missing locally
- **THEN** its row remains visible with an unavailable diagnostic in the inspector

#### Scenario: View an advisory naming mismatch
- **WHEN** the user views a skill whose safe frontmatter name differs from its source directory or has a naming-style inconsistency
- **THEN** details show labeled advisory warnings alongside the unmodified skill document, without an inventory error marker or warning-generated error row
- **AND** ordinary actions remain available for the usable skill

#### Scenario: Inspect a follower naming warning
- **WHEN** a follower replica contains a usable skill with divergent naming
- **THEN** its skill details show the advisory warning separately from blocking errors and preserve the read-only replica behavior

#### Scenario: View a real metadata error
- **WHEN** a skill document is malformed or required metadata is missing or unsafe
- **THEN** its details identify the blocking error and it is not treated as a warning-only usable skill
### Requirement: The TUI uses a consistent 256-color visual hierarchy
The TUI SHALL use indexed 256-color palette values by default. Repo borders SHALL be purple (indexed 99), User borders SHALL be blue (indexed 33), Library borders SHALL be gray (indexed 245), and modal and input-overlay borders SHALL remain blue, and titles plus persistent hotkey labels SHALL use an off-white bone color. The first line SHALL show scope tabs at the left and `Skillator` right-aligned at the right; scope paths SHALL appear only in the bottom status line. Active scope accents SHALL match the scope border color. Every modal title SHALL be capitalized, describe the modal's action or purpose rather than repeat the application name, and include one space between the left border and title text. Modal confirmation controls SHALL appear in the bottom border rather than as body text. Warning states SHALL use yellow accents and error states SHALL use red accents. Structural child-tree glyphs, divider lines, and unchecked `[ ]` markers SHALL use a visible dark gray without the terminal dim modifier. A selected row SHALL use a dark-blue background, bright primary text, and lighter subdued structural elements.

#### Scenario: Selecting a warning row
- **WHEN** the user selects a row carrying a warning state
- **THEN** its selected primary text is bright over the dark-blue selection background while unselected warnings retain their yellow accent

#### Scenario: Opening an editor
- **WHEN** the user opens any modal or input overlay
- **THEN** the overlay uses a blue border while the underlying workspace retains the active scope border color

#### Scenario: Opening a confirmation modal
- **WHEN** the user opens a save, discard, delete, or retry confirmation
- **THEN** its padded capitalized title names the action and its confirmation hotkeys appear only in the bottom border
### Requirement: The Target shows one Skill Directory at a time
User and Repo SHALL each show a second-line strip containing only their own configured Skill Directories. Labels SHALL identify the agent directory, such as `.agents` or `.claude`, without repeating the scope name. The default User directory SHALL remain `~/.agents/skills`. Existing User and Repository configurations SHALL retain their directory keys, paths, labels, and enablements. Root `skillator` SHALL select Repo inside a Git worktree, and User when started in the physical home directory or outside Git. Home-directory selection SHALL take precedence even when home is a Git worktree. These context defaults SHALL apply without Library configuration; Library onboarding SHALL appear only when Library is explicitly opened. Existing per-scope sub-tab state SHALL remain available during navigation. One table for the selected tab SHALL use an unlabeled checkbox column followed by `Mode`, `Skill`, `Description`, and `Action`. The `Mode` column SHALL contain compact `link`, `copy`, inherited `user`, or repository-owned `repo` values. `Description` SHALL remain Skill metadata rather than action text. `Action` SHALL contain only work Save will attempt and SHALL be blank for rows requiring no change. The `Repository` divider and its physical repository-owned Skills SHALL appear before every Library Source on Repository tabs. Remaining Source dividers SHALL be selectable and sorted by Source Key. Registered valid Skills and preserved Unresolved Enablements SHALL appear as indented child rows. Unregistered and Invalid Library Skills SHALL remain in the Library workspace.

#### Scenario: Launch from a Git Target
- **WHEN** the user launches root `skillator` from a Git worktree with one or more Repository Skill Directories
- **THEN** the first Repository Skill Directory is selected under the Repo scope while Library and User remain visible on the first line

#### Scenario: Directory switch
- **WHEN** the user presses `Tab` or `Shift+Tab`
- **THEN** the selected Skill Directory changes only within the active User or Repo scope while table-row focus remains stable where possible

#### Scenario: User tab shows its Skill Directory
- **WHEN** the default `.agents` sub-tab under User is active
- **THEN** the bottom status line shows `User: ~/.agents/skills`

#### Scenario: Repository Skills precede Library Skills
- **WHEN** a Repository tab contains both physical repository-owned Skills and Library Skills
- **THEN** the `Repository` group appears before every Library Source group

#### Scenario: Home directory inside Git
- **WHEN** root `skillator` starts in the physical home directory, including through a symbolic-link spelling, and home is a Git worktree
- **THEN** User is selected rather than Repo

#### Scenario: Context startup before Library setup
- **WHEN** Library configuration is absent and root `skillator` starts in a Git worktree or the home directory
- **THEN** Repo or User respectively is selected without automatic Library redirection
### Requirement: User Scope inheritance is explicit and read-only in Repository tabs

On a User Scope tab, desired Enablements SHALL remain editable and display ordinary `[✓] link` or `[✓] copy` state.
On a Repository tab, a Skill active only through User Scope SHALL display `[u] user` and SHALL not persist a Repository Enablement until the user explicitly stages and saves one.
User Scope itself SHALL remain read-only from Repository tabs.

Pressing `m` on an inherited row whose Skill is valid, Registered, and available through the Library SHALL stage a Linked Repository Enablement in the selected Skill Directory using the same Source Key and Skill path.
The row SHALL display `[✓] link` and a pending Enable action.
Save SHALL use ordinary repository link resolution and reconciliation, without changing User Scope configuration or materializations.
If the Skill cannot resolve to a valid, Registered, available Library Skill, `m` SHALL leave the row unchanged and explain why the link cannot be staged.

When the same Skill also has an explicit Repository Enablement, the explicit `[✓] link` or `[✓] copy` state SHALL remain visible with an `also active in User Scope` warning.
Ordinary repository mode changes SHALL remain available on that explicit Enablement.
Space on an explicit Enablement SHALL stage removal of only the Repository Enablement and restore `[u] user` when User Scope still enables the Skill.
Removing a saved override SHALL retain a pending Disable action until save; canceling a newly staged override SHALL clear its pending Enable action.
Space on an inherited-only row SHALL leave it unchanged and direct the user to its User tab for User Scope edits or to `m` for a repository link.
All edits SHALL remain staged until save and SHALL obey existing reconciliation safeguards.

#### Scenario: Inherited User Skill

- **WHEN** a Skill is enabled in User Scope but has no Enablement in the selected Repository Skill Directory
- **THEN** its Repository row displays `[u] user` without a pending action or saved Repository Enablement

#### Scenario: Explicit and inherited Skill

- **WHEN** the same Skill is enabled in User Scope and explicitly enabled in the selected Repository Skill Directory
- **THEN** its Repository row displays the explicit Repository mode and warns that the Skill is also active in User Scope

#### Scenario: Stage and save a repository link

- **WHEN** the user presses `m` on an available inherited Skill and saves
- **THEN** Skillator saves a Linked Enablement only in the selected Repository Skill Directory and links the Library Skill there
- **AND** reloading shows `[✓] link` with the User Scope warning and leaves User Scope configuration and materializations unchanged

#### Scenario: Discard a staged override

- **WHEN** the user stages a link from an inherited row and discards the edit
- **THEN** the row returns to `[u] user` and no configuration or materialization changes occur

#### Scenario: Remove an override

- **WHEN** the user presses Space on a saved repository override while User Scope still enables the Skill
- **THEN** the row displays `[u] user` with a pending Disable action
- **AND** saving removes only the Repository Enablement and its managed materialization under existing removal safeguards

#### Scenario: Cancel a new override

- **WHEN** the user presses `m` on an inherited row and then presses Space before saving
- **THEN** the row returns to `[u] user` with no pending action and saving performs no work for that Skill

#### Scenario: Unavailable inherited Skill

- **WHEN** the user presses `m` on an inherited Skill that cannot resolve to a valid, Registered, available Library Skill
- **THEN** Skillator explains the resolution problem and stages no Repository Enablement

#### Scenario: Occupied repository destination

- **WHEN** saving an override encounters unmanaged content at the expected repository entry
- **THEN** existing reconciliation rules require explicit confirmation before replacing recoverable conflicting content and preserve it if confirmation is declined
- **AND** Blocked conflicts remain blocked even with confirmation, and User Scope stays unchanged

### Requirement: Target bulk actions preserve intended modes
Source rows SHALL show tri-state enabled rollups and child counts. Toggling a mixed or disabled Source SHALL enable every currently available child, including filtered or collapsed children, while preserving modes already assigned and using `link` for newly enabled Skills. Toggling an all-enabled Source SHALL disable every child, including preserved Unresolved Enablements. An unavailable Skill MUST NOT receive a new Enablement.

#### Scenario: Bulk enable mixed Source
- **WHEN** a Source contains enabled, disabled, filtered, and collapsed valid Skills
- **THEN** toggling its divider enables all available children, preserves existing modes, and assigns `link` only to newly enabled children
### Requirement: Desired, observed, and pending action remain distinguishable
Checkboxes SHALL represent staged desired state. Mode SHALL display compact `link`, `copy`, `user`, or Library-acquisition `move`. The Action column SHALL distinguish pending Enable, Disable, Convert, Repair, Register, Unregister, Move, Copy, and Link work while remaining blank for no-op rows. Observed states including In Sync, Missing, Diverged Copy, and Unresolved SHALL remain available in the contextual inspector instead of occupying Description or Action. Non-Skill directory diagnostics SHALL appear as selectable diagnostics above the Skill rows rather than as fake Skills. An absent Skill Directory staged during first Repository setup SHALL be ordinary pending Save work and MUST NOT produce an initialization or missing-control-file Diagnostic row; existing malformed, uninspectable, or unexpectedly incomplete directories SHALL retain their diagnostics.

Ordinary table entries with a non-empty Action SHALL use Git-style semantic accents: green for additions, red for removals, and cyan for modifications. Yellow SHALL be reserved for actual conflicts. Skill names, descriptions, inspector details, and other free-form metadata MUST NOT determine row color. Invalid entries SHALL retain the error accent, and unavailable entries with no pending Action MAY remain structurally dimmed.

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
- **THEN** its row uses the semantic added, removed, or modified accent for that Action while the Action remains pending
### Requirement: Target navigation follows the approved key contract
The Target workspace SHALL support `j/k` for rows, `J/K` for Sources, `h/l` to collapse or expand Sources, `Space` to toggle editable Enablements, `m` to switch Link or Copy or stage Repo for a physical repository candidate, `Tab/Shift+Tab` for sub-tabs within the active scope, `Ctrl+H/Ctrl+L` for cycling Library, User, and Repo left/right, `/` to filter, `Esc` to clear or close, `s` for confirmed Save, `Ctrl+S` for safe fast Save and Exit, `u` to reset staged edits to their saved state, `q` to quit or close a non-editable overlay like `Esc`, `t` to change Target, `Ctrl+T` to create a sub-tab in the active scope, `a/e/d` to add/edit/delete a Skill Directory, and `?` for help. Editable overlays SHALL capture literal unmodified text keys, display a cursor, and use `Tab` to complete Location and Target paths. Plain arrow keys SHALL mirror `h/j/k/l`; Shift+Up and Shift+Down SHALL mirror `K/J` Source movement, while Shift+Left and Shift+Right SHALL retain collapse and expand. Ctrl-modified arrow keys SHALL remain unmapped; Ctrl+L SHALL NOT toggle Library. Scope and sub-tab navigation SHALL be captured by editable overlays rather than escaping into the workspace. In Library management, `m` SHALL cycle the available acquisition modes. The persistent action legend SHALL identify `s` as Save, `Ctrl+S` as Save and Exit, `m` as Mode, and `/` as Filter; SHALL omit page navigation and the `q` alias; SHALL be right-aligned with one-cell padding inside the main table's bottom border; and SHALL NOT create a separate horizontal footer rule. The Help modal SHALL explain both navigation levels, scope-aware tab creation, and the complete Target and Library mode cycles, document `q`, and scroll by row or page navigation. The special filters `/pending` and `/pending actions` SHALL show only rows whose Action is non-empty while preserving their containing dividers.

#### Scenario: Filtering collapsed Sources
- **WHEN** a filter matches children inside collapsed Sources
- **THEN** matching children are temporarily visible and clearing the filter restores prior collapse state

#### Scenario: Group navigation
- **WHEN** the user presses `J` or `K`
- **THEN** selection moves between Source dividers and skips directory diagnostics and individual Skills

#### Scenario: Compact persistent action legend
- **WHEN** either main workspace is rendered
- **THEN** its legend shows `m mode` and `/ filter` but does not enumerate modes, page navigation, or the `q` alias

#### Scenario: Scroll Help
- **WHEN** the Help modal is open and the user uses row or page movement
- **THEN** the modal scrolls while retaining the full mode reference and its close instructions

#### Scenario: Close an overlay with q
- **WHEN** a non-editable overlay is open and the user presses `q`
- **THEN** it closes with the same behavior as `Esc`

#### Scenario: Scope navigation
- **WHEN** the user presses Ctrl+L from Library with no overlay open
- **THEN** User becomes active, and a subsequent Ctrl+L selects Repo

#### Scenario: Sub-tab navigation stays in scope
- **WHEN** the user presses Tab at the final User directory
- **THEN** selection wraps to the first User directory and never changes to Repo

#### Scenario: Library shortcut returns to directory scope
- **WHEN** the user invokes Ctrl+H from User and then Ctrl+L from Library
- **THEN** the previously selected User directory is restored

#### Scenario: Library-only directional navigation
- **WHEN** `skillator library` starts on Library and its welcome modal is closed, then Ctrl+L is pressed twice
- **THEN** User then Repo is selected; Ctrl+H returns in the reverse direction

### Requirement: Skill Directory edits use one validated overlay
Adding Skill Directories through `Ctrl+T` or `a` in User and Repo SHALL use one filter-as-you-type chooser. Common suggestions SHALL include `.agents/skills` (Generic/Codex) and `.claude/skills` (Claude), displaying agent labels and full scope-relative paths. Up/Down SHALL scroll and select matching suggestions; Enter SHALL stage the selected suggestion. With no matches, Enter SHALL stage the typed custom path after validation. Editing and deleting SHALL retain compact validated overlays. Text keys SHALL enter literal text, including `j`, `k`, and `q`, rather than trigger workspace actions. User paths SHALL resolve beneath home and Repo paths beneath the selected repository. The same configuration validation and collision rules SHALL apply before staging and save, including duplicate keys, duplicate or overlapping paths, and containment. Already configured suggestions SHALL be marked unavailable. Addition SHALL select the new pending sub-tab without creating directories or configuration before save. Explicit Library first run SHALL use the normal Library workspace with a welcome modal. An absent Repository Configuration SHALL stage the Generic/Codex Repository directory in the normal Target workspace independently of Library setup.

#### Scenario: Existing recognized path
- **WHEN** first run detects a recognized agent path not yet configured
- **THEN** Skillator presents it as an unchecked recommendation and does not activate it automatically

#### Scenario: First Library screen
- **WHEN** Library Configuration is absent and Library is explicitly opened
- **THEN** Skillator shows the ordinary Library table with `./library` selected beneath the `I AM SKILLATOR!` welcome modal, identifies `e` as the location-edit action, and opens the path editor only after that explicit action

#### Scenario: Filter agent directories
- **WHEN** the user types `cla` in the User chooser
- **THEN** the list narrows to `.claude/skills` and selecting it stages a User directory rooted at `~/.claude/skills`

#### Scenario: Independent agent configurations
- **WHEN** the user saves both `.agents/skills` and `.claude/skills` under User
- **THEN** both sub-tabs reload with independently selectable enablements, without adding Repo directories

#### Scenario: Custom path with no matches
- **WHEN** no suggestion matches `custom-agent/skills` and the user presses Enter
- **THEN** the path is validated and staged in the active scope with a unique directory key and editable label

#### Scenario: Invalid or colliding custom path
- **WHEN** custom input escapes the scope root or collides with an existing directory
- **THEN** the chooser shows the validation error and stages no directory

#### Scenario: Cancel chooser
- **WHEN** the user presses Esc in the chooser
- **THEN** the chooser closes without changing staged directories, configuration, or materializations

### Requirement: Workspace and Target changes do not carry staged edits
Switching Target, changing top-level scope, or changing Library host while edits are staged SHALL offer Save, Discard and Continue, or Return to Editing as appropriate. Switching sub-tabs sharing one configuration SHALL preserve that configuration’s staged edits. Each scope SHALL remember its selected sub-tab and browsing state during the session. User Scope and Repository edits SHALL remain separate and MUST NOT be written to the other configuration.

#### Scenario: Toggle Library with staged Target edits
- **WHEN** the user presses `Ctrl+L` from Repo after changing Enablements
- **THEN** Skillator requires discard or return and performs no write unless the user separately saves

#### Scenario: Cross configuration tabs with staged edits
- **WHEN** the user attempts to leave a dirty User Scope tab for a Repository tab
- **THEN** Skillator requires save, discard, or return and does not carry those edits into Repository Configuration

#### Scenario: Implicit first-run default directory
- **WHEN** a Repository Configuration is absent and Skillator has only staged its implicit default Skill Directory without an explicit user edit
- **THEN** switching between User and Repository tabs does not require save or discard; returning to the Repository tab retains the implicit default

#### Scenario: Leave dirty local Library for follower
- **WHEN** the user selects a follower after staging local Library or host-registry changes
- **THEN** Save, Discard and Continue, or Return to Editing is required, and the pending changes never become remote edits

#### Scenario: Directory switch preserves scoped edits
- **WHEN** the user changes between `.agents` and `.claude` under User with pending enablements
- **THEN** the pending changes remain in the User session and are saved only to User configuration

### Requirement: All interactive edits remain staged until save
Checking, unchecking, changing modes, registration changes, follower additions, and directory edits SHALL remain in memory until an explicit save. Quitting or discarding SHALL leave configuration and Materializations unchanged. Library inventory save SHALL write only Library configuration; follower-registration save SHALL write only `~/.skillator/config.yaml`. These SHALL be distinct save plans, and a combined save request SHALL report each outcome without claiming cross-file atomicity. Remote inventory SHALL remain read-only. User Scope save SHALL write only `~/.agents/skillator.yaml`, and Repository save SHALL write only the current repository's `.agents/skillator.yaml` before its respective reconciliation.

#### Scenario: Quit after staging Target changes
- **WHEN** a user stages Enablement edits and quits without saving
- **THEN** Repository Configuration and filesystem Materializations remain unchanged
### Requirement: Save confirmation respects safety classification
Pressing `s` SHALL always show a confirmation, even for a clean or Safe-only plan. `Ctrl+S` SHALL skip confirmation only when every planned change is Safe. Confirmation questions and confirmation hotkeys SHALL use the off-white title color. Ordinary desired-action lines SHALL use the normal foreground and MUST NOT be presented as warnings; only actual warning or error content SHALL receive yellow or red semantic accents. Any Guarded plan SHALL show one complete batch confirmation with only Proceed with all Guarded Changes or Return to Editing. Blocked Changes SHALL be listed but MUST NOT be authorizable.

#### Scenario: Fast save with Guarded work
- **WHEN** the user presses `Ctrl+S` and the prepared plan contains a Guarded Change
- **THEN** Skillator shows the same guarded batch confirmation instead of bypassing it

#### Scenario: Reviewing desired Library actions
- **WHEN** Library management asks the user to confirm moves, copies, links, or configuration writes
- **THEN** the question and hotkey prompt are off-white while ordinary desired-action rows retain the normal foreground
### Requirement: Busy and partial outcomes preserve user understanding
An active Target lock SHALL not prevent browsing or staging; Save SHALL report Target Busy with Retry or Return to Editing and MUST NOT discard edits or retry automatically. A successful `s` save SHALL reload the current workspace with the saved state; a successful `Ctrl+S` save SHALL exit immediately except that a host-registration save SHALL defer exit until every newly saved follower's initialization prompt has been declined or completed. A partial or failed save SHALL remain on a concise result screen until acknowledged, identify applied, blocked, rolled-back, or Recovery Required work, and then exit nonzero.

#### Scenario: Partial save
- **WHEN** a confirmed save applies some changes while others remain blocked
- **THEN** the TUI presents the concise partial result until acknowledgement and exits with status `1`

#### Scenario: Host-only fast save
- **WHEN** `Ctrl+S` successfully saves one or more new follower registrations
- **THEN** the TUI offers initialization for each newly saved follower in registration order before exiting, whether each offer is declined or completed
### Requirement: Invalid configuration is diagnostic-only
Invalid or unsupported Repository or Library configuration SHALL open a read-only diagnostic screen that preserves the document exactly and provides no embedded YAML editor or save path.

#### Scenario: Unsupported Repository version
- **WHEN** the Target TUI opens a repository with unsupported Repository Configuration
- **THEN** it shows the version diagnostic, allows a normal read-only exit, and performs no writes
### Requirement: Repository-owned Skills are explicit and read-only
On a Repository tab, a physical Skill without a Repository Enablement SHALL appear as a repository candidate. Pressing `m` SHALL stage the candidate as `[r] repo`. A saved repository-owned Skill SHALL display `[r] repo`, SHALL remain outside Repository Configuration and Library resolution, and SHALL be read-only. Attempting to toggle or change mode on an `[r] repo` row SHALL explain that repository-owned Skills are managed through the parent tracking exceptions.

#### Scenario: Stage a repository candidate
- **WHEN** the user selects an unexcepted physical Skill and presses `m`
- **THEN** its row displays `[r] repo` with a pending repository-tracking Action and no Enablement is staged

#### Scenario: Repository row cannot be unchecked
- **WHEN** the user presses Space or `m` on a saved `[r] repo` Skill
- **THEN** the row remains `[r] repo` and no Library action is staged

#### Scenario: Save repository ownership
- **WHEN** the user saves a staged repo row under `.agents/skills/skillator`
- **THEN** Skillator adds `!skills/skillator/` beneath the exception-list marker and does not link, copy, or add a Repository Enablement

### Requirement: Scope tabs remain discoverable on the first line
The first TUI line SHALL show `Library`, `User`, and `Repo` in that order, with exactly one visibly active scope and `Skillator` right-aligned. All scopes SHALL remain discoverable in empty and diagnostic states. Without a Git target, Repo SHALL show an unavailable state rather than vanish or masquerade as User. Sub-tabs SHALL occupy the second line and belong only to the selected scope.

#### Scenario: Empty scope remains visible
- **WHEN** Repo has no configured directories
- **THEN** all three first-line scope labels remain visible and Repo provides its setup or unavailable state

#### Scenario: Narrow terminal
- **WHEN** available width cannot fit the normal header spacing
- **THEN** spacing contracts before labels, scope labels take priority over the title, and the header does not overlap or wrap

### Requirement: The bottom status line identifies the active scope and path
The bottom status line SHALL identify the active scope and resolved path. Library SHALL show its host and local Library configuration or remote replica path; User its selected Skill Directory; Repo its repository root and selected Skill Directory. Paths beneath home SHALL use `~`; remote paths SHALL be host-qualified. Long paths SHALL truncate without hiding the scope label, with full paths available in the inspector. Repository paths SHALL not appear in the first-line header.

#### Scenario: Local Library status
- **WHEN** Local is selected under Library
- **THEN** the bottom status line identifies `Library: Local · ~/.skillator/library.yaml`

#### Scenario: Follower Library status
- **WHEN** follower `build` is selected under Library
- **THEN** status identifies `Library: build` and `build:~/.skillator/library/replica`, with reported hostname separately when known

#### Scenario: User status
- **WHEN** `.claude` is selected under User
- **THEN** status identifies `User: ~/.claude/skills` without displaying the repository root as its scope path

#### Scenario: Repo status
- **WHEN** `.agents` is selected under Repo for `~/project`
- **THEN** status identifies `Repo: ~/project · .agents/skills`

#### Scenario: Long status path
- **WHEN** the active path exceeds status-line width
- **THEN** the scope label remains visible, truncation is indicated, and the inspector retains the full path

### Requirement: Library scope switching remains responsive
Entering Library SHALL render retained inventory and browsing state without waiting for filesystem or Git discovery. A previously loaded Target Library snapshot SHALL be reusable for matching Library configuration. Without retained inventory, explicit Library launch SHALL show configured locations and a loading status immediately. Discovery SHALL refresh in the background while navigation remains responsive; repeated requests SHALL NOT run concurrent local inventory scans. Completed refreshes SHALL preserve row identity, filters and collapse state, SHALL NOT overwrite staged edits or disrupt an active overlay, and SHALL NOT install obsolete results after configuration changes, undo or save. Leaving Library SHALL cancel outstanding follower operations. Cached inventory SHALL NOT bypass fresh save-time configuration, source or destination validation.

#### Scenario: Enter Library from a loaded Target
- **WHEN** the user moves from User or Repo to Library with unchanged Library configuration
- **THEN** existing Library rows appear without another synchronous discovery scan and refresh proceeds in the background

#### Scenario: Cold explicit Library launch
- **WHEN** Library starts without retained inventory
- **THEN** configured locations and loading status render before discovery completes and scope navigation remains available after any welcome modal is closed

#### Scenario: Discover external changes
- **WHEN** background discovery completes for a clean Library view after skills change on disk
- **THEN** the inventory updates while preserving selected row identity, filter and collapsed Sources

#### Scenario: Refresh during staged edits
- **WHEN** discovery completes while local changes or an editable overlay are active
- **THEN** the result waits and cannot overwrite checks, acquisition modes, paths, input or selection

#### Scenario: Superseded discovery
- **WHEN** an earlier discovery finishes after configuration changes, undo or save
- **THEN** its obsolete result is ignored and only current-state discovery may update the view

#### Scenario: Return from a follower
- **WHEN** the user leaves Library while follower inspection is running and later returns
- **THEN** the old inspection is canceled, the retained host/view state is shown, and refreshed inspection runs without blocking the UI

#### Scenario: Save after cached browsing
- **WHEN** the user saves after a Library source or configuration changed since the cached view was loaded
- **THEN** fresh save-time checks reject stale or unsafe changes rather than trusting the cached view

### Requirement: User and Repo entry remains responsive
Entering User or Repo from Library SHALL render retained destination state without waiting for filesystem or Git observation. If no destination state is retained, its loading view SHALL render immediately and allow scope navigation. Configuration, inventory and destination observation SHALL refresh in the background with no concurrent Target refresh scans. Completed refreshes SHALL preserve selected row identity, directory choice, filters and collapsed groups; they SHALL NOT overwrite staged edits in either scope or disrupt overlays. Replies superseded by save, undo or a different target SHALL NOT restore obsolete state. Cached browsing SHALL NOT bypass fresh save-time configuration, source or destination validation.

#### Scenario: Return to a loaded destination
- **WHEN** the user leaves Library for a previously loaded User or Repo scope
- **THEN** the retained destination renders before observation completes and updated filesystem state arrives asynchronously

#### Scenario: First destination entry from explicit Library
- **WHEN** Library was launched explicitly and no User/Repo view has been loaded
- **THEN** switching to User or Repo shows its loading state without waiting for Git and scope navigation remains responsive

#### Scenario: Refresh preserves browsing across directories
- **WHEN** clean destination refresh inserts or removes rows
- **THEN** directory choice, selected row identity, filters and collapsed groups are retained in both scopes where those rows/directories remain

#### Scenario: Refresh waits for edits and overlays
- **WHEN** refresh completes during staged changes in either scope or an open overlay
- **THEN** checks, modes, directory paths, input and selection are not overwritten

#### Scenario: Superseded destination refresh
- **WHEN** an earlier refresh completes after undo, successful save or selection of a different target
- **THEN** its result cannot reinstall old configuration or edits

#### Scenario: Save after cached destination browsing
- **WHEN** destination configuration, source content or destination safety changed after cached rows were loaded
- **THEN** Save performs fresh validation and rejects stale or unsafe changes rather than trusting browsing state

### Requirement: Remote Library synchronization is an explicit selected-follower action
On a selected remote Library host, `s` and `Ctrl+S` SHALL request synchronization of only that saved follower from the authoritative leader; neither key SHALL save staged Local Library, host-registry, User, or Repo edits or exit the TUI. On Local Library, `s` and `Ctrl+S` SHALL retain their save behavior. Before transfer, the TUI SHALL ask for explicit confirmation that content may be overwritten and stale files deleted only inside the follower's owned `~/.skillator/library/replica`; rejecting confirmation SHALL perform no delivery. Pending edits SHALL NOT implicitly authorize, save, or supply a remote transfer.

#### Scenario: Selected follower sync
- **WHEN** the user presses `s` or `Ctrl+S` while viewing the saved follower `build` and confirms the sync
- **THEN** only `build` is targeted by a leader-to-follower delivery and the TUI does not exit or save staged local configuration

#### Scenario: Local save remains local
- **WHEN** the user presses `s` while viewing Local Library
- **THEN** the ordinary confirmed Local save workflow is offered rather than a follower transfer

#### Scenario: Declined overwrite and deletion
- **WHEN** the user declines confirmation to sync a follower
- **THEN** no replica mutation or transfer starts and the registration remains unchanged

#### Scenario: Pending edits cannot authorize sync
- **WHEN** a selected follower has pending Local Library or host-registry changes
- **THEN** the existing save/discard/return guard applies and unsaved edits are neither written nor used as transfer input

### Requirement: Saved follower registration offers optional initialization
After a successful save creates one or more new follower registrations, the TUI SHALL ask whether to initialize each newly saved follower in registration order. Accepting SHALL use the same explicitly confirmed, selected-follower synchronization workflow; declining SHALL keep the registration and create no replica. Merely registering or browsing a follower SHALL NOT deliver content. A failed or unsaved registration SHALL NOT prompt for synchronization.

#### Scenario: Decline new follower initialization
- **WHEN** the user saves a new follower registration and declines its initialization prompt
- **THEN** its alias and destination remain saved but no replica is created by that prompt

#### Scenario: Multiple saved followers
- **WHEN** a save persists two new followers
- **THEN** each follower receives its own initialization decision, in registration order, without implicitly synchronizing either one

#### Scenario: Failed registration
- **WHEN** saving a staged new follower fails
- **THEN** no initialization prompt or transfer is started for that follower

### Requirement: TUI follower delivery uses fresh validated leader state
A confirmed remote sync SHALL re-read saved leader configuration, require that the selected alias still exists with its expected saved destination, and revalidate the leader Library sources before invoking the established owned-replica prerequisite, export, and rsync pipeline. It SHALL preserve the existing source acquisition modes and read-only treatment of leader skill content. Invalid, changed, unavailable, or ambiguous required input SHALL stop delivery before replica mutation and provide a useful diagnostic. It SHALL NOT change CLI behavior, persisted configuration formats, or follower-local selection/materialization state.

#### Scenario: Externally changed host
- **WHEN** a selected alias's saved destination changes or disappears after confirmation but before delivery
- **THEN** the transfer is refused instead of targeting the replacement or another follower

#### Scenario: Invalid leader source
- **WHEN** a source becomes unavailable or invalid after the TUI's cached inventory was rendered
- **THEN** fresh export validation reports the problem and no replica mutation begins

### Requirement: Follower delivery keeps the TUI responsive and reports the actual result
The TUI SHALL keep navigation responsive while one confirmed follower sync is running, SHALL NOT start duplicate transfers for repeated actions, SHALL cancel outstanding delivery on leaving its host or scope and ignore obsolete results. Cancellation SHALL stop running local SSH/rsync process groups and safely clean private exports; an interrupted transfer MAY leave a partially updated owned replica. Transfer and cleanup failures SHALL be visible with useful diagnostics, without leaking worker output to the terminal. Successful delivery SHALL refresh the selected follower's inspection; cancellation or stale replies SHALL NOT overwrite a different host's view. The TUI SHALL NOT claim atomic delivery, rollback, a transfer deadline, or bounded network operation.

#### Scenario: Duplicate keys while delivery is active
- **WHEN** the user presses `s` or `Ctrl+S` repeatedly while that follower's sync is in progress
- **THEN** at most one transfer is running and the UI remains responsive

#### Scenario: Leave selected host during delivery
- **WHEN** the user switches host or scope during a follower sync
- **THEN** local transport is canceled, private export cleanup is attempted, and a late result cannot replace the new view

#### Scenario: Successful sync refresh
- **WHEN** the selected follower sync succeeds and remains current
- **THEN** inspection refreshes so the newly delivered skills become visible without restarting the TUI

#### Scenario: Transfer or cleanup fails
- **WHEN** rsync, SSH, or private-export cleanup fails
- **THEN** the TUI reports the actual failure and any retained private export path needed for manual cleanup, without claiming replica rollback
