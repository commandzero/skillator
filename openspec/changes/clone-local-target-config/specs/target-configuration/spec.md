## MODIFIED Requirements

### Requirement: Repository Configuration has one version 1 schema
Each Git worktree SHALL keep its clone-local Target desired state in `.agents/skillator.yaml`. The document SHALL use numeric `version: 1` followed by zero or more top-level Skill Directory mappings keyed by Skill Directory Key. Each directory mapping SHALL contain required `path`, optional `label`, and `skills`. `skills` SHALL map the materialized Skill name to an entry with required `source`, optional Source-relative `path`, and optional `type` equal to `linked` or `copied`. Omitted `path` defaults to the Skill mapping key; omitted `type` defaults to `linked`. Presence of a Skill entry SHALL mean enabled; no `enabled` field exists in version 1.

The configuration is local to the checkout and MUST NOT be tracked by Git. A repository that uses Skillator SHALL have root `.gitignore` rules for `/.agents/skillator.yaml` and `/.agents/.gitignore`. Skillator MAY add the missing exact rules without changing unrelated root ignore content, but MUST NOT change the Git index. If the local configuration is already tracked, Skillator SHALL preserve its contents, block configuration writes, and report the exact `git rm --cached -- .agents/skillator.yaml` remediation.

#### Scenario: Minimal empty configuration
- **WHEN** a version 1 document contains no Skill Directory mappings
- **THEN** Skillator treats the document as valid intentional configuration and does not restage defaults

#### Scenario: Default Linked materialization
- **WHEN** the TUI creates a new Enablement without a prior mode choice
- **THEN** it stages `linked` and omits `type` from canonical saved YAML

#### Scenario: Clone-local configuration is ignored
- **WHEN** a repository has the required root ignore rules and a worktree saves `.agents/skillator.yaml`
- **THEN** Git leaves the local configuration untracked and available only in that checkout

#### Scenario: Legacy tracked configuration
- **WHEN** `.agents/skillator.yaml` is already tracked
- **THEN** Skillator preserves the file, performs no configuration write, and reports the required index-only removal command
