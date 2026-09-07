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
- Clone remote repositories or create, remove, or prune Git worktrees.
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

### Store Target registration separately from Library configuration

Configured Target worktrees will be recorded in `~/.skillator/targets.yaml` as canonical absolute paths. Each worktree is a separate entry because Repository Configuration is clone-local. Successful Target initialization, Target CLI mutation, and Target TUI save will register the selected worktree. Missing paths remain as unavailable entries so the user can see stale registrations later.

`library remove` will load valid Repository Configuration from every available registered Target plus User Scope and report Enablements whose Skills disappear from the resulting Library Snapshot. It will preserve those declarations. Invalid or unavailable registered Targets produce diagnostics and do not block removal unless registry state itself is invalid.

Adding Targets to `library.yaml` was considered but rejected. Library Locations and Target history have different lifecycles, and a separate strict file lets a future Target switcher evolve without changing Library Configuration.

### Extend reports with operation-specific payloads under common rules

Each command family will return a typed compact report with `format_version`, status, exit status, mode, affected scope, outcomes, and diagnostics. Library inventory reports will contain Sources and Skills. Mutation reports will contain configuration and Materialization outcomes. JSON and YAML will encode the same logical value and preserve deterministic ordering.

One universal report struct was considered. Optional fields for unrelated command families would make the schema harder to understand and version. Shared enums and rendering conventions are enough.

### Package one project-owned workflow skill

The implementation will add a Skill under `.agents/skills/skillator` using the repository's skill-writing guidance. The generated `.agents/.gitignore` will ignore local state and all `.agents/skills` entries while its repository-owned exception section allow-lists this directory for tracking. Skillator will preserve that exception section and will not edit the repository root `.gitignore`. The Skill's main file will contain the ordered resolve, inspect, preview, apply, and verify process. It will point to executable help for syntax and keep only decisions that help an agent choose link, copy, removal, or escalation. Each step will end in an observable result.

Copying the full CLI reference into the skill was considered and rejected because `--help` is cheaper to inspect and cannot drift from the installed binary.

### Keep repository ownership outside Repository Configuration

The Target TUI will discover physical Skills that are not declared as Enablements and show them as repository candidates. It places the `Repository` group before every Library Source so project-owned Skills stay distinct and visible. Pressing `m` stages `repo` mode. A saved repository Skill displays `[r] repo` and Space cannot uncheck it, matching the read-only behavior of `[u] user` rows. Save adds an exact exception such as `!skills/skillator/` to the parent control file.

Repository rows never become Enablements and never resolve against the Library. This keeps Library reconciliation limited to `link` and `copy`. Existing exception lines remain the source of truth for repository ownership, so no second repository-skill list is added to `.agents/skillator.yaml`.

### Keep the persistent legend compact

The main-window legend names non-obvious actions without spelling out navigation or every mode. It labels `m` as `mode`, includes `/ filter`, and leaves the complete Target and Library mode cycles in a scrollable Help modal. The Help modal is also the sole place that advertises `q` as an `Esc` equivalent for overlays; editable text fields continue to accept a literal `q`.

### Make the Target header follow the active tab

Repository tabs keep the repository root in the `Target:` header. User tabs show their home-relative Skill Directory instead. The primary User tab therefore shows `~/.agents/skills`, which makes the header describe the files currently being managed.

### Put reconciliation workflows under sync

`skillator sync target [directory]` and `skillator sync worktree [directory]` select a workflow explicitly. Bare `skillator sync` inspects `.` with Git. A linked worktree selects worktree synchronization; a primary worktree or ordinary checkout selects Target synchronization. Invalid directories then fail through the selected workflow's existing diagnostics. The old top-level `worktree` command is removed.

Initialization moves to `skillator init [directory]`. Target Enablement mutations remain under `skillator target`, so the rename does not disturb canonical link, copy, and remove commands.

### Compose with Git instead of wrapping repository operations

Repository acquisition and worktree creation remain Git operations. An agent uses `git clone` before `library add` and `git worktree add` before `sync worktree`. Skillator starts where its own state begins: Library registration, Repository Configuration, Materializations, synchronization, and machine-local registries.

Wrapping a subset of Git was rejected because it would duplicate mature path, branch, remote, and worktree semantics while still requiring users and agents to understand Git for the rest of the lifecycle.

### Separate scope inspection from Target registry inspection

Singular `target list [repository]` reports Enablements saved in one Repository Configuration, while `user list` reports saved User Scope Enablements. Plural `targets list` reports the machine-local registry of known worktrees and their availability. This keeps the existing `target` mutation group focused on one selected Target and gives registry maintenance an unambiguous namespace.

The scope listings report declarations and their observed state. Target listing excludes inherited User Scope Skills and repository-owned physical Skills so automation can treat every row as an editable Repository Enablement.

### Prune only registrations that are definitively stale

`library prune` and `targets prune` are explicit, previewable mutations. They remove entries only when Skillator can prove the registered state no longer exists: a Library path is absent, or a Target path is absent, is no longer a Git worktree, or lacks Repository Configuration. Permission failures, I/O errors, and invalid configurations are preserved with diagnostics because they can represent repairable or temporarily unavailable state.

Each prune uses one stale-checked atomic configuration update and never deletes filesystem content. Library pruning performs the same post-removal dependency inspection as an explicit Location removal. Target pruning changes only `targets.yaml`. `targets remove` remains the deliberate escape hatch for an entry that should be forgotten even when it is not classifiable as stale.

## Risks / Trade-offs

- [Prefix filters can match several Sources] -> Listing permits multiple matches and always prints full canonical Source Keys. Mutation still requires one exact selector.
- [A combined configuration and filesystem operation is more complex than sequential commands] -> Build it on the prepared-save and reconciliation transaction types already used by the TUI, then add failure-injection tests at each publication boundary.
- [First-mutation User Scope initialization can surprise callers] -> Include the new configuration in check output and create only the documented default directory with the requested Enablement.
- [Machine report variants expand the public compatibility contract] -> Keep each report compact, version it, and test JSON and YAML logical equivalence plus deterministic order.
- [Removing a Location can break resolution for many Enablements] -> Preserve every declaration and include affected unresolved identities in diagnostics.
- [A missing Location can be a temporarily detached mount] -> Require an explicit prune command, expose the complete removal set in `--check`, and preserve every path that cannot be classified as absent.
- [Pruning Targets can remove useful history] -> Remove only provably stale entries, report each path and reason, and keep `targets remove` separate for intentional deregistration.

## Migration Plan

1. Add the command groups without changing the no-argument TUI routes or existing sync behavior.
2. Add application workflows and report types, then expose them through clap.
3. Update help and README examples after behavior tests pass.
4. Add the agent skill last so its workflow matches the implemented command help.

Rollback removes the new subcommands and agent skill. Version 1 configuration written by them remains valid and editable through the existing TUI.
