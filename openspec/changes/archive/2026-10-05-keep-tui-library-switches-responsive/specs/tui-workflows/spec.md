## ADDED Requirements

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
