## MODIFIED Requirements

### Requirement: Sync does not create or edit desired state
Sync SHALL load current local Target configuration and reconcile only filesystem state. It MUST NOT create missing Repository Configuration, change Repository Configuration, or register Sources or Skills. `sync worktree` is the sole exception: it MAY replace the current linked worktree's local Target configuration with the validated primary worktree configuration. Missing Repository Configuration for ordinary sync SHALL direct the user to the TUI. Missing Library configuration SHALL use read-only discovery of the implicit `~/.skillator/library` Location; unavailable Source references SHALL remain Unresolved. Discovery and sync MUST NOT persist Library configuration.

#### Scenario: Missing Repository Configuration
- **WHEN** sync targets a repository without `.agents/skillator.yaml`
- **THEN** it performs no reconciliation writes and directs the user to run the Target TUI

#### Scenario: Missing Library configuration
- **WHEN** valid Repository Configuration exists but Library configuration is absent
- **THEN** sync discovers the implicit default Library, preserves desired state, and reports unavailable references as unresolved without creating Library configuration

#### Scenario: Worktree command is not ordinary sync
- **WHEN** `skillator sync worktree` has a valid primary worktree configuration and the current linked worktree has none
- **THEN** it copies the primary configuration before reconciling the current worktree
