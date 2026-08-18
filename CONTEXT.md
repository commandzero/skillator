# Skillator

Skillator manages which agent skills from a Library are active in one or more Skill Directories in a Target Repository.

## Language

**Library**:
A user's logical collection of Sources and their available Skills. Membership in the Library does not make a Skill active.
_Avoid_: Source, source directory, registry

**Library Location**:
A user-configured local path that contributes Sources to the Library. Nested Git repositories form independent Sources; Skills outside a nested Git repository belong to the Library Location's local Source.
_Avoid_: Source, Library

**Source**:
A stable discovery boundary within a Library Location that contributes one or more Skills. A Source is either a Git repository or the Library Location's local region outside nested Git repositories.
_Avoid_: Library, marketplace

**Registered Source**:
A discovered Source whose Source Key and location have been accepted and persisted in Library configuration.
_Avoid_: Available Skill

**Unregistered Source**:
A Source discovered within a Library Location but not yet accepted into the Library. Its suggested Source Key may be staged for registration, but non-interactive synchronization does not register it.
_Avoid_: Unavailable Source, Invalid Skill

**Source Key**:
A portable, immutable, case-insensitive identifier that uniquely names a Registered Source within a Library and maps to its machine-local location. Keys normalize to lowercase slash-separated segments; Git Sources default to `owner/repository` and non-Git Sources to `local/name`.
_Avoid_: Repository name, remote URL, local path

**Unavailable Source**:
A registered Source whose local path is missing or unreadable. Its identity and desired Enablements remain intact, but its Skills cannot be materialized or synchronized.
_Avoid_: Removed Source, empty Source

**Skill**:
A directory supplied by a Source that contains a `SKILL.md` file.
_Avoid_: Package, plugin

**Registered Skill**:
A valid Skill explicitly selected into the Library from a Registered Source. Only Registered Skills appear in the normal Target Repository enablement workflow.
_Avoid_: Enabled Skill, discovered Skill

**Unregistered Skill**:
A valid Skill discovered in a Source but not selected into the Library. New and moved Skills begin unregistered and appear only in Source management.
_Avoid_: Invalid Skill, disabled Skill

**Unavailable Skill**:
A previously Registered Skill that cannot currently be found at its Skill Key or whose Source is unavailable. Its identity and desired Enablements remain intact.
_Avoid_: Unregistered Skill, Invalid Skill

**Skill Key**:
The stable identity of a Skill, formed from its Source Key and slash-normalized directory path relative to the Source. A Skill's frontmatter name and directory basename are display metadata, not identity.
_Avoid_: Skill name, absolute path

**Invalid Skill**:
A discovered Skill whose `SKILL.md` does not provide valid required metadata. It remains visible with diagnostics but cannot receive new Enablements.
_Avoid_: Missing Skill, unavailable Skill

**Overlapping Library Locations**:
Separately configured Library Locations whose canonical discovery boundaries overlap. Registration is rejected by default; an explicit override preserves distinct Skill identities while surfacing warnings on affected Enablements.

**Target Repository**:
A Git repository whose active Skills Skillator manages. It is selected from the current working directory or an explicit directory argument.
_Avoid_: Source, skill directory, destination

**Skill Directory**:
A repository-relative managed exposure boundary where active Skills are materialized for discovery by one or more agents, such as `.agents/skills` or `.claude/skills`. A Target Repository may configure several non-overlapping Skill Directories with different Enablements.
_Avoid_: Target, library

**Skill Directory Control File**:
The canonical, Skillator-owned `.gitignore` inside a configured Skill Directory. It excludes every other entry beneath that boundary while remaining eligible for repository tracking.
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
An immediate child of a Skill Directory that is neither a reserved Skillator control entry nor at the expected path of an Enablement. Association with a known Skill does not prove that Skillator created it.
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
