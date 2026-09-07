## MODIFIED Requirements

### Requirement: The Target shows one Skill Directory at a time
The Target workspace SHALL show one flat horizontal strip beginning with the primary `User` Scope tab, followed by any additional tabs labeled `User · <label>`, then Repository Skill Directory tabs. The primary User tab SHALL represent `~/.agents/skills`; additional User tabs SHALL represent other home-relative User Scope Skill Directories. The `Target:` header SHALL show the active User Skill Directory when a User tab is selected and the repository path when a Repository tab is selected. When root `skillator` is launched for a Git Target, it SHALL select the first Repository Skill Directory rather than the User tab; if no Repository Skill Directory exists, it SHALL select the User tab. One table for the selected tab SHALL use an unlabeled checkbox column followed by `Mode`, `Skill`, `Description`, and `Action`. The `Mode` column SHALL contain compact `link`, `copy`, inherited `user`, or repository-owned `repo` values. `Description` SHALL remain Skill metadata rather than action text. `Action` SHALL contain only work Save will attempt and SHALL be blank for rows requiring no change. The `Repository` divider and its physical repository-owned Skills SHALL appear before every Library Source on Repository tabs. Remaining Source dividers SHALL be selectable and sorted by Source Key. Registered valid Skills and preserved Unresolved Enablements SHALL appear as indented child rows. Unregistered and Invalid Library Skills SHALL remain in the Library workspace.

#### Scenario: Launch from a Git Target
- **WHEN** the user launches root `skillator` from a Git worktree with one or more Repository Skill Directories
- **THEN** the first Repository Skill Directory is selected while the User tabs remain visible before it in the tab strip

#### Scenario: Directory switch
- **WHEN** the user presses `Tab` or `Shift+Tab`
- **THEN** the selected User or Repository Skill Directory changes while table-row focus remains stable where possible

#### Scenario: User tab shows its Skill Directory
- **WHEN** the primary `User` tab is active
- **THEN** the `Target:` header shows `~/.agents/skills`

#### Scenario: Repository Skills precede Library Skills
- **WHEN** a Repository tab contains both physical repository-owned Skills and Library Skills
- **THEN** the `Repository` group appears before every Library Source group

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

### Requirement: Target navigation follows the approved key contract
The Target workspace SHALL support `j/k` for rows, `J/K` for Sources, `h/l` to collapse or expand Sources, `Space` to toggle editable Enablements, `m` to switch Link or Copy or stage Repo for a physical repository candidate, `Tab/Shift+Tab` for Skill Directories, `/` to filter, `Esc` to clear or close, `s` for confirmed Save, `Ctrl+S` for safe fast Save and Exit, `u` to reset staged edits to their saved state, `q` to quit or close a non-editable overlay like `Esc`, `t` to change Target, `Ctrl+T` to open a new Target Tab prefilled as `.claude`, `a/e/d` to add/edit/delete a Skill Directory, `Ctrl+L` to toggle Library and Target workspaces, and `?` for help. Editable overlays SHALL capture literal unmodified text keys, display a cursor, and use `Tab` to complete Location and Target paths. Plain arrow keys SHALL mirror `h/j/k/l`; Shift+Up and Shift+Down SHALL mirror `K/J` Source movement, while Shift+Left and Shift+Right SHALL retain collapse and expand. Ctrl-modified arrows SHALL remain unmapped. In Library management, `m` SHALL cycle the available acquisition modes. The persistent action legend SHALL identify `s` as Save, `Ctrl+S` as Save and Exit, `m` as Mode, and `/` as Filter; SHALL omit page navigation and the `q` alias; SHALL be right-aligned with one-cell padding inside the main table's bottom border; and SHALL NOT create a separate horizontal footer rule. The Help modal SHALL list the complete Target and Library mode cycles, document `q`, and scroll by row or page navigation. The special filters `/pending` and `/pending actions` SHALL show only rows whose Action is non-empty while preserving their containing dividers.

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
