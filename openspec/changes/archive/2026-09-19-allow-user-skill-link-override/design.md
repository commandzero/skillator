## Context

Repository rows already distinguish inherited User Scope skills from explicit repository Enablements.
The current TUI treats inherited rows as read-only, while the existing target link workflow supports the requested saved state.

## Decisions

1. Treat `m` on an inherited row as a request for a normal repository link. Preserve its Source Key and Skill path, and resolve the link through the Library. Do not link through the user's home materialization.
2. Retain inheritance information while an explicit override is staged or saved. Space removes only the repository Enablement and restores `[u] user`; it cannot disable User Scope.
3. Keep ordinary link/copy mode behavior once an explicit override exists. This change adds only the entry from inherited state and the return to it.
4. Require the same valid, registered, available Library skill as any new repository Enablement. An unresolved inherited row stays unchanged and explains why a link cannot be staged.
5. Reuse existing staged-save, containment, collision, confirmation, and removal rules. Replacing recoverable conflicting content requires explicit confirmation; Blocked conflicts remain blocked even with confirmation. Staging an override alone does not authorize replacement.

## Validation

Test staging, cancellation, saving and reloading, removal, unavailable sources, and destination conflicts.
Verify that every repository-only action leaves User Scope configuration and skill entries unchanged.

## Migration

No migration is needed. Saved overrides use ordinary version 1 repository Enablements.
