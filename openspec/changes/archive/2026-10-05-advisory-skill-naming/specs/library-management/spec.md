## MODIFIED Requirements

### Requirement: Skill inventory is live
A Skill SHALL be identified by its Source Key plus slash-normalized directory path relative to the Source. Every discovered valid Skill SHALL appear as a choice for new Enablements on the next Library Snapshot. Invalid Skills SHALL remain visible with diagnostics but MUST NOT receive new Enablements. Naming-style deviations and divergence between a safe frontmatter name and its source directory SHALL be advisory warnings, not invalidity; warnings MUST NOT prevent enablement, acquisition, materialization or Library synchronization.

#### Scenario: Newly discovered Skill
- **WHEN** a user adds a valid Skill directory beneath a configured Library Location
- **THEN** the Skill appears in the Library and Target inventories on the next discovery pass without a Library Configuration edit

#### Scenario: Invalid Skill
- **WHEN** a discovered directory has missing, malformed or unsafe required `SKILL.md` metadata
- **THEN** Skillator displays its validation error but prevents a new Enablement

#### Scenario: Divergent skill name
- **WHEN** a usable skill named `cloud-onboarding` lives in an `onboarding` directory
- **THEN** Skillator reports an advisory naming warning and permits ordinary enablement, acquisition, materialization and Library synchronization without rewriting the skill document

#### Scenario: Human-readable skill name
- **WHEN** a usable skill uses a safe frontmatter name such as `Make Bot UI`
- **THEN** naming-style and directory-name inconsistencies are warnings rather than blockers

#### Scenario: Unsafe metadata name
- **WHEN** a frontmatter name is empty, a dot or parent component, contains a path separator or a control character
- **THEN** the skill remains invalid and the name cannot authorize an acquisition or materialization path
