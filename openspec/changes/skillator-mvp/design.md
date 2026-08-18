## Context

The repository currently contains only a minimal Rust binary and the canonical domain glossary in `CONTEXT.md`. This change introduces the first functional version of Skillator, so there is no legacy runtime architecture or configuration to preserve. The behavior contracts live in the five capability specs in this change.

The design must keep filesystem mutation understandable and testable across macOS, Linux, and WSL while serving both a synchronous TUI and a non-interactive command. The same validated configuration, discovery, observation, planning, and execution behavior must be shared by both interfaces.

## Goals / Non-Goals

**Goals:**

- Keep domain decisions and safety policy independent from terminal rendering and command-line formatting.
- Make observation immutable, reconciliation planning pure, and mutation explicitly authorized.
- Preserve a complete trustworthy result when independent filesystem operations partially succeed.
- Keep configuration codecs strict and prevent raw YAML or machine-local paths from leaking across module boundaries.
- Test behavior at stable module and workflow seams with real temporary filesystems and Git repositories.

**Non-Goals:**

- General-purpose plugin, dependency-injection, event, filesystem-abstraction, or repository-trait frameworks.
- Async execution, background services, watchers, shared caches, or automatic worktree hooks.
- Transaction-wide atomicity, power-loss durability, or cross-platform support beyond Unix APIs used by macOS, Linux, and WSL.
- Configuration migrations or compatibility policy beyond rejecting and preserving unsupported versions.

## Decisions

### Use one Cargo package with a library and thin binary

The package will expose an internal library containing application workflows and a thin `main.rs` that delegates process execution. This keeps CLI integration tests and future embedding practical without creating a multi-crate workspace before a genuine independent package boundary exists.

Alternative considered: separate domain, CLI, and TUI crates. Rejected for the MVP because it adds dependency and publication structure without improving the current single-binary deployment.

### Organize code around deep domain and workflow modules

The library will contain these top-level modules:

```text
domain
config
git
library
target
materialization   (private)
reconcile
app
cli
tui
```

The conceptual dependency direction is:

```text
domain
  ↑
config  git  materialization
  ↑       ↑        ↑
library  target
    \     /
    reconcile
       ↑
      app
     ↗   ↖
   cli   tui
```

Modules expose only the workflows and domain/result types their callers require. Implementations, codecs, inspectors, executors, and helpers remain crate-private. Lower modules never import `app`, `cli`, or `tui`; presentation modules never bypass application workflows.

Alternative considered: horizontal `models`, `services`, and `utils` modules. Rejected because they scatter ownership and make it harder to identify where validation, observation, or safety policy belongs.

### Put shared validated identities in `domain`, not every data structure

`domain` owns only values shared across real module boundaries: Source Keys, Skill Keys, Skill Directory Keys, validated repository-relative paths, Enablements, and Materialization kinds. The module that produces a snapshot or plan owns that type.

Alternative considered: a comprehensive shared model graph. Rejected because it couples Library, Target, reconciliation, and presentation evolution and encourages invalid intermediate states.

### Hide both YAML documents behind one strict configuration seam

`config` owns separate Library and Repository document codecs behind a consistent interface. It performs version dispatch, syntax and structural validation, conversion into validated domain values, deterministic serialization, and conditional atomic replacement. Raw serialization structures never escape.

Each successful load returns a validated document plus a content fingerprint. Save compares that fingerprint with the current file and refuses stale content rather than merging or overwriting external edits. Absent documents have an explicit absent fingerprint. Writes stage a sibling file and rename it into place where supported.

The implementation will use Serde-compatible JSON data types and a maintained YAML implementation proven by golden fixtures to meet the constrained YAML contracts. The deprecated `serde_yaml` crate will not be introduced. Exact crate selection remains an implementation dependency choice, not a behavioral API.

Alternative considered: letting workflows parse or modify YAML directly. Rejected because it would duplicate validation and make byte-preserving unsupported-version behavior unreliable.

### Produce immutable Library and Target snapshots

`library` consumes validated Library Configuration plus structured Git facts and returns an immutable `LibrarySnapshot` containing discovery, registration, resolution, availability, and diagnostics. It does not construct table rows.

`target` resolves a Target and returns immutable Observed State for configured Skill Directories. Observation records facts and comparison results only; it contains no reconciliation action, authorization, or presentation wording.

The private `materialization` module owns physical entry inspection, canonical-link validation, copied-tree equivalence, and staged candidate construction. It does not decide safety or publish changes.

Alternative considered: one mutable repository session object that discovers, plans, and applies. Rejected because it would blur when facts were observed and make stale-plan tests unreliable.

### Separate pure planning from guarded execution

`reconcile` owns policy and mutation. Its planner is pure: validated desired state, a Library snapshot, and Observed State produce an immutable Plan containing Safe, Guarded, Blocked, and No Change outcomes. The executor consumes that exact Plan, revalidates preconditions, and either applies its existing operations or records them Blocked; it never silently replans.

Preparing a mutation acquires the Target lock and returns a non-cloneable capability containing the lock guard and reviewed Plan. Guarded authorization is represented as a typed Safe-only or All-Guarded value, never a general Boolean. Confirmation consumes the capability; cancellation or drop releases it without writes.

Publication ordering, control-file gates, sibling staging, backup, rollback, Recovery Artifacts, failure isolation, removals-last ordering, and final observation all belong to `reconcile`. A private fault-injection seam permits deterministic recovery testing.

Alternative considered: classify and mutate each entry in one loop. Rejected because the user could confirm a plan different from the one actually applied and partial results would be difficult to explain.

### Use three command-shaped application workflows

`app` owns cross-module sequencing through:

- `LibraryWorkflow`, which loads the Library workspace and saves staged Library edits;
- `TargetWorkflow`, which loads a Target workspace, prepares staged Repository Configuration edits, and commits a confirmed save; and
- `SyncWorkflow`, which checks or reconciles current desired state and returns one semantic command report.

A Target save follows this sequence:

```text
validate staged edits
→ acquire Target lock
→ verify loaded configuration fingerprint
→ rescan Library and Target facts
→ prepare the exact save and reconciliation plan
→ obtain confirmation if required
→ conditionally write canonical Repository Configuration
→ consume prepared reconciliation
→ perform final observation
→ return SaveResult
```

The prepared save is opaque and non-cloneable. A configuration-write failure prevents all reconciliation writes. A successful valid configuration write remains authoritative if later operations are partial.

Alternative considered: have CLI and TUI orchestrate lower modules directly. Rejected because the two interfaces would drift in validation, lock ordering, and partial-apply behavior.

### Keep Git integration fact-oriented and index-read-only

`git` hides whether facts come from Git subprocesses or a library. It resolves worktree roots, identifies Source repository boundaries and remotes, and inspects exact paths for tracked, staged, unmerged, and ignored state. It returns structured facts and command failures but does not classify Drift or mutate the index.

Alternative considered: allow reconciliation to invoke Git commands directly. Rejected because it spreads path-safety and status interpretation across policy code.

### Model TUI interaction as a reducer with effects

The TUI core uses:

```text
Model + Action → Model + Effect
```

The Model owns selection, filters, collapsed groups, overlays, typed staged edits, and any pending prepared-save capability. The event loop executes Effects through `app` workflows and feeds typed results back as Actions. Widgets do not inspect files, run Git, serialize configuration, or classify safety.

Alternative considered: stateful widgets calling services. Rejected because navigation and save-confirmation behavior would require terminal-driven tests and could bypass application invariants.

### Keep CLI rendering downstream of semantic results

`cli` owns clap parsing, conversion into a presentation-neutral command, workflow dispatch, exit mapping, and text/JSON/YAML rendering. `SyncWorkflow` returns the compact semantic report model; reconciliation never creates CLI-specific output. Machine encoders serialize a complete in-memory document before writing stdout.

Alternative considered: stream changes as they occur. Rejected because a late serialization or operation failure could leave malformed machine output and prevent a trustworthy single report.

### Test through stable seams

- Configuration codecs use malformed-input matrices, canonical golden files, and unsupported-version preservation fixtures.
- Library and Target inspection use temporary filesystem trees and real temporary Git repositories.
- Reconciliation planning uses table-driven pure tests.
- Execution and recovery use temporary trees with private fault injection.
- Application workflows receive end-to-end tests through their public interfaces.
- CLI renderers use golden text, JSON, and YAML reports, with JSON/YAML logical equivalence checks.
- TUI behavior uses reducer transition tests plus a small rendered-screen and manual terminal smoke suite.
- Core acceptance journeys run on macOS, Linux, and WSL's Linux filesystem.

Tests assert observable outcomes through module interfaces rather than preserving private helper structure.

## Risks / Trade-offs

- **Filesystem behavior differs across mounts and operating systems** → Probe required capabilities, stage on the destination filesystem, and return Blocked or capability diagnostics without fallback.
- **Strict YAML support can vary between libraries** → Constrain the accepted and emitted data model, pin the selected implementation, and validate duplicate-key, multi-document, quoting, and deterministic-output fixtures before relying on it.
- **Partial application is more complex than fail-fast mutation** → Keep one immutable reviewed Plan, isolate Expected Entry operations, retain recoverable backups, and always return fresh final observation.
- **Absolute links are machine-specific** → Keep Source paths only in user-local Library configuration and recreate links from portable Repository Configuration on each machine.
- **Synchronous scans may pause the TUI on large Libraries** → Keep the MVP synchronous and surface progress where useful; do not add async state until real measurements justify it.
- **A single crate can grow broad** → Enforce module ownership and crate-private implementation seams; split crates only when independent consumers or build boundaries appear.

## Migration Plan

No schema migration exists for the MVP because this is Skillator's first version. Unsupported configuration versions remain read-only and byte-preserved; migration design is deferred until a concrete version 2 exists.
