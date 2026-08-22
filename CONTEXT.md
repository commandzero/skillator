# Skillator

Skillator manages which agent skills from a Library are active in the User Scope or in one or more Skill Directories in a Target Repository.

## Language

**Library**:
A user's live inventory of Sources and available Skills discovered beneath configured Library Locations. Presence in the Library does not make a Skill active.
_Avoid_: Registry, static inventory

**Library Location**:
A user-configured local path that contributes Sources to the Library. Nested Git repositories form independent Sources; Skills outside a nested Git repository belong to the Library Location's local Source.
_Avoid_: Source, Library

**Source**:
A discovery boundary within a Library Location that contributes one or more Skills. A Source is either a Git repository or the Library Location's local region outside nested Git repositories.
_Avoid_: Library, marketplace

**Source Key**:
A portable, case-insensitive identifier derived from a discovered Source. Keys normalize to lowercase slash-separated segments; Git Sources default to `owner/repository` and non-Git Sources to `local/name`. A collision is surfaced rather than silently resolved.
_Avoid_: Repository name, remote URL, local path

**Unavailable Source**:
A Source that cannot be discovered because its Library Location is missing or unreadable. It is absent from the live inventory; existing Target Enablements that name it remain declarative but cannot be resolved.
_Avoid_: Removed Source, empty Source

**Skill**:
A directory supplied by a Source that contains a `SKILL.md` file.
_Avoid_: Package, plugin

**Unavailable Skill**:
A Skill named by an existing Target Enablement that is absent from the current Library Snapshot. Its Enablement remains declarative but cannot be materialized or synchronized.
_Avoid_: Invalid Skill

**Skill Key**:
The stable identity of a Skill, formed from its Source Key and slash-normalized directory path relative to the Source. A Skill's frontmatter name and directory basename are display metadata, not identity.
_Avoid_: Skill name, absolute path

**Invalid Skill**:
A discovered Skill whose `SKILL.md` does not provide valid required metadata. It remains visible with diagnostics but cannot receive new Enablements.
_Avoid_: Missing Skill, unavailable Skill

**Overlapping Library Locations**:
Separately configured Library Locations whose canonical discovery boundaries overlap. Configuration is rejected by default; an explicit override preserves distinct discovered identities while surfacing warnings on affected Enablements.

**Target Repository**:
A Git repository whose active Skills Skillator manages. It is selected from the current working directory or an explicit directory argument.
_Avoid_: Source, skill directory, destination

**User Scope**:
Machine-local desired state shared by agent sessions regardless of repository. It is configured at `~/.agents/skillator.yaml`, appears as the first tab of a Target workspace, and is inherited by every Target Repository view.
_Avoid_: User Target, global Library, repository target

**Inherited User Enablement**:
A Skill active through the User Scope while viewing a Repository Skill Directory. It is displayed as `[u] user`, is read-only from the Repository tab, and can be changed only from its User Scope tab.
_Avoid_: Repository Enablement, copied Skill, implicit repository link

**Skill Directory**:
A managed exposure boundary where active Skills are materialized for discovery by one or more agents, such as `.agents/skills` or `.claude/skills`. Its path is relative to the repository root in Repository Configuration and relative to the user's home directory in User Scope Configuration. Either scope may configure several non-overlapping Skill Directories with different Enablements.
_Avoid_: Target, library

**Skill Directory Control File**:
The canonical, Skillator-owned `.gitignore` in the parent of a Repository Skill Directory (for example, `.agents/.gitignore` for `.agents/skills`). It ignores Skillator-managed materializations while explicitly allowing the repository configuration and pre-existing unmanaged entries. It remains eligible for repository tracking. User Scope Skill Directories do not use control files.
_Avoid_: Repository ignore policy, user `.gitignore`

**Skill Directory Key**:
A repository-scoped, case-insensitive, lowercase kebab-case identifier for a Skill Directory. Enablements refer to this stable key rather than to an agent name or filesystem path.
_Avoid_: Agent name, directory path, preset key

**Skill Directory Preset**:
A creation-time template that suggests explicit metadata and a default path for a Skill Directory. After saving, a directory created from a built-in preset behaves exactly like an arbitrary user-configured Skill Directory.
_Avoid_: Directory type, agent runtime

**Agent Compatibility**:
Informational metadata derived from a normalized Skill Directory path, describing agents documented to discover that path. It does not guarantee that the directory is exclusive to those agents.
_Avoid_: Agent ownership, agent isolation

**Enablement**:
The desired relationship that makes a Skill active in one specific Skill Directory, which may expose it to several compatible agents. Its desired state may differ from the Materialization currently observed on disk.
_Avoid_: Installation, link

**Unresolved Enablement**:
A valid Enablement whose Source or Skill cannot be resolved through the current machine's Library configuration. Its desired state remains intact even though Skillator cannot currently materialize it.
_Avoid_: Invalid Enablement, disabled Skill

**Repository Configuration**:
The declarative `.agents/skillator.yaml` file describing a Target Repository's Skill Directories and desired Enablements. It identifies Sources portably while user-level Library configuration resolves them to machine-specific paths.
_Avoid_: Library configuration, observed state

**User Scope Configuration**:
The machine-local `~/.agents/skillator.yaml` file describing User Scope Skill Directories and desired Enablements. It uses the same desired-state shape as Repository Configuration but resolves paths relative to the user's home directory and has no Git tracking contract.
_Avoid_: Library configuration, Repository Configuration

**First-run Library Welcome**:
The one-time-in-process welcome shown when Library Configuration is absent. It opens the normal Library workspace with the default `./library` Location; the user configures Locations through ordinary Library controls and then uses `Ctrl+L` to manage the current Target. It performs no automatic acquisition or User Scope changes.
_Avoid_: onboarding mode, schema migration, marketplace, automatic acquisition

**Observed State**:
An immutable snapshot of filesystem facts for the configured Skill Directories and their immediate entries. It supports comparison with desired state without prescribing reconciliation actions.
_Avoid_: Desired state, repair plan

**Expected Entry**:
The immediate child path within a Skill Directory where an Enablement's Materialization should appear, derived from the Skill's specification-valid name. It may be unknown while an Unresolved Source-root Skill's name is unavailable.
_Avoid_: Output alias, Source path

**Expected Entry Collision**:
Two or more Enablements in one Skill Directory whose resolved Skill names claim the same Expected Entry. No occupant can satisfy the colliding Enablements unambiguously.
_Avoid_: Duplicate Materialization, compatibility overlap

**Drift**:
A verifiable difference between an Enablement's desired Materialization and its Observed State, including an expected Materialization that is absent.
_Avoid_: Unresolved Enablement, conflict

**Unverifiable Materialization**:
A present candidate Materialization whose conformity cannot be determined because its Skill cannot be resolved or its filesystem state cannot be inspected completely.
_Avoid_: Drift, missing Materialization

**Reconciliation**:
An attempt to make the observed Materializations in a Target Repository conform to its valid Repository Configuration. Independent changes may succeed or fail without changing the declared desired state.
_Avoid_: Configuration migration, Source update

**Safe Change**:
A planned reconciliation change whose verified preconditions permit it to apply without risking user-controlled or otherwise unrecoverable content.
_Avoid_: Warning, In Sync

**Guarded Change**:
A planned reconciliation change that may replace or remove recoverable content and therefore requires explicit TUI confirmation or non-interactive force authorization.
_Avoid_: Conflict, unsafe change

**Blocked Change**:
A planned reconciliation change whose validity, containment, capability, or recoverability requirements are not satisfied. Confirmation and force authorization cannot permit it.
_Avoid_: Guarded Change, failed change

**Reconciliation Conflict**:
A condition that prevents a planned reconciliation change from being Safe. Explicit authorization may make the change Guarded, while unresolved uncertainty or violated invariants leave it Blocked.
_Avoid_: Drift, warning, failure

**Duplicate Materialization**:
Multiple entries within one Skill Directory that can be associated with the same Skill. Repetition across distinct Skill Directories is not duplication.
_Avoid_: Compatibility overlap

**Unmanaged Entry**:
An immediate child of a Skill Directory that is neither a reserved Skillator control entry nor at the expected path of an Enablement. Association with a known Skill does not prove that Skillator created it. It is repository-owned content: Skillator preserves it and allow-lists it in the control file rather than planning removal.
_Avoid_: Disabled Skill, previously managed entry

**Recovery Artifact**:
A reserved staging or backup entry retained when Materialization replacement does not complete cleanly. It preserves recoverable content and is diagnosed separately from an Unmanaged Entry.
_Avoid_: Unmanaged Entry, Duplicate Materialization

**Recovery Required**:
A reconciliation condition in which Recovery Artifacts cannot be resolved without a person choosing which recoverable content to preserve. Generic confirmation and force authorization do not resolve it.
_Avoid_: Unmanaged Entry, Guarded Change

**Materialization**:
The filesystem representation of an Enablement: either Linked or Copied.
_Avoid_: Installation

**Linked**:
A Materialization represented by a symbolic link from a Skill Directory to the canonical absolute path of the Skill in its Source. This is the default Materialization.

**Copied**:
A Materialization represented by a self-contained snapshot of the Skill's complete content tree, excluding repository-control metadata. Internal symbolic links retain their relative text and must remain within that tree; Source changes require explicit synchronization.

**Equivalent Copy**:
A Copied Materialization whose relative names, entry kinds, file bytes, symbolic-link text, and executable state match its resolved Source Skill. Incidental filesystem metadata such as timestamps and ownership does not affect equivalence.
_Avoid_: Identical directory, unchanged copy

**Copy-Ineligible Skill**:
A valid Skill that cannot form a self-contained Copied Materialization because it contains an unsupported entry or an internal symbolic link that is absolute, escapes its tree, or cannot be proven to remain within it. The Skill may still be Linked.
_Avoid_: Invalid Skill, unavailable Skill

**Diverged Copy**:
A Copied Materialization whose contents differ from its currently resolved Source Skill. The difference alone does not identify whether the Source, the copy, or both changed.
_Avoid_: Locally modified copy, stale copy
