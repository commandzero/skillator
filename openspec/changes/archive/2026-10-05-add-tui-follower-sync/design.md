## Context

See [proposal.md](proposal.md) for motivation and [the TUI workflow delta](specs/tui-workflows/spec.md) for observable behavior. The existing leader push exports the current valid Library and uses SSH/rsync with ownership and prerequisite checks; the TUI currently registers and inspects followers but cannot push. Library/Target refreshes already use background work, cancellation, and generation guards. Local Library and host-registry saves are separate, and `Ctrl+S` normally exits after a successful save.

## Goals / Non-Goals

**Goals:** Reuse the actual leader push for a single saved alias, preserve host/registry save boundaries, and keep UI navigation and reports safe throughout confirmation, transfer and cancellation.

**Non-Goals:** New transport or persistence, automatic sync on registration, retries, CLI behavior changes, pull-from-follower delivery, atomic delivery, or a deadline for rsync/network transfer.

## Decisions

1. **One TUI-only backend entry point over the existing push pipeline.** The remote layer accepts an alias, expected destination, and cancellation flag; it reloads saved leader configuration, verifies destination equality, revalidates current Library sources, then runs the existing export, prerequisites, ownership, and rsync path for that alias alone. This retains source acquisition behavior, protected replica deletion and private export cleanup. CLI command signatures and report formats remain unchanged. Alternative: invoke the public CLI as a child or duplicate export logic; both obscure cancellation and risk divergent safety behavior.
2. **Confirmation belongs to the UI before creating a worker.** An existing remote host's `s` and `Ctrl+S` mean sync, not Local save; display the overwrite/stale-file-delete boundary of the owned replica and require explicit consent. New registrations only enter an initialization queue after a successful registry save. Process new hosts in saved registration order; declining does nothing remotely. Host-only save-and-exit waits for each decision and accepted operation to complete. Alternative: synchronize as part of Save; this would make saving a host implicitly mutate a remote machine and couple distinct failure outcomes.
3. **Use cancellable background delivery, independent from inspection.** The UI retains selected alias/destination and a generation for each worker; an active operation blocks/coalesces duplicate triggers. Leaving the host or Library scope signals cancellation, running local SSH and rsync process groups are stopped, and obsolete worker replies cannot update a new view. On current success trigger fresh inspection; on failure expose the actual report/cleanup diagnostics within the TUI, without stdout/stderr writes. Alternative: block the UI while waiting; this defeats navigation and makes in-flight cancellation impractical.
4. **Keep staged and saved state separate.** A selected follower action never persists dirty Library/registry/Target edits. Existing navigation guards handle pending changes, while the backend validates the saved destination against the UI's expected destination and refuses stale or invalid sources before replica mutation. Alternative: use the UI's cached inventory or staged host map; both permit unintended or stale delivery.

## Risks / Trade-offs

- [Cancellation races a transport completion] → Reject stale replies using host identity and generation; ensure canceled transport and export cleanup are attempted even if UI has navigated away.
- [Ordinary rsync can partially mutate an owned replica] → Describe interruption and repeat-delivery convergence accurately; do not promise rollback or atomic transfer.
- [A new-host save succeeds but sync fails] → Preserve registration, show the actual delivery and cleanup diagnostics separately, and allow an explicit retry from its host tab later.
- [External configuration or source changes after confirmation] → Validate fresh state at backend start and reject changed destination; no silent retargeting or cached source-based writes.

## Migration Plan

No persisted-format migration. Existing leader registrations remain eligible for explicit TUI sync. Older installations still use the CLI; restoring an older build does not remove saved registrations or existing owned replicas. No archive or PR publication is part of this implementation step.
