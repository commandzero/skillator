## ADDED Requirements

### Requirement: CLI desired-state changes and reconciliation use one prepared operation
A CLI command that changes an Enablement SHALL prepare configuration, control-file, and Materialization changes from one observed snapshot. Before the first write it SHALL validate every precondition and verify that loaded configuration has not changed. Application SHALL either publish the requested configuration with every authorized change, roll back recoverable work on failure, or report Recovery Required without claiming convergence.

#### Scenario: Configuration changes during preparation
- **WHEN** a configuration fingerprint differs immediately before a CLI mutation begins writing
- **THEN** Skillator reports stale state and performs none of the prepared writes

#### Scenario: Materialization fails after staging
- **WHEN** a staged Materialization cannot be published during a CLI mutation
- **THEN** Skillator rolls back the batch where recovery is verifiable and reports the failed operation

#### Scenario: Independent blocked work
- **WHEN** the requested Enablement change includes a Blocked Materialization change
- **THEN** Skillator does not publish desired state that falsely claims the blocked result was achieved

