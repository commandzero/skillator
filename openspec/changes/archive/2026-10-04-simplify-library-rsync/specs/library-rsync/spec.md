# Spec Delta

## ADDED Requirements

### Requirement: Hosts are configured explicitly

Skillator SHALL read optional version 1 main configuration from `~/.skillator/config.yaml`. A leader SHALL use a `hosts` mapping from unique follower aliases to entries containing one required SSH `destination` string. A follower SHALL use one `leader` entry containing one required SSH `destination` string instead. Configuring both forms, or neither usable form, SHALL fail before connecting. It SHALL reject unknown fields, duplicate keys, unsupported versions, empty destinations, and destinations interpreted as command options. Configuration SHALL remain machine-local. Leaders SHALL select all configured followers unless `--hosts` supplies a comma-separated subset of known aliases. Duplicate requested aliases SHALL be deduplicated; empty or unknown aliases SHALL fail before connecting. Followers SHALL reject `--hosts` and contact only their leader. SSH SHALL use existing user configuration and authentication without copying credentials. Aliases SHALL be selection/report labels, not persistent participant identities.

#### Scenario: Explicit host subset
- **WHEN** configuration contains `build` and `dev` and the command selects `--hosts dev`
- **THEN** only `dev` receives files and no connection is made to `build`

#### Scenario: Follower configuration
- **WHEN** configuration has `leader.destination` and the user runs `skillator library rsync`
- **THEN** the command pulls from that leader into the local follower replica without contacting any other follower

#### Scenario: Ambiguous role configuration
- **WHEN** configuration contains both `hosts` and `leader`
- **THEN** the command reports invalid configuration before connecting or writing

### Requirement: All skill content participates

Delivery SHALL include all skills discovered on the leader regardless of visibility or selection, respect the leader's location exclusions, and transfer complete skill directories including supporting files and executable modes. Acquisition links SHALL deliver their skill content without changing the originating link. Internal links SHALL follow existing self-contained copy rules. Unrelated source files, Git administrative state, configurations, registrations, selections, and materializations SHALL NOT be transferred.

#### Scenario: Hidden skill with supporting files
- **WHEN** the main host has a hidden, unselected skill containing templates and executable scripts
- **THEN** the receiver gets its complete directory without enabling the skill or changing the main host

#### Scenario: Acquired skill is linked locally
- **WHEN** an acquisition link exposes a valid skill
- **THEN** the receiver gets usable skill content rather than a link back to the main host, and the original local link remains unchanged

#### Scenario: Equal names from different sources
- **WHEN** two local Sources contain a skill at the same relative path
- **THEN** both are delivered under distinct source groupings without overwriting each other

### Requirement: The leader is authoritative regardless of initiator

On the leader, `library rsync` SHALL push its current skill export to configured followers. On a follower, it SHALL pull a freshly prepared export from its configured leader into its local replica. It SHALL NOT upload follower edits, compare editing histories, prompt for conflicts, or manage Git checkouts. In either invocation, differing follower content SHALL be replaced and stale replica content removed according to the leader's complete export.

#### Scenario: Main-host update
- **WHEN** the user changes a local skill and runs `skillator library rsync --hosts dev`
- **THEN** `dev` receives the updated content and the local content is unchanged

#### Scenario: Follower pulls a fresh leader update
- **WHEN** the leader changes a skill without first running a push and the follower runs `skillator library rsync`
- **THEN** the follower receives the leader's current content, not a cached export from an earlier push, and the leader's skill files remain unchanged

#### Scenario: Follower edits never propagate upstream
- **WHEN** a follower edits its replica and then runs `skillator library rsync`
- **THEN** the leader's content replaces the local replica edit and no follower skill content is uploaded

#### Scenario: Receiver edits a replica
- **WHEN** a receiving host edits a copied skill and the main host pushes again
- **THEN** the main host's content replaces that edit without a conflict prompt or any reverse transfer

#### Scenario: Main-host deletion
- **WHEN** a complete local export no longer contains a previously delivered skill or supporting file
- **THEN** the next leader push or follower pull removes it from the follower replica without deleting anything outside that replica

### Requirement: Receiver replicas are isolated

Files SHALL be delivered beneath the dedicated `~/.skillator/library/replica` directory in each receiving user's home, with stable source-relative grouping. The command SHALL create an absent replica and mark it as managed; it SHALL refuse an existing unmarked destination. It SHALL reject symlinked destination ancestors or replica roots before writes. Mirror deletion SHALL be confined to the managed replica, never the home, arbitrary checkouts, or other library content.

#### Scenario: First delivery
- **WHEN** the selected host has no replica directory
- **THEN** application creates the dedicated replica and delivers the main host's skills without registering external locations or cloning repositories

#### Scenario: Unmanaged destination already exists
- **WHEN** the replica path already exists without the command's ownership marker
- **THEN** the command refuses to overwrite or delete it and gives guidance to move that content aside

#### Scenario: Unrelated receiver library content
- **WHEN** a receiver has other library directories and stale content inside the managed replica
- **THEN** either a push or a pull removes only stale replica content and preserves all other library directories and configuration

#### Scenario: Destination root redirects elsewhere
- **WHEN** a replica root or its below-home ancestors are symbolic links
- **THEN** the host is rejected before application and no write is made through those links

### Requirement: Incomplete leader input does not authorize mirroring

Before changing any follower replica, Skillator SHALL obtain a complete usable export of the leader's current library using normal discovery and exclusions on the leader. Unavailable configured Locations, unreadable content, invalid discovered skills, unsupported links, overlapping export paths, and source-key collisions SHALL fail with actionable diagnostics rather than silently shrinking the export. A successfully inspected empty leader library SHALL be valid input, distinct from unavailable input.

#### Scenario: Configured source is unavailable
- **WHEN** a configured leader Location cannot be read during a push or follower-initiated pull
- **THEN** no receiver changes and the command reports unavailable input rather than deleting the receiver's corresponding skills

#### Scenario: Intentionally empty library
- **WHEN** all configured Locations are available and complete discovery finds no skills
- **THEN** application empties only an already managed replica while preserving its ownership marker

### Requirement: Delivery uses existing SSH and rsync

The command SHALL use ordinary rsync over existing SSH authentication. A pushed-to follower SHALL need SSH access, a usable POSIX shell, rsync, and ordinary POSIX filesystem utilities including `find`, but SHALL NOT require Skillator or Git. A follower initiating a pull SHALL run Skillator locally and require SSH, rsync, Skillator, and `cmp`, `rm`, `find`, and `chmod` on its leader for export preparation, validation, and cleanup. Required dependencies and destination suitability SHALL be checked before replica writes; leader export validation and cleanup utilities SHALL be checked before creating a temporary export. Failures SHALL identify the follower alias or leader. Skillator SHALL NOT install dependencies or establish host trust automatically.

#### Scenario: No remote Skillator or Git
- **WHEN** the selected receiver has SSH, a usable POSIX shell, rsync, and required POSIX filesystem utilities but neither Skillator nor Git
- **THEN** skill delivery succeeds without installing either application

#### Scenario: Missing rsync
- **WHEN** a selected receiver lacks rsync
- **THEN** that host receives no writes and its failure is reported with installation guidance

#### Scenario: Leader cannot prepare a pull export
- **WHEN** a follower's leader cannot run Skillator or complete discovery
- **THEN** the pull fails with an actionable leader diagnostic and leaves the follower replica unchanged

#### Scenario: Missing leader export utility
- **WHEN** the leader's SSH PATH lacks `cmp`, `rm`, `find`, or `chmod`
- **THEN** the pull fails before creating a temporary export or changing the follower replica

### Requirement: Preview is write-free

`--check` SHALL preview a leader push or follower pull without persistent changes or prompts. It SHALL use ordinary rsync dry-run for an existing managed replica and report creation/delivery for an absent replica without creating it. Temporary exports SHALL be cleaned after inspection; failed cleanup SHALL be reported. It SHALL return `1` for required, failed, or unverified work and `0` only when every affected replica matches the current leader export.

#### Scenario: Read-only skill directory cleanup
- **WHEN** valid leader skills contain read-only skill-root or supporting directories
- **THEN** push, pull, and preview preserve delivered directory modes and remove their temporary exports without changing source file modes or modification times

#### Scenario: Preview first delivery
- **WHEN** the replica is absent and the user runs `--check`
- **THEN** the report describes replica creation and delivery, returns `1`, and creates no remote directory, marker, or configuration

#### Scenario: Preview stale remote content
- **WHEN** an existing managed replica differs from the main host
- **THEN** preview reports planned updates and removals without changing either host

#### Scenario: Follower previews a pull
- **WHEN** a follower runs `skillator library rsync --check`
- **THEN** it compares the leader's current export with its local replica, preserves both libraries and local replica files, and removes temporary export data

### Requirement: Failures use ordinary retryable rsync outcomes

Leader pushes SHALL deliver the same export independently to selected followers. A follower pull SHALL affect only that follower. Failures SHALL NOT reverse another follower's successful work or modify leader skill content. Reports SHALL distinguish direction and successful, failed, or previewed work without claiming cross-host atomicity or protection of follower edits. Rerunning either invocation SHALL converge from current files without synchronization history or enrollment.

#### Scenario: One receiver is unreachable
- **WHEN** `build` is unreachable but `dev` is available
- **THEN** `dev` can receive the export, `build` is reported failed, and the aggregate result is unsuccessful

#### Scenario: Interrupted delivery
- **WHEN** rsync fails after some receiver files have changed
- **THEN** the report identifies the failed receiver without claiming complete success or rollback, and a later successful push converges its replica

