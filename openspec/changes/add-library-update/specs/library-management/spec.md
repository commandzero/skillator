## ADDED Requirements

### Requirement: Library updates discover distinct Git checkouts

`skillator library update` SHALL inspect all registered Locations using existing resolution, exclusion, and directory-symlink rules. It SHALL select roots at or below Locations, including ordinary nested repositories and worktrees represented by `.git` files, even without valid Skills. It MUST NOT traverse `.git`, follow acquired links to external repositories, select enclosing roots outside Locations, or update bare repositories. It SHALL deduplicate canonical checkout paths, not Source Keys, remote URLs, or common Git directories, and freeze the plan before sequential canonical-path-ordered execution. Initialized submodules SHALL be skipped with a canonical-path `submodule_skipped` advisory, even when directly registered. A skip alone SHALL NOT cause failure.

#### Scenario: Multiple Locations and nested checkouts
1. **WHEN** Locations expose nested checkouts, colliding Source Keys, and repeated canonical checkout paths
2. **THEN** each distinct non-submodule checkout is planned once regardless of Skill validity

#### Scenario: Worktrees and submodules
1. **WHEN** 2 worktrees share a Git common directory and an initialized submodule is discovered
2. **THEN** both worktrees are selected independently and the submodule is skipped with an advisory

#### Scenario: Direct submodule Location
1. **WHEN** a directly registered submodule has an attached tracking branch
2. **THEN** it is still skipped without pulling

#### Scenario: Discovery boundaries
1. **WHEN** a repository is excluded, reached only through a directory symlink, bare, or encloses a Location from outside
2. **THEN** it is not selected for update

### Requirement: Library pulls only fast-forward clean checkouts

Immediately before pulling, Skillator SHALL verify unchanged checkout identity, an attached branch, a configured upstream, no staged, unstaged, or untracked changes, and no in-progress Git operation. Ignored files alone SHALL NOT block a checkout. Ineligible checkouts SHALL receive blocked results. Eligible checkouts SHALL pull their configured upstream with fast-forward-only integration, overriding rebase, autostash, non-fast-forward merge, and submodule-recursion settings. Skillator MUST NOT stash, reset, switch branches, force, merge divergent history, rebase, initialize submodules, or update submodule checkouts.

#### Scenario: Fast-forward update
1. **WHEN** a clean checkout is behind its upstream without divergence
2. **THEN** it fast-forwards and reports applied

#### Scenario: Current or locally ahead
1. **WHEN** a successful pull leaves HEAD unchanged
2. **THEN** the result is unchanged and local commits remain intact without a push

#### Scenario: Dirty or unconfigured checkout
1. **WHEN** a checkout is dirty, detached, lacks an upstream, has an operation in progress, or changed identity
2. **THEN** it is blocked before pulling with a diagnostic

#### Scenario: Diverged history
1. **WHEN** pulling discovers divergent local and upstream commits
2. **THEN** it fails without merging or rebasing and preserves local branch history and working content

### Requirement: Library updates preserve independent outcomes and desired state

Skillator SHALL continue after per-checkout blocks, failures, or unavailable Locations, and diagnose incomplete discovery including unreadable subtrees. Successful pulls SHALL remain applied. Skillator MUST NOT roll back successful pulls or claim a failed pull made no Git metadata changes. It MUST NOT edit configuration, Enablements, registries, control files, or Materializations. Existing links SHALL expose Source changes and copies SHALL require explicit synchronization. Later discovery SHALL read live content.

#### Scenario: Partial update
1. **WHEN** 1 repository fails or a Location is unavailable while another repository can update
2. **THEN** independent work proceeds, all outcomes are reported, and exit status is 1

#### Scenario: Materializations
1. **WHEN** a pulled Source supplies links and copies
2. **THEN** links expose the new content while copies and declarations remain unchanged

#### Scenario: Empty Library
1. **WHEN** configuration is absent, Locations are empty, or no non-submodule checkouts exist
2. **THEN** no files are created, advisories are retained, and the command succeeds unless discovery has blocking errors
