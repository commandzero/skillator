## Purpose

Synchronize skill content and user selections across SSH hosts while preserving home-relative locations, Git revisions, and independently edited content.

## ADDED Requirements

### Requirement: Hosts are configured explicitly

Skillator SHALL read optional version 1 main configuration from `~/.skillator/config.yaml`, with a `hosts` mapping from unique host aliases to entries containing one required SSH `destination` string. It SHALL reject unknown fields, duplicate keys, unsupported versions, empty destinations, and destinations interpreted as command options. Host configuration SHALL remain machine-local. The command SHALL select all configured hosts unless `--hosts` supplies a comma-separated subset of known aliases. Duplicate requested aliases SHALL be deduplicated; empty or unknown aliases SHALL fail before connecting. No configured hosts SHALL produce an actionable error. SSH SHALL use existing user configuration and authentication without copying credentials or requiring connections between remote hosts.

#### Scenario: Explicit host subset

- **WHEN** configuration contains `build` and `dev` and the command selects `--hosts dev`
- **THEN** only `dev` and the initiating machine participate, and no connection is made to `build`

### Requirement: Every selected host passes preflight before writes

The command SHALL verify reachability, remote home discovery, compatible Skillator synchronization protocol, Git, rsync, and readable required configuration on all selected hosts before any persistent write on any participant. A failure SHALL abort the whole run without cloning, fetching into existing repositories, staging transfers, writing configuration, or advancing synchronization history. Diagnostics SHALL identify the configured host alias and the missing dependency or incompatibility. Skillator SHALL NOT install dependencies automatically. Compatibility SHALL be determined by the synchronization protocol and required capabilities, not exact application-version equality.

Absent receiving Library or User Scope Configuration SHALL be treated as empty initial state, not a missing dependency. Malformed or unreadable existing configuration SHALL fail preflight. SSH preflight SHALL require previously established host trust and SHALL NOT write trust files or persistent connection state; missing trust SHALL direct the user to establish it separately.

#### Scenario: Skillator is missing remotely

- **WHEN** a selected host does not have Skillator installed
- **THEN** the command exits unsuccessfully with `Skillator is not installed on remote host <alias>` and no participant changes

#### Scenario: Incompatible remote version

- **WHEN** a selected host cannot support the required synchronization protocol
- **THEN** the command reports the host, encountered version, and required protocol, and performs no persistent writes anywhere

#### Scenario: Receiving host has no library configuration

- **WHEN** a compatible receiving Skillator has no Library or User Scope Configuration
- **THEN** preflight accepts empty initial state and application can create the required registrations and selections

### Requirement: Library paths preserve their home-relative location

The command SHALL map each participating library location's resolved path relative to its user's home into the same relative path under each other user's home. It SHALL include the central library and external locations, preserve existing location registrations, and add missing corresponding registrations. Paths whose resolved or physical destination escapes the user's home SHALL be rejected without writing through them. Unsupported outside-home locations SHALL fail validation before application. Configuration expressions SHALL be normalized to portable home-relative registrations when transferred, without replacing existing equivalent expressions. Overlap and identity collisions SHALL retain existing blocking rules.

#### Scenario: Different user homes

- **WHEN** the initiating location is `~/Development/acme/skills` and a selected host uses a different home directory
- **THEN** that host receives the location at its own `~/Development/acme/skills`

#### Scenario: Destination parent escapes home

- **WHEN** a destination ancestor is a symbolic link resolving outside the receiving user's home
- **THEN** the command rejects that destination and does not write through the link

### Requirement: All skill content participates

Synchronization SHALL include all discovered skills regardless of visibility or selection, respect location exclusions, and transfer each complete skill directory including supporting files. Invalid discovered skills SHALL retain their diagnostics and SHALL NOT gain new enablements. The transfer scope SHALL also retain previously synchronized paths needed to recognize deletion or a missing `SKILL.md`. Rsync SHALL NOT transfer unrelated source files, Git administrative state, target registrations, or project selections. Git bootstrap is the explicit exception that creates a complete repository checkout. Direct-child library acquisition links SHALL transfer their skill content while preserving the originating link; incoming user links SHALL be rebuilt locally. Internal skill links SHALL satisfy existing self-contained copy rules. An unreadable path SHALL be an error, never evidence of deletion.

#### Scenario: Hidden skill with supporting files

- **WHEN** a discovered hidden, unselected skill has templates and scripts
- **THEN** its complete skill directory participates in synchronization without enabling it

### Requirement: New Git sources use the initiating host's exact commit

For each Git source present on the initiating machine, the command SHALL use its origin, source identity, and exact checked-out commit as the reference. At an absent destination it SHALL clone from that origin and check out that commit before reconciling skill content. It SHALL NOT choose a newer upstream revision. A destination already containing a repository SHALL require matching origin identity and checked-out commit; a mismatch SHALL block that repository without changing its checkout. Existing non-repository destination content SHALL also block bootstrap. An unavailable origin or unobtainable reference commit SHALL produce a repository-specific failure without falling back to another revision. Sources discovered only remotely SHALL be reported as lacking an initiating Git reference and remain unchanged until aligned explicitly. No operation SHALL pull, merge, rebase, reset an existing checkout, create commits, push, or update an existing branch.

#### Scenario: Upstream has advanced

- **WHEN** the initiating checkout is at commit A and its origin branch is now at commit B
- **THEN** a new remote checkout uses commit A and the initiating checkout remains at A

#### Scenario: Existing remote checkout differs

- **WHEN** the initiating checkout is at A and an existing remote checkout is at B
- **THEN** the command reports a commit mismatch and performs no synchronization writes for that source on any participant

#### Scenario: Private or unpublished commit cannot be cloned

- **WHEN** the receiving host cannot obtain the exact initiating commit from origin
- **THEN** bootstrap fails with an actionable diagnostic and does not substitute the latest branch commit

### Requirement: Git content and local edits remain separate

For Git sources aligned at the same commit, rsync SHALL reconcile untracked skill content and uncommitted changes to tracked skill files, including supported removals and executable-mode changes. It SHALL preserve the receiving index and SHALL NOT stage transferred files. The Git commit's skill content SHALL serve as the initial comparison base for tracked files when no synchronization history exists. Existing unrelated dirty files SHALL remain untouched and SHALL NOT alone block skill synchronization at an already matching commit. Unmerged index entries within the affected source SHALL block that source. Subsequent comparisons SHALL use verified synchronization history and observed file content, not modification times alone.

#### Scenario: Local tracked skill edit on initial sync

- **WHEN** the initiating host edits a tracked skill file at commit A and the remote clone is clean at A
- **THEN** the edit transfers as an uncommitted working-tree change and the remote index remains unchanged

#### Scenario: Different edits to the same tracked file

- **WHEN** aligned hosts independently edit the same tracked skill file to different contents
- **THEN** the command reports a conflict and applies the selected conflict policy

### Requirement: Multi-host planning does not depend on host order

The initiating machine SHALL gather selected participants' observations before selecting file outcomes. A conflict-free remote edit SHALL reach the initiating machine and every other selected host in the same successful run. Comparisons SHALL distinguish unchanged values, one-sided changes, identical concurrent changes, competing changes, and unknown history. A source blocked by a commit mismatch or invalid observation SHALL be excluded from writes on all participants; independent sources SHALL continue after global preflight succeeds. Unselected hosts SHALL not affect the plan and SHALL retain their history. Concurrent initiators or content changes after observation SHALL never silently overwrite a newer value.

#### Scenario: Remote edit reaches another host

- **WHEN** only host A changes a synchronized file and hosts A and B participate
- **THEN** the same run copies that edit to the initiating host and host B

### Requirement: Conflicts require an explicit policy

`--conflict local|remote|ask` SHALL default to `ask`. `local` SHALL choose the initiating participant's observed value, including absence. `remote` SHALL choose the single distinct changed remote value; conflicting changed remote values SHALL remain unresolved. `ask` SHALL show candidate hosts and values and permit explicit choice or skip without requiring a TUI. Without interactive input and output, or with JSON or YAML output, `ask` SHALL preserve conflicting entries and report them while applying independent work. Content differences at first contact without a usable base SHALL be conflicts. The policies SHALL NOT authorize changing Git commits, escaping paths, overwriting unmanaged user materializations, or bypassing stale-write checks.

#### Scenario: Multiple remote candidates

- **WHEN** two remote hosts have different changed versions and the policy is `remote`
- **THEN** the command preserves the conflicting entries, identifies both hosts, and exits with a non-converged report

#### Scenario: Noninteractive ask

- **WHEN** `ask` encounters a conflict without interactive input and output
- **THEN** it does not prompt or alter conflicting files, completes independent changes, and returns an unsuccessful conflict report

### Requirement: Missing skill content follows an explicit policy

`--missing copy|remove|ignore` SHALL default to `copy` and apply in both directions. `copy` SHALL restore a missing entry from an available non-conflicting version. `ignore` SHALL leave one-sided absence unchanged and report it as intentionally ignored. `remove` SHALL propagate deletion only when verified history proves prior presence on the deleting and receiving participants. On first contact, `remove` SHALL report unmatched entries without deleting them or treating their absence as a historical deletion. A deletion competing with an edit SHALL be a conflict under every missing policy. Directory removal SHALL affect only proven synchronized content and SHALL preserve unrelated entries. Deselections SHALL follow the user-state rules independently of `--missing`.

#### Scenario: Default restores an unchanged deleted file

- **WHEN** one host deletes a previously shared file and the others retain its unchanged version
- **THEN** the default `copy` policy restores the file on the deleting host

#### Scenario: Explicit deletion propagation

- **WHEN** verified history shows a formerly shared file was deleted on one host and unchanged elsewhere, and `--missing remove` is selected
- **THEN** the command removes that file from participating hosts without deleting unrelated content

#### Scenario: Delete versus edit

- **WHEN** one host deletes a shared file while another edits it
- **THEN** the conflict policy decides its outcome rather than the missing policy silently discarding either change

### Requirement: Synchronization history records only verified results

The command SHALL maintain synchronization history outside Library Configuration. It SHALL associate content and desired-state observations with stable participant identities and SHALL detect a replaced host or missing history. History SHALL advance only for successfully published and verified outcomes on the affected participants. Unresolved, skipped, stale, or failed work SHALL retain its prior baseline. History loss SHALL revert comparisons to conservative first-contact behavior and SHALL NOT authorize deletion. A later run after partial failure SHALL reconcile actual state and retained history without assuming global success.

#### Scenario: Interrupted transfer

- **WHEN** a connection fails after some independent files have completed
- **THEN** the report names completed and failed work, failed entries retain their prior history, and retry preserves unresolved edits

### Requirement: Preview and application preserve recoverable state

`--check` SHALL inspect and plan without persistent writes on any participant, including clones, fetches into existing repositories, locks, staging, or history. It SHALL not prompt for conflict resolution. When a missing checkout prevents full inspection, it SHALL report required bootstrap and the unverified work without claiming full convergence. Application SHALL stage transfers, verify content, recheck preconditions, and preserve recoverable originals before replacement. Failed publication SHALL attempt rollback and report any recovery required. The command SHALL NOT claim a cross-host atomic transaction. Locks SHALL prevent simultaneous mutation of the same managed locations, user state, or history.

#### Scenario: Preview needs a clone

- **WHEN** a remote source is absent and the user runs `--check`
- **THEN** the report describes the exact-commit clone and unverified follow-on work without creating a repository or history

#### Scenario: Destination changes after planning

- **WHEN** a destination file changes after observation but before publication
- **THEN** publication is blocked for that entry and the intervening edit is preserved
