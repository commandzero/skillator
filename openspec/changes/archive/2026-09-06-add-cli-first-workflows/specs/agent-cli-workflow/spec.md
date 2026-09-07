## Purpose

Defines a project-owned agent skill that uses Skillator's non-interactive commands to make explicit, previewed, and verified Skill state changes.

## ADDED Requirements

### Requirement: The agent skill follows a preview and verification workflow
The repository SHALL provide an agent skill for requests to inspect Library inventory, register or prune Library Locations, initialize Targets, inspect or clean the Target registry, or change Target and User Scope Enablements. The skill SHALL resolve canonical identities through machine-readable Library output, inspect current state, preview each mutation with `--check`, apply the same mutation only after the preview permits it, and verify the resulting state. It SHALL stop for human authorization before adding `--force` or resolving Recovery Required state.

#### Scenario: Agent links a repository Skill
- **WHEN** an agent receives a request to link a Library Skill into the current Target
- **THEN** it resolves the canonical selector, previews the exact command, applies it when no additional authorization is needed, and verifies convergence

#### Scenario: Agent encounters selector ambiguity
- **WHEN** Library results do not identify exactly one requested Skill
- **THEN** the agent reports the candidates and requests a choice without mutating state

#### Scenario: Agent encounters a Guarded Change
- **WHEN** the preview reports work that requires force
- **THEN** the agent presents the affected path and reason and waits for explicit authorization before using `--force`

#### Scenario: Agent prunes stale registrations
- **WHEN** an agent is asked to clean stale Library Locations or Target registrations
- **THEN** it previews the appropriate prune command, reports the paths and stale reasons, applies the same command when authorized by the request, and verifies the resulting registry

### Requirement: The agent skill composes Skillator with Git
The agent skill SHALL use the installed Git CLI to clone remote Skills repositories and create linked worktrees. After `git clone`, it SHALL register the local path with `skillator library add`. After `git worktree add`, it SHALL run `skillator sync worktree` against the new worktree. It SHALL use `library list`, `target list`, `user list`, and `targets list` to verify the Skillator-owned state instead of inferring it only from filesystem entries.

#### Scenario: Agent completes a remote Skill lifecycle
- **WHEN** an agent is asked to acquire a remote Skills repository and enable one of its Skills
- **THEN** it clones with Git, registers the clone as a Library Location, resolves the canonical Skill selector, previews and applies the requested User Scope or Target Enablement, and verifies the saved and observed state

#### Scenario: Agent creates and synchronizes a worktree
- **WHEN** an agent is asked to create a linked worktree with the primary Target state
- **THEN** it creates the worktree with Git, runs worktree synchronization, and verifies that the destination appears in `targets list`

### Requirement: The agent skill delegates command syntax to CLI help
The agent skill SHALL state decision rules and completion criteria while directing the agent to current `skillator --help` output for command details that the executable can provide. It SHALL distinguish Linked and Copied Materializations, canonical Skill selectors, managed entries, and unresolved Enablements without duplicating the full CLI reference.

#### Scenario: CLI syntax changes later
- **WHEN** the installed executable's help differs from examples remembered by the agent
- **THEN** the agent follows the installed help while retaining the skill's preview, authorization, and verification process
