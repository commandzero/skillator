## Purpose

Defines a project-owned agent skill that uses Skillator's non-interactive commands to make explicit, previewed, and verified Skill state changes.

## ADDED Requirements

### Requirement: The agent skill follows a preview and verification workflow
The repository SHALL provide an agent skill for requests to inspect Library inventory, register Library Locations, initialize Targets, or change Target and User Scope Enablements. The skill SHALL resolve canonical identities through machine-readable Library output, inspect current state, preview each mutation with `--check`, apply the same mutation only after the preview permits it, and verify the resulting state. It SHALL stop for human authorization before adding `--force` or resolving Recovery Required state.

#### Scenario: Agent links a repository Skill
- **WHEN** an agent receives a request to link a Library Skill into the current Target
- **THEN** it resolves the canonical selector, previews the exact command, applies it when no additional authorization is needed, and verifies convergence

#### Scenario: Agent encounters selector ambiguity
- **WHEN** Library results do not identify exactly one requested Skill
- **THEN** the agent reports the candidates and requests a choice without mutating state

#### Scenario: Agent encounters a Guarded Change
- **WHEN** the preview reports work that requires force
- **THEN** the agent presents the affected path and reason and waits for explicit authorization before using `--force`

### Requirement: The agent skill delegates command syntax to CLI help
The agent skill SHALL state decision rules and completion criteria while directing the agent to current `skillator --help` output for command details that the executable can provide. It SHALL distinguish Linked and Copied Materializations, canonical Skill selectors, managed entries, and unresolved Enablements without duplicating the full CLI reference.

#### Scenario: CLI syntax changes later
- **WHEN** the installed executable's help differs from examples remembered by the agent
- **THEN** the agent follows the installed help while retaining the skill's preview, authorization, and verification process

