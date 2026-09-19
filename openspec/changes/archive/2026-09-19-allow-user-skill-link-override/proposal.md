## Why

Some agent harnesses do not immediately discover skills in the user's home directory.
The repository TUI currently blocks mode changes on inherited user skills, even though an explicit repository link can make the skill available there.

Issue: [#27](https://github.com/commandzero/skillator/issues/27)

## What changes

1. Allow `m` on an available `[u] user` row in a Repository tab to stage an explicit `[✓] link` Enablement in that directory.
2. Save through the existing repository reconciliation workflow, using the same Library skill identity and link destination as an ordinary repository link.
3. Let Space remove the repository override and return the row to inherited user state.
4. Keep User Scope configuration and materializations unchanged, and explain the shortcut in Help and the README.

## Capabilities

### New capabilities

None.

### Modified capabilities

1. `tui-workflows`: Allow an explicit repository link from an inherited user row and restore inheritance when the override is removed.

## Impact

Changes affect TUI row state, mode handling, save/reload tests, Help, and the README.
The existing version 1 configuration, CLI commands, and reconciliation safety rules already support the resulting repository Enablement.

## Non-goals

1. Automatic detection of harness compatibility or automatic repository links.
2. New CLI options, configuration fields, or links that depend on the user's materialized skill path.
3. Changes to User Scope editing, repository-owned skills, or bulk selection behavior.
