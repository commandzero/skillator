# Spec Delta

## ADDED Requirements

### Requirement: Interactive sessions capture and restore mouse reporting
The TUI SHALL enable mouse reporting by default for its interactive session and disable it when leaving that session, including cleanup after setup failure. Keyboard operation SHALL remain available without mouse events. Non-interactive commands SHALL NOT enable mouse reporting.

#### Scenario: Exit an interactive workspace
- **WHEN** a user exits a Target or Library interactive session normally or through a handled error
- **THEN** mouse reporting, alternate-screen state and raw terminal mode are restored before control returns to the shell

#### Scenario: Setup fails after mouse reporting is attempted
- **WHEN** terminal setup fails after it may have enabled mouse reporting
- **THEN** cleanup attempts to disable mouse reporting and restore the terminal before returning the error

#### Scenario: Keyboard-only terminal
- **WHEN** the terminal delivers keyboard input but no mouse events
- **THEN** the existing keyboard navigation, editing and save workflows remain available

#### Scenario: Non-interactive command
- **WHEN** a user runs a non-interactive inventory or configuration command
- **THEN** it does not enable terminal mouse reporting

### Requirement: Mouse targets match the visible rendered surface
Only visible control cells SHALL accept mouse activation. Hit targets SHALL follow terminal-cell widths, clipping, table scrolling, filtering, collapsed Sources and the current frame size. Resize, navigation and background updates SHALL NOT leave targets referring to an obsolete row or control. Unoccupied table space, table headers and non-action border segments SHALL perform no action.

#### Scenario: Click after table scrolling and filtering
- **WHEN** a filtered table has scrolled to keep its selected row visible and the user clicks a displayed Skill checkbox
- **THEN** only that displayed Skill is targeted, not the row at the same position in the unfiltered or unscrolled inventory

#### Scenario: Unicode and clipped labels
- **WHEN** tabs contain wide or combining characters and a narrow terminal clips the rendered strip
- **THEN** click targets follow the displayed terminal cells and do not activate hidden labels or adjacent controls

#### Scenario: Refresh or resize changes the view
- **WHEN** a background refresh inserts a row or the terminal is resized before the next click is processed
- **THEN** the click uses refreshed visible geometry or is ignored until that geometry is available

#### Scenario: Empty table space
- **WHEN** the user clicks a column heading, empty table space, status text or an undecorated border segment
- **THEN** no selection, staged edit or workflow action occurs

### Requirement: Row selection is distinct from checkbox activation
A left click on a visible row body SHALL select that row for contextual inspection without editing it. A left click on a rendered Skill or Source checkbox SHALL select that row and invoke the same toggle as Space exactly once. The existing validity, availability, inheritance, repository-ownership and read-only rules SHALL apply.

#### Scenario: Select without toggling
- **WHEN** the user clicks a Skill name, description or pending Action cell
- **THEN** the clicked row becomes selected and its contextual information is shown without changing its desired state or starting pending work

#### Scenario: Select and deselect a Skill
- **WHEN** the user clicks an editable unchecked Skill checkbox and later clicks its checked checkbox
- **THEN** the first click stages enabling or showing that Skill and the second reverses that choice according to the existing workspace toggle behavior
- **AND** neither click writes configuration or materializations

#### Scenario: Source bulk toggle
- **WHEN** the user clicks a mixed Source checkbox in a Target directory
- **THEN** the same bulk-enable operation as Space includes eligible filtered or collapsed children and preserves their intended modes

#### Scenario: Protected rows retain their rules
- **WHEN** the user clicks a checkbox on an inherited-only, saved repository-owned, invalid or unavailable Skill, or on a follower replica
- **THEN** the same restriction and explanation as the corresponding keyboard action applies and no forbidden edit is staged

### Requirement: Disclosure and mode cells support direct activation
A left click on a Source disclosure glyph SHALL select the Source and toggle its stored collapsed state without toggling children. A left click within a Skill Mode cell SHALL select that Skill and invoke the same mode action as m. Filtering SHALL retain its existing temporary expansion behavior. Non-Skill Mode cells SHALL NOT change modes.

#### Scenario: Collapse and expand a Source
- **WHEN** the user clicks a Source's expanded disclosure glyph and then its collapsed glyph
- **THEN** children are hidden and then restored without changing their desired states

#### Scenario: Filter temporarily expands a collapsed Source
- **WHEN** the user clicks a Source disclosure while a filter forces its matching children visible
- **THEN** the filter continues showing matches and clearing the filter respects the resulting stored collapse state

#### Scenario: Change an addressed Skill mode
- **WHEN** one Skill is selected and the user clicks another Skill's Mode cell
- **THEN** only the clicked Skill receives the existing Target mode change, inherited override, repository-candidate action or Library acquisition-mode cycle as applicable

### Requirement: Tabs support direct mouse navigation
A left click on a scope tab SHALL request that exact scope. A left click on a directory or Library host sub-tab SHALL request that exact destination within the active scope. Existing dirty-edit guards and retained browsing state SHALL apply. Clicking the active tab SHALL be a no-op. Displayed sub-tab overflow arrows SHALL navigate one previous or next sub-tab within the active scope.

#### Scenario: Direct non-adjacent scope selection
- **WHEN** Library is active and the user clicks Repo
- **THEN** navigation requests Repo directly without entering User or triggering an intermediate scope guard
- **AND** without a Git target Repo retains its existing unavailable presentation

#### Scenario: Dirty scope or host switch
- **WHEN** the user clicks another scope or Library host while the current configuration has staged edits
- **THEN** the existing Save, Discard and Continue, or Return to Editing choices appear before leaving
- **AND** the clicked destination is retained through the user's choice without carrying edits into its configuration

#### Scenario: Directory click preserves staged edits
- **WHEN** the user clicks another User directory with pending User enablements
- **THEN** that directory becomes active and the pending edits remain in the User session without affecting Repo

#### Scenario: Overflow and active tab
- **WHEN** the user clicks a displayed sub-tab overflow arrow or the active sub-tab label
- **THEN** the arrow requests one sub-tab in its indicated direction within the active scope, while the active label causes no reload or guard

### Requirement: Visible action commands are clickable
Each individually labeled command in the workspace action legend SHALL accept a left click equivalent to its keyboard action. Combined action labels SHALL be separated into individually addressable commands. A visible Details command SHALL open the selected row's existing details view. Save, Save and Exit, follower Sync, Undo, Filter, tab creation, Help and applicable Library management commands SHALL retain their existing semantics.

#### Scenario: Mouse save respects confirmation
- **WHEN** the user clicks Save with staged edits
- **THEN** the ordinary save preparation and confirmation run before any write
- **AND** clicking Save and Exit skips confirmation only under the existing Safe-only fast-save rules

#### Scenario: Follower command remains a sync request
- **WHEN** a saved remote Library host is selected and the user clicks Sync
- **THEN** only that follower's explicitly confirmed synchronization is requested, without saving local edits or exiting

#### Scenario: Open details and an editor
- **WHEN** the user selects a Skill and clicks Details, or selects a Local Library Location and clicks Edit
- **THEN** the existing details view or validated Location editor respectively opens for that selection

#### Scenario: Separated Library location actions
- **WHEN** Local Library displays Add, Edit and Remove location commands
- **THEN** each label activates only its named action and Remove still requires the existing delete confirmation

### Requirement: Modal choices are directly clickable
Every modal SHALL expose individually labeled mouse targets for its available affirmative, negative, cancel, close, retry or exit choices. Clicking a choice SHALL invoke its existing semantic action, including save/discard/return and follower initialization sequencing. Guarded work SHALL still require explicit batch approval; Blocked work SHALL NOT become authorizable.

#### Scenario: Choose a dirty navigation outcome
- **WHEN** a dirty-scope modal is open and the user clicks Save and Switch, Discard and Switch or Return to Editing
- **THEN** exactly the corresponding existing navigation outcome occurs for the originally requested destination

#### Scenario: Decline guarded work
- **WHEN** a guarded save confirmation is open and the user clicks Return to Editing
- **THEN** no guarded change is applied and staged edits are preserved

#### Scenario: Retry a busy save
- **WHEN** the Target Busy modal is open and the user clicks Retry
- **THEN** one explicit retry is requested with the existing staged plan, without an automatic retry loop

#### Scenario: Acknowledge a partial save result
- **WHEN** a partial or failed save result is shown and the user clicks its Exit choice
- **THEN** the existing acknowledgement workflow exits with its nonzero status

#### Scenario: Decline follower initialization
- **WHEN** an optional follower initialization prompt is open and the user clicks Skip
- **THEN** no transfer starts for that follower and the existing remaining-prompt or exit sequence continues

### Requirement: Choosers and editors accept mouse controls without changing text semantics
Clicking a visible directory suggestion SHALL select it without applying it. Editor and chooser Apply and Cancel controls SHALL invoke the existing validation or cancellation action; Filter SHALL expose Apply and Clear controls. Path completion SHALL be clickable only where supported. Clicking an input SHALL retain keyboard editing without repositioning its cursor or submitting text.

#### Scenario: Select then apply a suggestion
- **WHEN** the user clicks a visible directory suggestion and then Stage
- **THEN** that suggestion is validated and staged in the active scope without creating directories or configuration before save

#### Scenario: Reject an unavailable suggestion
- **WHEN** the user selects an already-configured directory suggestion and clicks Stage
- **THEN** the existing validation rejects the duplicate without adding a directory

#### Scenario: Apply a custom path or cancel input
- **WHEN** a chooser has no matching suggestions and the user clicks Stage, or the user clicks Cancel in an editor
- **THEN** the existing typed-path validation or cancellation occurs without altering unrelated staged edits

#### Scenario: Complete a path
- **WHEN** the user clicks Complete in a Location or Target path editor
- **THEN** the same path completion as Tab updates the typed value without submitting it

#### Scenario: Input click does not submit
- **WHEN** the user clicks inside an editor input and then types q
- **THEN** q is appended as literal input using the existing editor semantics and the workspace does not quit

### Requirement: Overlays isolate mouse interaction
While a visible overlay is open, mouse input SHALL affect only controls and scrollable content belonging to that overlay. Clicking outside it SHALL neither dismiss it nor activate the workspace. A click that closes or replaces an overlay SHALL NOT also activate newly exposed content. Status-only notices SHALL remain non-modal.

#### Scenario: No click-through on cancel
- **WHEN** a user clicks a modal Cancel control located above an underlying Skill checkbox
- **THEN** only cancellation occurs and the Skill's desired state remains unchanged

#### Scenario: Outside click and wheel
- **WHEN** an editor or confirmation is open and the user clicks a background tab or scrolls over the background table
- **THEN** the overlay stays open and background selection, tabs and staged edits remain unchanged

#### Scenario: Status-only notice
- **WHEN** a non-modal status notice is displayed and the user clicks a valid workspace tab
- **THEN** the click follows the ordinary guarded tab-navigation behavior rather than being blocked by the notice

### Requirement: Vertical wheel navigation follows the pointed surface
An unmodified vertical wheel event over the main table SHALL move selection one visible row in its direction. Over a directory chooser it SHALL move suggestion selection one item; over a scrollable Help, details, diagnostic or follower-result view it SHALL scroll that view one line. Movement SHALL stop at the available bounds without changing staged state. Unsupported surfaces SHALL ignore wheel input.

#### Scenario: Wheel through a filtered inventory
- **WHEN** the user scrolls down over a filtered table with collapsed Sources
- **THEN** selection advances one currently visible row per event, remains visible and stops at the last visible row without toggling any checkbox

#### Scenario: Scroll modal content
- **WHEN** the user scrolls over a long details or Help view
- **THEN** only that view scrolls and scrolling stops at its content boundaries

#### Scenario: Wheel chooser and non-scrollable surfaces
- **WHEN** the user scrolls over a chooser or a confirmation dialog
- **THEN** chooser selection moves within matching suggestions, while the confirmation ignores the wheel

### Requirement: A mouse press activates at most one action
Only an unmodified left-button press SHALL activate a click target. Release, motion, drag, middle/right button, modified mouse and horizontal wheel events SHALL NOT invoke application actions. One supported event SHALL resolve to at most one control; keyboard input SHALL continue to operate on the resulting selection and overlay state.

#### Scenario: Press followed by release
- **WHEN** the terminal reports a left press and release over a Skill checkbox
- **THEN** desired state toggles once, not twice

#### Scenario: Unsupported mouse input
- **WHEN** the terminal reports a right click, drag or modified click over a destructive modal choice
- **THEN** no confirmation, cancellation or state change occurs

#### Scenario: Continue with the keyboard
- **WHEN** the user selects a Skill with the mouse and then presses Space or opens an editor and types using the keyboard
- **THEN** the existing key behavior acts on the mouse-selected row or active editor without a separate focus mode

### Requirement: Mouse gestures remain discoverable
Help and user documentation SHALL describe row selection versus checkbox toggling, mode and disclosure activation, tabs, clickable commands and modal choices, and wheel navigation. Documentation SHALL state that text entry still uses the keyboard and terminal-native text selection during mouse capture depends on the terminal's bypass gesture.

#### Scenario: Learn mouse behavior from Help
- **WHEN** the user opens Help
- **THEN** mouse gestures and keyboard alternatives are available in its scrollable content without replacing the existing key and mode reference
