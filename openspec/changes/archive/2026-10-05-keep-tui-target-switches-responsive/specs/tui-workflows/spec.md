## ADDED Requirements

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
