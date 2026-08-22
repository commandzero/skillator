## MODIFIED Requirements

### Requirement: Repository controls live beside, not inside, Skill Directories
Every configured Repository Skill Directory SHALL use a generated UTF-8 `.gitignore` in the directory's parent, such as `.agents/.gitignore` for `.agents/skills`, ending in a newline. The parent control file is clone-local and MUST be ignored by the repository root `.gitignore`; Skillator MUST NOT stage it.

The generated content SHALL ignore only Skillator-managed materializations and Skillator recovery artifacts for that Skill Directory. It MUST NOT use a catch-all rule and MUST NOT ignore an unlisted repository-owned skill. Skillator SHALL regenerate the control file from current local Target desired state. User Scope Skill Directories SHALL have no Skillator-managed `.gitignore` and no Git tracking requirement.

#### Scenario: Control file created
- **WHEN** a repository has the required root ignore rule and the local control file is absent
- **THEN** Skillator creates it as a Safe Change and verifies that configured materializations are effectively Git-ignored

#### Scenario: Repository-owned skill remains trackable
- **WHEN** `.agents/skills` contains a skill not declared by the local Target configuration
- **THEN** Skillator leaves it unignored, preserves it without a reconciliation action, and allows Git to track it

#### Scenario: Managed skill is ignored locally
- **WHEN** local Target configuration enables `release-checklist` in `.agents/skills`
- **THEN** the generated `.agents/.gitignore` ignores `skills/release-checklist` without ignoring other entries in `skills`

#### Scenario: Control file is local
- **WHEN** the generated local control file exists
- **THEN** the repository root ignore rule hides it from Git status and Skillator never asks the user to stage it

#### Scenario: Tracked repository-owned entry
- **WHEN** an unlisted repository-owned Skill is Git-tracked
- **THEN** Skillator preserves it and performs no reconciliation action for that entry

#### Scenario: Tracked expected entry
- **WHEN** a configured expected entry is already Git-tracked
- **THEN** Skillator blocks its replacement even under force and leaves unrelated Git worktree changes untouched
