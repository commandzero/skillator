## Context
Rsync already delivers <SourceKey>/_skills/<SkillPath>. Generic discovery ignored this identity boundary, while TUI first-run discovery and save planning used different missing-configuration policies.

## Decisions
Recognize replicas by the existing ownership signature, derive Source Keys from validated container paths, and discover skills relative to _skills. Do not flatten replica sources, follow linked containers or mask real collisions. Omit empty enclosing local sources only when a replica supplies inventory.
Use LibraryWorkflow::load for effective discovery so first-run default locations agree across browsing and materialization. Keep saved-only configuration loading for explicit Library registration mutations, updates and leader exports. Existing configuration, including locations: [], wins.

## Risks
Old flattened selectors are not rewritten automatically; document canonical re-selection. Invalid markers and source keys remain diagnostic. Preview must not create Library configuration or ~/.agents. Source exclusions remain relative to the registered Location.

## Verification
403 tests passed, one ignored; strict Clippy, formatting, OKF docs and 12 main specs passed. Real rsync push/pull used a local SSH routing adapter. Actual terminal interactions proved independent repository collapse/expand and first User cpr-loop save with neither Library configuration nor ~/.agents; saved selection stayed resolved after restart. Real Repo CLI link and sync also used implicit discovery. Actual account inventory resolved cpr-loop; its link preview was blocked by another running Skillator process, which was preserved.
