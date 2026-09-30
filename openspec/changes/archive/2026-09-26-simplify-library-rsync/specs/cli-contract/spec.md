## MODIFIED Requirements

### Requirement: Library rsync exposes explicit policies

`skillator library rsync` SHALL accept `--hosts <alias,...>`, `--conflict <local|remote|ask>`, `--missing <copy|remove|ignore>`, `--check`, `--format <text|json|yaml>`, and `--color <auto|always|never>`. Defaults SHALL be all configured hosts, `ask`, `copy`, text, and automatic color. Explicit color SHALL conflict with machine formats. Unsupported policy values, empty host selectors, and unknown aliases SHALL return `2`. This command SHALL NOT expose `--force`; its policy flags authorize only the defined synchronization actions. It SHALL run without a terminal and only ask for skill-content conflicts in interactive text application mode. The alias `local` SHALL be reserved for the initiating participant. Help SHALL describe library-content synchronization; user selections and materializations remain host-local.

#### Scenario: Default command

- **WHEN** the user runs `skillator library rsync` without flags
- **THEN** it selects every configured host and uses interactive conflict resolution when a terminal is available, with missing-file copying
