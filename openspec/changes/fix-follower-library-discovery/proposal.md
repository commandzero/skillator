## Why
Follower replicas lost repository source identities during local discovery. First User activation also displayed skills from the implicit default Library but planned saves against an empty inventory when library.yaml was absent.

## What Changes
- Recognize owned replica source boundaries and preserve leader selectors without Git metadata.
- Share implicit default discovery across inventory, User/Repo saves, selector validation and materialization checks without persisting Library configuration.
- Preserve exclusions, collision diagnostics, explicit configuration precedence and destination safety.

## Capabilities
### Modified Capabilities
- `library-management`: owned replica source discovery and consistent implicit Library discovery.
- `cli-contract`: missing Library configuration uses read-only implicit default discovery.

## Impact
Library discovery and application loading, behavior regressions, follower documentation and changelog. No replica format change, remote configuration writes, automatic activation or user-selection migration.
