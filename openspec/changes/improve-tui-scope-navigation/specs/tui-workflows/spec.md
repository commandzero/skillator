# Spec Delta

## MODIFIED Requirements

### Requirement: The TUI uses a consistent 256-color visual hierarchy
The TUI SHALL use indexed 256-color palette values by default. Repo borders SHALL be purple (indexed 99), User borders SHALL be blue (indexed 33), Library borders SHALL be bone (indexed 230), and modal and input-overlay borders SHALL remain blue, and titles plus persistent hotkey labels SHALL use an off-white bone color. The first line SHALL show scope tabs at the left and `Skillator` right-aligned at the right; scope paths SHALL appear only in the bottom status line. Active scope accents SHALL match the scope border color. Every modal title SHALL be capitalized, describe the modal's action or purpose rather than repeat the application name, and include one space between the left border and title text. Modal confirmation controls SHALL appear in the bottom border rather than as body text. Warning states SHALL use yellow accents and error states SHALL use red accents. Structural child-tree glyphs, divider lines, and unchecked `[ ]` markers SHALL use a visible dark gray without the terminal dim modifier. A selected row SHALL use a dark-blue background, bright primary text, and lighter subdued structural elements.

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
User and Repo SHALL each show a second-line strip containing only their own configured Skill Directories. Labels SHALL identify the agent directory, such as `.agents` or `.claude`, without repeating the scope name. The default User directory SHALL remain `~/.agents/skills`. Existing User and Repository configurations SHALL retain their directory keys, paths, labels, and enablements. When root `skillator` is launched for a Git Target, it SHALL select Repo and its first Repository Skill Directory; if no Repository Skill Directory exists, it SHALL select User. Absent Library configuration SHALL retain the Library-first onboarding flow. One table for the selected tab SHALL use an unlabeled checkbox column followed by `Mode`, `Skill`, `Description`, and `Action`. The `Mode` column SHALL contain compact `link`, `copy`, inherited `user`, or repository-owned `repo` values. `Description` SHALL remain Skill metadata rather than action text. `Action` SHALL contain only work Save will attempt and SHALL be blank for rows requiring no change. The `Repository` divider and its physical repository-owned Skills SHALL appear before every Library Source on Repository tabs. Remaining Source dividers SHALL be selectable and sorted by Source Key. Registered valid Skills and preserved Unresolved Enablements SHALL appear as indented child rows. Unregistered and Invalid Library Skills SHALL remain in the Library workspace.

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

### Requirement: Target navigation follows the approved key contract
The Target workspace SHALL support `j/k` for rows, `J/K` for Sources, `h/l` to collapse or expand Sources, `Space` to toggle editable Enablements, `m` to switch Link or Copy or stage Repo for a physical repository candidate, `Tab/Shift+Tab` for sub-tabs within the active scope, `Ctrl+Left/Right` for cycling Library, User, and Repo, `/` to filter, `Esc` to clear or close, `s` for confirmed Save, `Ctrl+S` for safe fast Save and Exit, `u` to reset staged edits to their saved state, `q` to quit or close a non-editable overlay like `Esc`, `t` to change Target, `Ctrl+T` to create a sub-tab in the active scope, `a/e/d` to add/edit/delete a Skill Directory, `Ctrl+L` to switch between Library and the last active User or Repo scope, and `?` for help. Editable overlays SHALL capture literal unmodified text keys, display a cursor, and use `Tab` to complete Location and Target paths. Plain arrow keys SHALL mirror `h/j/k/l`; Shift+Up and Shift+Down SHALL mirror `K/J` Source movement, while Shift+Left and Shift+Right SHALL retain collapse and expand. Ctrl+Up and Ctrl+Down SHALL remain unmapped. Scope and sub-tab navigation SHALL be captured by editable overlays rather than escaping into the workspace. In Library management, `m` SHALL cycle the available acquisition modes. The persistent action legend SHALL identify `s` as Save, `Ctrl+S` as Save and Exit, `m` as Mode, and `/` as Filter; SHALL omit page navigation and the `q` alias; SHALL be right-aligned with one-cell padding inside the main table's bottom border; and SHALL NOT create a separate horizontal footer rule. The Help modal SHALL explain both navigation levels, scope-aware tab creation, and the complete Target and Library mode cycles, document `q`, and scroll by row or page navigation. The special filters `/pending` and `/pending actions` SHALL show only rows whose Action is non-empty while preserving their containing dividers.

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
- **WHEN** the user presses Ctrl+Right from Library with no overlay open
- **THEN** User becomes active, and a subsequent Ctrl+Right selects Repo

#### Scenario: Sub-tab navigation stays in scope
- **WHEN** the user presses Tab at the final User directory
- **THEN** selection wraps to the first User directory and never changes to Repo

#### Scenario: Library shortcut returns to directory scope
- **WHEN** the user invokes Ctrl+L from User and then again from Library
- **THEN** the previously selected User directory is restored

### Requirement: Skill Directory edits use one validated overlay
Adding Skill Directories through `Ctrl+T` or `a` in User and Repo SHALL use one filter-as-you-type chooser. Common suggestions SHALL include `.agents/skills` (Generic/Codex) and `.claude/skills` (Claude), displaying agent labels and full scope-relative paths. Up/Down SHALL scroll and select matching suggestions; Enter SHALL stage the selected suggestion. With no matches, Enter SHALL stage the typed custom path after validation. Editing and deleting SHALL retain compact validated overlays. Text keys SHALL enter literal text, including `j`, `k`, and `q`, rather than trigger workspace actions. User paths SHALL resolve beneath home and Repo paths beneath the selected repository. The same configuration validation and collision rules SHALL apply before staging and save, including duplicate keys, duplicate or overlapping paths, and containment. Already configured suggestions SHALL be marked unavailable. Addition SHALL select the new pending sub-tab without creating directories or configuration before save. Library first run SHALL use the normal Library workspace with a welcome modal; after the Library is saved, an absent Repository Configuration SHALL stage the Generic/Codex Repository directory in the normal Target workspace.

#### Scenario: Existing recognized path
- **WHEN** first run detects a recognized agent path not yet configured
- **THEN** Skillator presents it as an unchecked recommendation and does not activate it automatically

#### Scenario: First Library screen
- **WHEN** Library Configuration is absent
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
- **WHEN** the user presses `Ctrl+L` after changing Enablements
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

## ADDED Requirements

### Requirement: Scope tabs remain discoverable on the first line
The first TUI line SHALL show `Library`, `User`, and `Repo` in that order, with exactly one visibly active scope and `Skillator` right-aligned. All scopes SHALL remain discoverable in empty and diagnostic states. Without a Git target, Repo SHALL show an unavailable state rather than vanish or masquerade as User. Sub-tabs SHALL occupy the second line and belong only to the selected scope.

#### Scenario: Empty scope remains visible
- **WHEN** Repo has no configured directories
- **THEN** all three first-line scope labels remain visible and Repo provides its setup or unavailable state

#### Scenario: Narrow terminal
- **WHEN** available width cannot fit the normal header spacing
- **THEN** spacing contracts before labels, scope labels take priority over the title, and the header does not overlap or wrap

### Requirement: The bottom status line identifies the active scope and path
The bottom status line SHALL identify the active scope and resolved path. Library SHALL show its host and Library configuration path; User its selected Skill Directory; Repo its repository root and selected Skill Directory. Paths beneath home SHALL use `~`; remote paths SHALL be host-qualified. Long paths SHALL truncate without hiding the scope label, with full paths available in the inspector. Repository paths SHALL not appear in the first-line header.

#### Scenario: Local Library status
- **WHEN** Local is selected under Library
- **THEN** the bottom status line identifies `Library: Local · ~/.skillator/library.yaml`

#### Scenario: Follower Library status
- **WHEN** follower `build` is selected under Library
- **THEN** status identifies `Library: build` and `build:~/.skillator/library.yaml`, with reported hostname separately when known

#### Scenario: User status
- **WHEN** `.claude` is selected under User
- **THEN** status identifies `User: ~/.claude/skills` without displaying the repository root as its scope path

#### Scenario: Repo status
- **WHEN** `.agents` is selected under Repo for `~/project`
- **THEN** status identifies `Repo: ~/project · .agents/skills`

#### Scenario: Long status path
- **WHEN** the active path exceeds status-line width
- **THEN** the scope label remains visible, truncation is indicated, and the inspector retains the full path
