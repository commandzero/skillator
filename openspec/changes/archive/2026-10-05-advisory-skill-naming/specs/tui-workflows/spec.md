## MODIFIED Requirements

### Requirement: The Library inspector exposes contextual diagnostics
Selecting a Location, Source, or Skill SHALL expose relevant details in a contextual inspector, including original and resolved paths, Source kind and key, Git facts, validation errors, unavailability, and overlap warnings. Invalid Skills SHALL remain visible rather than disappear. Skill details SHALL distinguish advisory naming warnings from blocking errors in Local Library, Targets and follower replicas; naming-only warnings MUST NOT produce inventory error markers or diagnostic rows.

#### Scenario: Unavailable Location
- **WHEN** a configured Location is missing locally
- **THEN** its row remains visible with an unavailable diagnostic in the inspector

#### Scenario: View an advisory naming mismatch
- **WHEN** the user views a skill whose safe frontmatter name differs from its source directory or has a naming-style inconsistency
- **THEN** details show labeled advisory warnings alongside the unmodified skill document, without an inventory error marker or warning-generated error row
- **AND** ordinary actions remain available for the usable skill

#### Scenario: Inspect a follower naming warning
- **WHEN** a follower replica contains a usable skill with divergent naming
- **THEN** its skill details show the advisory warning separately from blocking errors and preserve the read-only replica behavior

#### Scenario: View a real metadata error
- **WHEN** a skill document is malformed or required metadata is missing or unsafe
- **THEN** its details identify the blocking error and it is not treated as a warning-only usable skill
