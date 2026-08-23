## ADDED Requirements

### Requirement: Individual User Scope Enablements can be edited without the TUI
`skillator user link <selector>` and `skillator user copy <selector>` SHALL add or update one User Scope Enablement and reconcile the requested Materialization. `skillator user remove <selector>` SHALL remove the matching Enablement and reconcile only its managed Materialization. When User Scope Configuration is absent, the first link or copy SHALL prepare version 1 configuration with the `agents` directory at `.agents/skills`. User Scope commands SHALL apply existing home-relative containment and reconciliation rules without Git inspection or control-file changes.

#### Scenario: First User Scope link
- **WHEN** User Scope Configuration is absent and the user links a valid Skill
- **THEN** Skillator creates the default User Scope Configuration, adds the Linked Enablement, and reconciles it under `~/.agents/skills`

#### Scenario: Remove a User Scope Enablement
- **WHEN** the selected User Scope Enablement and its conforming Materialization exist
- **THEN** Skillator removes the declaration and managed Materialization without changing repository files

#### Scenario: Existing unmanaged User Scope entry
- **WHEN** the Expected Entry is occupied by content that Skillator cannot prove it manages
- **THEN** Skillator preserves the entry and blocks the mutation

