## MODIFIED Requirements

### Requirement: Busy and partial outcomes preserve user understanding
An active Target lock SHALL not prevent browsing or staging; Save SHALL report Target Busy with Retry or Return to Editing and MUST NOT discard edits or retry automatically. A successful `s` save SHALL reload the current workspace with the saved state; a successful `Ctrl+S` save SHALL exit immediately except that a host-registration save SHALL defer exit until every newly saved follower's initialization prompt has been declined or completed. A partial or failed save SHALL remain on a concise result screen until acknowledged, identify applied, blocked, rolled-back, or Recovery Required work, and then exit nonzero.

#### Scenario: Partial save
- **WHEN** a confirmed save applies some changes while others remain blocked
- **THEN** the TUI presents the concise partial result until acknowledgement and exits with status `1`

#### Scenario: Host-only fast save
- **WHEN** `Ctrl+S` successfully saves one or more new follower registrations
- **THEN** the TUI offers initialization for each newly saved follower in registration order before exiting, whether each offer is declined or completed

## ADDED Requirements

### Requirement: Remote Library synchronization is an explicit selected-follower action
On a selected remote Library host, `s` and `Ctrl+S` SHALL request synchronization of only that saved follower from the authoritative leader; neither key SHALL save staged Local Library, host-registry, User, or Repo edits or exit the TUI. On Local Library, `s` and `Ctrl+S` SHALL retain their save behavior. Before transfer, the TUI SHALL ask for explicit confirmation that content may be overwritten and stale files deleted only inside the follower's owned `~/.skillator/library/replica`; rejecting confirmation SHALL perform no delivery. Pending edits SHALL NOT implicitly authorize, save, or supply a remote transfer.

#### Scenario: Selected follower sync
- **WHEN** the user presses `s` or `Ctrl+S` while viewing the saved follower `build` and confirms the sync
- **THEN** only `build` is targeted by a leader-to-follower delivery and the TUI does not exit or save staged local configuration

#### Scenario: Local save remains local
- **WHEN** the user presses `s` while viewing Local Library
- **THEN** the ordinary confirmed Local save workflow is offered rather than a follower transfer

#### Scenario: Declined overwrite and deletion
- **WHEN** the user declines confirmation to sync a follower
- **THEN** no replica mutation or transfer starts and the registration remains unchanged

#### Scenario: Pending edits cannot authorize sync
- **WHEN** a selected follower has pending Local Library or host-registry changes
- **THEN** the existing save/discard/return guard applies and unsaved edits are neither written nor used as transfer input

### Requirement: Saved follower registration offers optional initialization
After a successful save creates one or more new follower registrations, the TUI SHALL ask whether to initialize each newly saved follower in registration order. Accepting SHALL use the same explicitly confirmed, selected-follower synchronization workflow; declining SHALL keep the registration and create no replica. Merely registering or browsing a follower SHALL NOT deliver content. A failed or unsaved registration SHALL NOT prompt for synchronization.

#### Scenario: Decline new follower initialization
- **WHEN** the user saves a new follower registration and declines its initialization prompt
- **THEN** its alias and destination remain saved but no replica is created by that prompt

#### Scenario: Multiple saved followers
- **WHEN** a save persists two new followers
- **THEN** each follower receives its own initialization decision, in registration order, without implicitly synchronizing either one

#### Scenario: Failed registration
- **WHEN** saving a staged new follower fails
- **THEN** no initialization prompt or transfer is started for that follower

### Requirement: TUI follower delivery uses fresh validated leader state
A confirmed remote sync SHALL re-read saved leader configuration, require that the selected alias still exists with its expected saved destination, and revalidate the leader Library sources before invoking the established owned-replica prerequisite, export, and rsync pipeline. It SHALL preserve the existing source acquisition modes and read-only treatment of leader skill content. Invalid, changed, unavailable, or ambiguous required input SHALL stop delivery before replica mutation and provide a useful diagnostic. It SHALL NOT change CLI behavior, persisted configuration formats, or follower-local selection/materialization state.

#### Scenario: Externally changed host
- **WHEN** a selected alias's saved destination changes or disappears after confirmation but before delivery
- **THEN** the transfer is refused instead of targeting the replacement or another follower

#### Scenario: Invalid leader source
- **WHEN** a source becomes unavailable or invalid after the TUI's cached inventory was rendered
- **THEN** fresh export validation reports the problem and no replica mutation begins

### Requirement: Follower delivery keeps the TUI responsive and reports the actual result
The TUI SHALL keep navigation responsive while one confirmed follower sync is running, SHALL NOT start duplicate transfers for repeated actions, SHALL cancel outstanding delivery on leaving its host or scope and ignore obsolete results. Cancellation SHALL stop running local SSH/rsync process groups and safely clean private exports; an interrupted transfer MAY leave a partially updated owned replica. Transfer and cleanup failures SHALL be visible with useful diagnostics, without leaking worker output to the terminal. Successful delivery SHALL refresh the selected follower's inspection; cancellation or stale replies SHALL NOT overwrite a different host's view. The TUI SHALL NOT claim atomic delivery, rollback, a transfer deadline, or bounded network operation.

#### Scenario: Duplicate keys while delivery is active
- **WHEN** the user presses `s` or `Ctrl+S` repeatedly while that follower's sync is in progress
- **THEN** at most one transfer is running and the UI remains responsive

#### Scenario: Leave selected host during delivery
- **WHEN** the user switches host or scope during a follower sync
- **THEN** local transport is canceled, private export cleanup is attempted, and a late result cannot replace the new view

#### Scenario: Successful sync refresh
- **WHEN** the selected follower sync succeeds and remains current
- **THEN** inspection refreshes so the newly delivered skills become visible without restarting the TUI

#### Scenario: Transfer or cleanup fails
- **WHEN** rsync, SSH, or private-export cleanup fails
- **THEN** the TUI reports the actual failure and any retained private export path needed for manual cleanup, without claiming replica rollback
