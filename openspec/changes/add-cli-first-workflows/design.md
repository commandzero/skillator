## Context

The current CLI dispatches two TUIs and two reconciliation commands. Library and Target mutation logic already lives below the TUI in application workflows, with strict codecs, configuration fingerprints, prepared reconciliation plans, guarded authorization, staging, and rollback. The new commands must reuse those boundaries. Editing YAML directly in CLI handlers would create a second safety model.

Skill identity is a Source Key plus its path relative to the Source. Frontmatter names and directory basenames are display data and can collide. Repository Configuration can contain several Skill Directories, while User Scope commonly begins without configuration.

The CLI report schema currently describes reconciliation only. CLI-first configuration commands need comparable stable output without turning one report into a dump of every internal type.

## Goals / Non-Goals

**Goals:**

- Keep clap parsing, path selection, and rendering in the CLI layer.
- Give the TUI and non-interactive commands one set of configuration and reconciliation rules.
- Make every mutation previewable, idempotent, stale-checked, and scriptable.
- Give agents a short workflow with checkable completion criteria.

**Non-Goals:**

- Search Skill names, descriptions, or remote marketplaces.
- Register individual Sources or Skills. Discovery remains live.
- Add arbitrary Target Skill Directories in this first slice.
- Delete a Library Location directory when unregistering it.
- Add migration aliases for alternate command spellings.

## Decisions

### Add typed application commands above existing workflows

The CLI will convert clap arguments into presentation-neutral requests for Library Location edits, inventory listing, Target initialization, and Enablement edits. Application workflows will load state, resolve selectors, produce prepared operations, and commit them. CLI handlers will only choose check or apply mode and render the result.

This keeps filesystem policy out of `cli.rs` and makes workflow tests independent of terminal rendering. Calling the TUI reducer from the CLI was considered, but reducers contain staged UI state and confirmation behavior that do not belong in automation.

### Prepare desired-state and reconciliation changes together

Target and User link, copy, and remove commands will derive the proposed configuration and its reconciliation plan from one session snapshot. Commit will recheck configuration fingerprints and filesystem preconditions before writing. A blocked requested Materialization prevents publication of desired state that claims success. Recoverable failures use the existing staged publication and rollback machinery.

Running a configuration edit followed by the existing `sync` command was considered. That approach can leave a saved Enablement whose requested Materialization was known to be blocked during the same invocation, and it creates a race between two independently loaded snapshots.

### Use canonical selectors at the mutation boundary

Mutation commands accept `<source-key>:<skill-path>`. The parser splits on the final colon, validates both existing domain types, and resolves the pair against the current Library Snapshot. The colon gives a boundary that remains unambiguous when both fields contain slashes.

Bare display-name lookup was considered but rejected for the first slice. It makes scripts depend on mutable and non-unique metadata. `library list` supplies the canonical fields agents and users need.

### Filter Library inventory by Source Key prefix

`library list [filter]` performs a case-insensitive prefix comparison against canonical Source Keys. `elastic` therefore matches sources owned by Elastic, while `elastic/agent-skills` narrows the result. The filter does not inspect Skill metadata.

Substring and fuzzy matching were considered. Both make a short filter harder to reason about in scripts and can change matches when unrelated Sources appear. Prefix matching has a simple stable rule.

Configured paths use the separate `library locations` command. Overloading `library list` with locations and Skills would make its output depend on flags and weaken its use as the canonical selector discovery command.

### Default Skill Directory selection follows saved keys

Target mutations use `--directory <key>` when supplied. Without it, they select `agents` when present or the sole configured directory. Any other case is ambiguous and returns the available keys without writing. Target initialization creates only the existing `agents` preset.

Guessing from compatible agent paths was considered but rejected because compatibility is advisory and directory keys are the saved identity.

### Extend reports with operation-specific payloads under common rules

Each command family will return a typed compact report with `format_version`, status, exit status, mode, affected scope, outcomes, and diagnostics. Library inventory reports will contain Sources and Skills. Mutation reports will contain configuration and Materialization outcomes. JSON and YAML will encode the same logical value and preserve deterministic ordering.

One universal report struct was considered. Optional fields for unrelated command families would make the schema harder to understand and version. Shared enums and rendering conventions are enough.

### Package one project-owned workflow skill

The implementation will add a Skill under `.agents/skills` using the repository's skill-writing guidance. Its main file will contain the ordered resolve, inspect, preview, apply, and verify process. It will point to executable help for syntax and keep only decisions that help an agent choose link, copy, removal, or escalation. Each step will end in an observable result.

Copying the full CLI reference into the skill was considered and rejected because `--help` is cheaper to inspect and cannot drift from the installed binary.

## Risks / Trade-offs

- [Prefix filters can match several Sources] -> Listing permits multiple matches and always prints full canonical Source Keys. Mutation still requires one exact selector.
- [A combined configuration and filesystem operation is more complex than sequential commands] -> Build it on the prepared-save and reconciliation transaction types already used by the TUI, then add failure-injection tests at each publication boundary.
- [First-mutation User Scope initialization can surprise callers] -> Include the new configuration in check output and create only the documented default directory with the requested Enablement.
- [Machine report variants expand the public compatibility contract] -> Keep each report compact, version it, and test JSON and YAML logical equivalence plus deterministic order.
- [Removing a Location can break resolution for many Enablements] -> Preserve every declaration and include affected unresolved identities in diagnostics.

## Migration Plan

1. Add the command groups without changing the no-argument TUI routes or existing sync behavior.
2. Add application workflows and report types, then expose them through clap.
3. Update help and README examples after behavior tests pass.
4. Add the agent skill last so its workflow matches the implemented command help.

Rollback removes the new subcommands and agent skill. Version 1 configuration written by them remains valid and editable through the existing TUI.
