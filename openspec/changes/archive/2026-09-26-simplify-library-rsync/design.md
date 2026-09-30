## Context

See proposal.md. The current implementation is archived as library-rsync and uses protocol 4. Stage metadata and validation are spread across coordinator, transport, and session. File transfers run individually. User-state merging adds a second reconciliation workflow.

## Goals / Non-Goals

1. Keep the coordinator responsible for planning and per-entry outcomes; put stage containment, identity checks, and guarded transfers behind one boundary.
2. Reduce code and process duplication without weakening publication preconditions or recovery.
3. Keep exact-commit Git bootstrap and acquisition aliases. Do not replace two-way reconciliation with sequential mirroring, or add remote-to-remote connections.
4. Do not change ordinary user-scope commands or automatically refresh copied user materializations.

## Decisions

1. Delete remote user snapshots, user-history entries, key parsing, planner selection mode, protocol commands, and remote-only application/reconciliation helpers. Retain the user-home lock because library mutation uses it. Do not add a user-key abstraction for a workflow being removed.
2. Use a shared stage descriptor for the participant home, stage path, and device/inode identity. Opening rechecks physical containment and identity; local session ownership also retains the parent directory handle needed for safe cleanup. Keep clone-stage validation distinct where its destination-filesystem rules differ.
3. Guard both endpoints before and after each batched transfer. Keep source export validation and per-entry destination publication, journals, verification, and acknowledgement unchanged. Batch only successful staged exports using explicit flat generated payload names; no broad directory mirror, recursive symlink traversal, or deletion flags. Route remote-to-remote outcomes through the initiator.
4. Plan all file outcomes first, collect exports by source participant, transfer each source batch to initiating staging, distribute payload batches by destination, then publish each planned entry in the existing safe order. A failed batch does not acknowledge unverified files; independent transfers and metadata-only operations may continue. Preview remains write-free.
5. Keep shared file hashing/copying and Git-validation helpers small. Preserve all existing observation checkpoints and phase-specific guards; do not cache externally mutable Git state across checks.
6. Bump the private protocol for content-only snapshots and batched transfer framing. This unreleased branch receives a clean cutover, not compatibility aliases. Keep history validation strict and make any on-disk format change explicit.
7. Consolidate deterministic library/skill fixtures. Remove tests solely for deleted user-sync behavior, retain unrelated safety cases, and cover host-local state preservation and batch partial failure.

## Risks / Trade-offs

1. A moved stage can retain its inode while leaving the allowed home. Identity checks never replace containment checks.
2. Batching enlarges the failed transfer unit but not the publication/acknowledgement unit. Verified staging and unchanged per-entry publication prevent silent partial success.
3. Acquisition links and internal skill links have different containment semantics and remain distinct.
4. Copied user skills no longer refresh as a side effect of library rsync. Users reconcile them explicitly with host-local commands.
