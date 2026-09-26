## MODIFIED Requirements

### Requirement: Every selected host passes preflight before writes

The command SHALL verify reachability, remote home discovery, compatible Skillator synchronization protocol, Git, rsync, and readable required configuration on all selected hosts before any persistent write on any participant. A failure SHALL abort the whole run without cloning, fetching into existing repositories, staging transfers, writing configuration, or advancing synchronization history. Diagnostics SHALL identify the configured host alias and the missing dependency or incompatibility. Skillator SHALL NOT install dependencies automatically. Compatibility SHALL be determined by the synchronization protocol and required capabilities, not exact application-version equality.

Absent receiving Library Configuration SHALL be treated as empty initial state, not a missing dependency. Malformed or unreadable existing required configuration SHALL fail preflight. User Scope Configuration is not synchronization input and SHALL NOT be read or validated by this command. SSH preflight SHALL require previously established host trust and SHALL NOT write trust files or persistent connection state; missing trust SHALL direct the user to establish it separately.

#### Scenario: Skillator is missing remotely

- **WHEN** a selected host does not have Skillator installed
- **THEN** the command exits unsuccessfully with `Skillator is not installed on remote host <alias>` and no participant changes

#### Scenario: Incompatible remote version

- **WHEN** a selected host cannot support the required synchronization protocol
- **THEN** the command reports the host, encountered version, and required protocol, and performs no persistent writes anywhere

#### Scenario: Receiving host has no library configuration

- **WHEN** a compatible receiving Skillator has no Library Configuration
- **THEN** preflight accepts empty initial state and application can create the required library registrations

#### Scenario: Invalid host-local user configuration

- **WHEN** a host has malformed User Scope Configuration but valid synchronization inputs
- **THEN** library synchronization proceeds without reading or modifying that user configuration

### Requirement: All skill content participates

Synchronization SHALL include all discovered skills regardless of visibility or selection, respect location exclusions, and transfer each complete skill directory including supporting files. Invalid discovered skills SHALL retain their diagnostics. The transfer scope SHALL also retain previously synchronized paths needed to recognize deletion or a missing `SKILL.md`. Rsync SHALL NOT transfer unrelated source files, Git administrative state, target registrations, project selections, User Scope Configuration, or user materializations. Git bootstrap is the explicit exception that creates a complete repository checkout. Direct-child library acquisition links SHALL transfer their skill content while preserving the originating link. Internal skill links SHALL satisfy existing self-contained copy rules. An unreadable path SHALL be an error, never evidence of deletion.

#### Scenario: Hidden skill with supporting files

- **WHEN** a discovered hidden, unselected skill has templates and scripts
- **THEN** its complete skill directory participates in synchronization without enabling it

### Requirement: Missing skill content follows an explicit policy

`--missing copy|remove|ignore` SHALL default to `copy` and apply in both directions. `copy` SHALL restore a missing entry from an available non-conflicting version. `ignore` SHALL leave one-sided absence unchanged and report it as intentionally ignored. `remove` SHALL propagate deletion only when verified history proves prior presence on the deleting and receiving participants. On first contact, `remove` SHALL report unmatched entries without deleting them or treating their absence as a historical deletion. A deletion competing with an edit SHALL be a conflict under every missing policy. Directory removal SHALL affect only proven synchronized content and SHALL preserve unrelated entries. User selections and deselections SHALL remain host-local under every missing policy.

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

The command SHALL maintain synchronization history outside Library Configuration. It SHALL associate content observations with stable participant identities and SHALL detect a replaced host or missing history. User desired state and materialization outcomes SHALL NOT enter synchronization history. History SHALL advance only for successfully published and verified outcomes on the affected participants. Unresolved, skipped, stale, or failed work SHALL retain its prior baseline. History loss SHALL revert comparisons to conservative first-contact behavior and SHALL NOT authorize deletion. A later run after partial failure SHALL reconcile actual state and retained history without assuming global success.

#### Scenario: Interrupted transfer

- **WHEN** a connection fails after some independent files have completed
- **THEN** the report names completed and failed work, failed entries retain their prior history, and retry preserves unresolved edits

### Requirement: Preview and application preserve recoverable state

`--check` SHALL inspect and plan without persistent writes on any participant, including clones, fetches into existing repositories, locks, staging, or history. It SHALL not prompt for conflict resolution. When a missing checkout prevents full inspection, it SHALL report required bootstrap and the unverified work without claiming full convergence. Application SHALL stage transfers, verify content, recheck preconditions, and preserve recoverable originals before replacement. Failed publication SHALL attempt rollback and report any recovery required. The command SHALL NOT claim a cross-host atomic transaction. Locks SHALL prevent simultaneous mutation of the same managed library locations or history and coordinate with other library writers.

#### Scenario: Preview needs a clone

- **WHEN** a remote source is absent and the user runs `--check`
- **THEN** the report describes the exact-commit clone and unverified follow-on work without creating a repository or history

#### Scenario: Destination changes after planning

- **WHEN** a destination file changes after observation but before publication
- **THEN** publication is blocked for that entry and the intervening edit is preserved

## ADDED Requirements

### Requirement: Batched payload transfers retain per-entry safety

The command SHALL batch explicitly selected staged skill payloads per peer through the initiating host rather than starting a transfer process for each file. It SHALL NOT require connections between remote hosts, transfer unrelated staged files, or use broad mirror deletion. Each entry SHALL retain source validation, staged-content verification, destination preconditions, recoverable publication, and independent acknowledgement. A failed batch SHALL NOT authorize publication of unverified payloads or advance their history.

#### Scenario: Several files from a remote host

- **WHEN** multiple skill files from one remote are selected for propagation to the initiating host and another remote
- **THEN** payloads travel in peer batches through the initiator while each file retains independent publication verification

#### Scenario: Batch failure and retry

- **WHEN** a payload batch fails after transferring only some bytes
- **THEN** affected unverified entries remain unpublished and unacknowledged, independent verified work may complete, and retry reobserves current content
