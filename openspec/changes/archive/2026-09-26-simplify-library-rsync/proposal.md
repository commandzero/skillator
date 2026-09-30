## Why

The remote library synchronization implementation grew beyond the core two-way skill-content workflow. Reduce duplicated safety orchestration and transfer overhead while retaining exact-commit Git bootstrap and verified, recoverable publication.

## What Changes

1. Consolidate stage identity and guarded transfers, low-level file operations, Git comparisons, and test fixtures.
2. **BREAKING**: Keep user enablements, skill-directory definitions, and materializations host-local. Library synchronization no longer reads, merges, publishes, or acknowledges user desired state.
3. Batch explicit staged payloads per peer through the initiating host. Preserve per-entry export validation, publication, acknowledgement, and recovery.
4. Retain exact-commit Git bootstrap, acquisition aliases, all-host preflight, conflict and missing policies, containment, and locks.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

1. `library-rsync`: Content-only synchronization, batched guarded payload transfers, and content-only history.
2. `user-scope-onboarding`: Remove remote user-state merge and materialization requirements.
3. `cli-contract`: Describe library-content synchronization without user-selection replication.

## Impact

Changes affect the private remote protocol, coordinator, session, snapshot/history, CLI help, and remote-only application helpers. Normal user-scope commands remain unchanged. Protocol incompatibility is reported before mutation; no compatibility shim is added for the unreleased protocol. Main specs and user documentation will describe the revised scope.
