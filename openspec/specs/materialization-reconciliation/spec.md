# materialization-reconciliation Specification

## Purpose
Defines how Skillator observes and safely reconciles declared Skill Enablements into Linked or Copied filesystem representations without losing user-controlled content.
## Requirements
### Requirement: Observation is action-free and fact-based
Skillator SHALL build an immutable Observed State for configured Skill Directories without prescribing actions or claiming historical ownership. Each Enablement SHALL compare as Drifted when a mismatch is proven, otherwise Unverifiable when conformity cannot be proven, otherwise In Sync. Unresolved identity SHALL remain orthogonal to that comparison.

#### Scenario: Proven mismatch on unresolved Enablement
- **WHEN** an unresolved Enablement has a known Expected Entry that is absent
- **THEN** Skillator reports the Enablement as both Unresolved and Drifted

#### Scenario: Source unavailable for present copy
- **WHEN** a copied directory is present but its Source cannot be resolved for comparison
- **THEN** Skillator reports the Enablement as Unverifiable unless another fact proves Drift

### Requirement: Skill Directory roots and entries are inspected without unsafe traversal
Skillator SHALL classify each configured root as absent, readable directory, inaccessible directory, symlink, or other object. It SHALL initially inspect only immediate children without following symlinks. An absent, symlinked, or non-directory root SHALL be Drifted; an inaccessible root SHALL be Unverifiable unless another mismatch is proven.

#### Scenario: Symlinked Skill Directory root
- **WHEN** the configured Skill Directory root is a symbolic link
- **THEN** Skillator reports directory Drift and does not traverse it as a managed root

### Requirement: Expected Entry names come from valid Skill names
For a non-root Skill, the Expected Entry SHALL be the final segment of `skill.path`, verified against frontmatter `name` when available. For `skill.path: .`, it SHALL be the validated frontmatter name. Names SHALL compare with exact case. Skillator MUST NOT invent a root-Skill name from the Source Key, repository name, or alias. Multiple Enablements in one directory resolving to the same Expected Entry SHALL form a Blocked collision group.

#### Scenario: Unavailable Source-root Skill
- **WHEN** an unresolved Source-root Skill has no available validated name
- **THEN** its Expected Entry remains unknown and no existing child is claimed on its behalf

#### Scenario: Expected Entry collision
- **WHEN** two Enablements in one Skill Directory resolve to the same Skill name
- **THEN** Skillator marks the claimants Drifted and blocks all operations for that entry without blocking unrelated entries

### Requirement: Linked Materializations are canonical absolute symlinks
A Linked Expected Entry SHALL be a symbolic link storing the canonical absolute path of its resolved Source Skill. Source changes SHALL be visible through the existing link without rewriting it. A noncanonical link reaching the correct Skill MAY be safely canonicalized. Capability failure MUST preserve `linked` desired state and MUST NOT silently create a copy.

#### Scenario: Missing Linked Materialization
- **WHEN** a resolved Linked Enablement has no occupant at its Expected Entry
- **THEN** ordinary reconciliation stages and publishes the canonical absolute link as a Safe Change

#### Scenario: Destination cannot create symlinks
- **WHEN** the destination filesystem rejects symbolic-link creation
- **THEN** Skillator reports a capability failure, preserves existing content, and does not fall back to Copied

### Requirement: Copied Materializations are self-contained verified snapshots
Skillator SHALL copy a Skill through a physical recursive walk that includes hidden and Git-ignored content, excludes every exact `.git` entry at every depth, never traverses symlinks, and preserves executable file bits while clearing special permission bits. It SHALL preserve an internal symlink's exact relative text only when every resolution step and final existing target remain inside the Source Skill. Absolute, escaping, broken, cyclic, inaccessible, or unverifiable links and unsupported filesystem entry kinds SHALL make the Skill Copy-Ineligible.

#### Scenario: Safe internal relative symlink
- **WHEN** a Skill contains a relative symlink whose entire resolution stays inside the Skill tree
- **THEN** the Copied Materialization recreates the symlink with the same stored text

#### Scenario: Escaping internal symlink
- **WHEN** a Skill contains a symlink that escapes its tree
- **THEN** Skillator blocks Copied materialization while allowing Linked to remain available

### Requirement: Copy equivalence compares meaningful content
A copy SHALL be Equivalent only when relative filename bytes and case, entry kinds, regular-file bytes, symlink text, and executable state match the resolved Source Skill. Timestamps, ownership, other permission bits, ACLs, extended attributes, inode identity, and `.git` content SHALL be ignored. A difference SHALL be a Diverged Copy without attributing the edit to Source or Target.

#### Scenario: Timestamp-only change
- **WHEN** a copy differs from its Source only in timestamps
- **THEN** Skillator reports it In Sync and performs no refresh

#### Scenario: File content changed
- **WHEN** a copied file's bytes differ from the current Source
- **THEN** Skillator reports a Diverged Copy and requires authorization before replacement

### Requirement: Unmanaged and duplicate content is reported conservatively
An immediate child not reserved by Skillator and not claimed by an Expected Entry SHALL be Unmanaged and diagnostic-only. It SHALL remain Git-trackable, SHALL not contribute removal work, and SHALL not be removed by ordinary reconciliation. Skillator MAY associate it with current known Skills but MUST NOT infer that Skillator created it. Multiple entries associated with one Skill in the same directory SHALL be reported as Duplicate or Possible Duplicate; repetition across different directories SHALL not.

#### Scenario: Enablement removed by hand
- **WHEN** Repository Configuration no longer declares a previously materialized entry
- **THEN** ordinary sync treats the occupant as Unmanaged and preserves it

#### Scenario: Repository-owned skill remains trackable
- **WHEN** an unlisted repository-owned Skill is present beside managed entries
- **THEN** Skillator leaves it unignored and performs no reconciliation action for it

### Requirement: Repository controls live beside, not inside, Skill Directories
Every configured Repository Skill Directory SHALL use an exact UTF-8 generated `.gitignore` in the directory's parent (for example, `.agents/.gitignore` for `.agents/skills`), ending in a newline. Its generated content SHALL ignore only configured Skillator materializations and `.skillator-*` recovery artifacts. It MUST NOT use a catch-all rule, ignore an unlisted repository-owned Skill, merge user content, or modify the Git index. The parent control file SHALL be clone-local and hidden by the repository root rule `/.agents/.gitignore`. User Scope Skill Directories SHALL have no Skillator-managed `.gitignore` and no Git tracking requirement.

#### Scenario: Control file created
- **WHEN** the control file is absent and the repository has the required root ignore rule
- **THEN** Skillator creates it as a Safe Change and verifies configured materializations are ignored

#### Scenario: Managed skill is ignored locally
- **WHEN** local Target configuration enables `release-checklist` in `.agents/skills`
- **THEN** the generated `.agents/.gitignore` ignores `skills/release-checklist` without ignoring other entries in `skills`

#### Scenario: Control file is local
- **WHEN** the generated control file exists
- **THEN** the root ignore rule hides it from Git status and Skillator never asks the user to stage it

#### Scenario: Tracked repository-owned entry
- **WHEN** an Unmanaged Entry is Git-tracked
- **THEN** Skillator preserves it, leaves it unignored, and leaves unrelated Git worktree changes untouched

#### Scenario: Tracked expected entry
- **WHEN** an Expected Entry is Git-tracked
- **THEN** Skillator blocks its replacement even under force and leaves unrelated Git worktree changes untouched

### Requirement: Reconciliation plans classify every change
Every planned mutation SHALL be exactly Safe, Guarded, or Blocked. Safe Changes SHALL be eligible for automatic apply. Guarded Changes SHALL require explicit TUI batch confirmation or invocation-wide `--force`. Blocked Changes SHALL remain unauthorized by either mechanism. Already conforming entries SHALL be No Change.

#### Scenario: Ordinary mixed sync
- **WHEN** a plan contains independent Safe, Guarded, and Blocked Changes
- **THEN** ordinary sync applies Safe work, reports Guarded work Not Authorized, reports Blocked work, and continues independent operations

#### Scenario: Forced mixed sync
- **WHEN** the same plan runs with `--force`
- **THEN** Skillator authorizes every viable Guarded Change but does not authorize Blocked work or Recovery Required

### Requirement: Safety boundaries protect uncertain or unrecoverable content
Missing roots and Materializations, canonicalization of a correct link, replacement of a broken link after Source verification, conversion of an In-Sync Materialization, and reviewed removal of an In-Sync disabled Materialization SHALL be Safe. Replacement of recoverable conflicting content, Diverged Copies, or modified control files SHALL be Guarded. Unmanaged Entries SHALL be preserved. Invalid configuration, containment violations, unresolved required content, Copy-Ineligible Skills, inaccessible entries, unsupported capabilities, changed preconditions, tracked expected occupants, collisions, ambiguous recovery, or inability to preserve content SHALL be Blocked.

#### Scenario: Misdirected link
- **WHEN** an Expected Entry is a symlink proven to target a different Skill and it can be preserved
- **THEN** Skillator classifies replacement as Guarded

#### Scenario: Unverifiable occupant
- **WHEN** an occupant cannot be inspected enough to preserve or replace it safely
- **THEN** Skillator classifies the operation as Blocked regardless of confirmation or force

### Requirement: Mutation is coordinated and preconditions are revalidated
Save and sync SHALL acquire one exclusive Target mutation lock before planning and retain it through final observation. Worktree sync SHALL acquire the primary and destination locks in stable canonical-path order. An active owner SHALL produce Target Busy without writes. Check mode SHALL also return Target Busy rather than inspect transitional state. Immediately before every mutation, Skillator SHALL revalidate relevant Source and destination facts; changed facts SHALL block only the affected operation and MUST NOT trigger silent replanning.

#### Scenario: Concurrent mutation
- **WHEN** another process actively owns the Target mutation lock
- **THEN** save, sync, and check return Target Busy without mutating or inspecting transitional state

#### Scenario: Source changes after staging
- **WHEN** a Source changes while a copy candidate is staged
- **THEN** Skillator discards the candidate when possible, leaves the existing destination untouched, and reports changed-during-reconciliation

### Requirement: Publication preserves recoverable content
Every new link or copy SHALL be completely staged and validated as a sibling inside its destination directory before publication. When rename cannot replace the existing kind or non-empty directory portably, Skillator SHALL move the existing occupant to an operation-specific backup, install the staged candidate, verify it, and then remove the backup. Installation failure SHALL trigger rollback. Skillator MUST NOT promise transaction-wide atomicity or power-loss durability.

#### Scenario: Replacement succeeds
- **WHEN** a guarded replacement is authorized and the staged candidate validates
- **THEN** Skillator preserves the old occupant until the candidate is published and verified, then removes the operation's backup

#### Scenario: Installation fails and rollback succeeds
- **WHEN** publication fails after displacing the old occupant and restoration succeeds
- **THEN** Skillator reports Failed — Rolled Back and retains the original destination state

### Requirement: Recovery Artifacts are never guessed away
Immediate children beginning `.skillator-` SHALL be reserved for stages and backups. After acquiring the lock, Skillator MAY delete a validated abandoned stage after restoring any safely paired backup, and SHALL restore exactly one valid backup when its encoded original destination is absent. Ambiguous, malformed, inaccessible, or multiple backups, or coexistence of destination and backup, SHALL produce Recovery Required or Blocked with exact paths. Neither confirmation, force, nor file age SHALL authorize ambiguous deletion.

#### Scenario: Unique interrupted backup
- **WHEN** exactly one valid backup identifies an absent original destination
- **THEN** normal reconciliation restores the backup before planning further work

#### Scenario: Destination and backup coexist
- **WHEN** both a destination and its abandoned backup are present
- **THEN** Skillator preserves both and requires manual recovery

### Requirement: Partial apply keeps desired state authoritative
A TUI save SHALL write the complete valid Repository Configuration before applying its prepared reconciliation. If configuration writing fails, no reconciliation mutation SHALL occur. After a successful configuration write, later filesystem failure MUST NOT roll desired state back. Independent viable operations SHALL continue, pure removals SHALL run last, and Skillator SHALL always perform fresh final observation and report every remaining Drift, Unverifiable state, and recovery action.

#### Scenario: Filesystem failure after configuration save
- **WHEN** valid desired state is written and a later Materialization operation fails
- **THEN** the new Repository Configuration remains authoritative and the final result reports partial convergence for a later sync

### Requirement: Successful reconciliation is idempotent
An In-Sync save, sync, or check SHALL perform no configuration or Materialization writes. An Equivalent Copy SHALL not be refreshed, and incidental filesystem metadata SHALL remain untouched. Repeated successful reconciliation SHALL converge on the same observable Materializations.

#### Scenario: Repeated sync
- **WHEN** a Target is fully In Sync and sync runs again
- **THEN** Skillator reports In Sync and performs no filesystem or configuration writes
