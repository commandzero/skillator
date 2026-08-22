## Context

See `proposal.md` for the motivation. The current code treats `.agents/skillator.yaml` as a tracked repository file and builds a parent `.gitignore` that ignores everything before allow-listing unmanaged entries. That makes a local enablement choice visible to every clone, and it makes a repository-owned skill look like an exception.

`Target`, `GitRepository`, configuration codecs, observation, and reconciliation already provide useful boundaries. The change needs one new worktree-discovery boundary and one shared way to plan local Target state. Worktree sync must use the same configuration validation and reconciliation path as the TUI and ordinary `sync`.

## Goals / Non-Goals

**Goals:**

- Keep each checkout's Target configuration and generated controls local and ignored.
- Preserve normal Git behavior for repository-owned skills.
- Let a linked worktree intentionally project the primary worktree's local Target configuration.
- Make one reconciliation plan responsible for Target materializations, regardless of whether the TUI, ordinary sync, or worktree sync starts it.

**Non-Goals:**

- Do not share Library configuration, Library locations, or Sources across machines.
- Do not synchronize worktree changes back into the primary worktree.
- Do not infer ownership from a skill's name or filesystem history.
- Do not automatically remove an already tracked local configuration from the Git index.
- Do not add a background watcher or Git hook.

## Decisions

### Clone-local files have two levels of ignore rules

The repository root `.gitignore` carries two stable, shared rules:

```gitignore
/.agents/skillator.yaml
/.agents/.gitignore
```

Those rules are the repository's declaration that Skillator state is local. The Target save flow adds missing exact rules without rewriting unrelated root ignore content. It treats a changed root `.gitignore` as ordinary repository work and reports any required Git staging. It never changes the index itself.

`.agents/.gitignore` remains local and generated. It lists only paths that Skillator owns in that checkout, such as `skills/release-checklist` and `skills/.skillator-*`. It has no `*` rule. Git therefore continues to see repository-owned entries under `.agents/skills`.

Using only `.git/info/exclude` would keep all changes private, but it hides the repository's local-state convention. Tracking a fixed `.agents/.gitignore` would not work because each clone enables different skills.

### Control-file composition is grouped by parent directory

Observation derives one control group for every parent directory shared by configured Skill Directories. The group computes its content from the configured expected entries and reserved recovery artifact patterns. It does not inspect unmanaged entries to decide what to ignore.

This replaces the current per-directory catch-all file. Grouping prevents two configured directories under `.agents` from writing competing versions of `.agents/.gitignore`.

Unmanaged entries remain inspector diagnostics and duplicate candidates, but they do not produce drift or `RemoveUnmanaged` plan items. Explicitly disabling a configured Enablement still produces a removal operation through the existing original-versus-staged transition plan.

### Local configuration writes use one Target-state planner

Introduce a private Target-state planner used by TUI save and worktree sync. It accepts a Target, a validated desired configuration, the expected configuration fingerprint, and the desired root ignore rules. It prepares:

1. a root `.gitignore` append or creation when rules are missing;
2. a conditional local configuration publication;
3. grouped local control-file publications;
4. the existing reconciliation plan.

Each publication keeps the existing sibling staging, fingerprint, containment, and rollback safeguards. A successful local configuration write remains authoritative if a later materialization fails. The planner reports the final observation from the destination worktree.

A tracked `.agents/skillator.yaml` is a hard protection boundary. The planner can read it but will not overwrite it. It reports the exact `git rm --cached -- .agents/skillator.yaml` action needed before the user can save local state.

### Worktree discovery is a narrow Git module

Add a `GitRepository` operation that returns the primary worktree and linked registered worktrees from Git's worktree metadata. It validates canonical paths and distinguishes the primary worktree from linked worktrees. The rest of the application receives a small `WorktreePair` value containing only the primary and current roots.

The command accepts only a current linked worktree. Running it from the primary worktree, a normal checkout, or outside Git is invalid input. Directory names and sibling layout are never used to infer membership.

### Worktree sync is a projection, not a merge

`skillator worktree sync` reads and validates the primary worktree's local configuration, then uses those bytes as the desired state for the current linked worktree. It does not merge configurations.

The command locks the primary and destination Targets in stable canonical-path order. It captures the primary configuration fingerprint, rechecks it before destination publication, and never writes to the primary worktree. It first verifies that the destination root has the shared ignore rules. Then it conditionally writes the destination local configuration, regenerates its local control files, and invokes the normal destination reconciliation path.

An absent destination configuration is Safe. An equal configuration is a no-op. A differing untracked configuration is Guarded and needs `--force`. A tracked, staged, unmerged, unreadable, or changed-after-planning destination configuration is Blocked. `--check` prepares and reports the same work without writes.

Using a separate copy-and-link implementation would make worktree sync drift from Target save behavior. Reusing the Target-state planner keeps safety and reporting consistent.

## Risks / Trade-offs

- [A repository has no root ignore rules] → Target save can add the two exact rules while preserving other content. Worktree sync refuses to expose clone-local files until the rules are effective.
- [A developer still tracks the old local configuration] → Preserve the file and report the explicit index-only removal command. Do not stage a deletion automatically.
- [A worktree has intentional local configuration changes] → Default worktree sync reports a Guarded replacement. The caller must use `--force` to replace it.
- [Two commands run against related worktrees] → Acquire both locks in canonical order and recheck source and destination fingerprints before publication.
- [A branch does not yet contain the root ignore rules] → Worktree sync reports the destination precondition rather than modifying the branch's root ignore file behind the caller's back.

## Migration Plan

1. Ship root ignore-rule planning and selective local control files.
2. Detect tracked `.agents/skillator.yaml` without rewriting or removing it.
3. Tell the user to run `git rm --cached -- .agents/skillator.yaml`, then commit the deletion and root `.gitignore` rules.
4. After that commit reaches the relevant branches, each checkout can save its own ignored local configuration and use worktree sync.

Rollback consists of restoring the previous tracked configuration from Git and removing the root ignore rules in a normal repository commit. Skillator does not delete local configuration during rollback.
