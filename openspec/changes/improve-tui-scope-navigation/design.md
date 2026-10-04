# Design

## Context

See [proposal.md](proposal.md) for motivation and the deltas under [specs/tui-workflows](specs/tui-workflows/spec.md) and [specs/library-hosts](specs/library-hosts/spec.md) for behavior.

The current `src/tui.rs` separates Library/Target workspaces, then flattens User and Repository directories into parallel tab arrays. Existing directory configurations already support multiple User directories. Current rendering uses purple for all Target views and blue for Library; bone, blue, and purple palette constants already exist. The new hierarchy is a presentation/session cutover, not a new desired-state file format.

PR32 supplies a strict version 1 host map at `~/.skillator/config.yaml`, with alias keys and `destination` values. Its remote modules are not present in this worktree. Its synchronization is bidirectional library-content synchronization, not User/Repo state replication. Reference: [PR32](https://github.com/commandzero/skillator/pull/32), specifically its final `src/remote/config.rs` and `docs/library-rsync.md`.

## Goals / Non-Goals

**Goals:** Model scope, sub-tab, and write destination explicitly; reuse existing workflows and safety boundaries; keep SSH setup cancellable and diagnostics distinct from hostname data.

**Non-Goals:** New synchronization direction or conflict policy, remote User/Repo management, direct remote Library editing, an SSH credential manager, automatic host trust, software installation, or new TUI rsync controls.

## Decisions

### 1. Separate navigation identity from configuration ownership

Introduce an explicit three-value scope selection and per-scope selected sub-tab identity. Library sub-tabs use `Local` plus host aliases; User and Repo use directory keys. Store browsing state by `(scope, sub-tab identity)`, rather than trusting indexes after additions. Keep existing Library, User, and Target workflow sessions as owners of staged data; do not copy rows or enablements between them.

Tab/Shift+Tab cycles only the current scope's sub-tabs; Ctrl+Left/Right cycles scopes. Ctrl+L selects Library or restores the last active directory scope. Overlay handlers take precedence. Preserve `t` for changing the repository and all non-navigation mode semantics. Dirty transitions use existing save/discard/return mechanics; switching directories within one configuration keeps staged edits. Dirty local-Library-to-follower transitions additionally account for pending host configuration.

A flat strip with scope prefixes was rejected: it repeats the current discoverability problem and cannot distinguish top-level selection from directory/host selection.

### 2. Reserve two navigation rows and one scope status row

Illustrative Repo layout:

```text
 Library   User   [Repo]                                  Skillator
 [.agents]   .claude
 ┌────────────────────────────────────────────────────────────────┐
 │                    existing skill table                        │
 └────────────────── s save · Ctrl+S save & exit · Ctrl+T new tab ┘
 Repo: ~/project · .agents/skills
```

The first line has independently sized left tabs and right title; use terminal display-cell width, not string byte length. At narrow widths remove padding first and prioritize scope labels over the title. Sub-tabs scroll horizontally to retain the selected identity. The last terminal row is scope/path status; keep the current action legend in the table's bottom border, without an extra separator rule. Inspector and table height account for the reserved rows; preserve existing small-terminal handling.

Status shows the Library configuration anchor because Library has multiple Locations, not one inventory root. Full Location paths remain in its inspector. Remote anchors use alias-qualified home-relative paths, never the initiating user's absolute home path. Repo shows repository root and directory; User shows its resolved directory. On errors or empty scopes keep the scope label and known anchor, with an unavailable indicator instead of a fabricated path.

Select border/accent from scope using existing indexed constants: Library BONE 230, User BLUE 33, Repo PURPLE 99. Retain blue modal borders and existing semantic warning/error/selection colors. Color does not replace active-tab markup or labels.

### 3. One searchable agent-directory chooser

Reuse Generic/Codex and Claude preset definitions as the initial common list; show `.agents/skills` and `.claude/skills` with short agent labels. Do not introduce a separate unverified agent-path registry. The input filters label and path case-insensitively as text changes; Up/Down selects and scrolls, Enter accepts, Esc cancels. Keep selection stable by candidate identity; reset to the first eligible match when that identity disappears. Already configured paths are visible but unavailable; Enter on one explains the collision.

No-match input exposes an explicit custom-path row. Custom paths use the same home-relative or repository-relative parsers and overlap validation as configured directories. Derive the directory key from the agent/path basename with deterministic numeric suffixes for key collisions; default the label to the agent-directory name and permit existing edit overlay customization. A path collision remains an error rather than becoming a suffixed duplicate. Creation modifies only the active scope's staged configuration and selects its pending directory. No filesystem creation occurs until save.

Hard-coded `.claude` input was rejected because it hides available options and makes an already configured path the default.

### 4. Extend the existing host entry, retaining connection aliases

Planned compatible shape:

```yaml
version: 1
hosts:
  build:
    destination: build
    hostname: worker-07.example.net
```

Make `hostname` optional for existing destination-only entries, strict when present, and preserve all other validation. Share its validator with probe results. Update the PR32 reader and serializer together; its current `deny_unknown_fields` would reject this new field. Older PR32 builds cannot read entries containing `hostname`; document that downgrade limitation. Do not replace `destination` or alias with the returned hostname: an SSH alias can supply User, HostName, Port, ProxyJump, and key settings that the returned name cannot reproduce.

The requested single `Follower name` field is an SSH alias as well as the registry alias. Support PR32's existing `user@host` destinations when loading existing entries, but this modal creates alias-based entries; it does not ask users to duplicate SSH configuration. Validate unique nonreserved alias and destination before probing. Use the host registry's containment, fingerprint and atomic-publication patterns, adding a narrowly scoped writer rather than a second configuration location.

Host registry staging is distinct from Library inventory staging. A save can include both plans, but report each independently and never claim atomicity across files. Inventory mutations remain disabled on follower tabs, while pending host registration remains an initiating-host configuration operation.

### 5. Probe through the existing bounded subprocess boundary

Invoke SSH directly with argument separation, not a shell. The command's operation is `ssh -T {destination} hostname`, plus PR32-equivalent batch authentication, trusted-key-only, no trust-file updates, and persistent-connection suppression. Inherit connection settings from the user's SSH config. Use a 10-second connection deadline and 15-second overall probe deadline; cancel via the existing process-group lifecycle. Run outside the input/render loop, with an in-progress state and Esc cancellation. Tag results with the pending request identity so a late result cannot stage a canceled or replaced entry.

Capture bounded stdout and stderr separately (4 KiB each); exceedance is an error rather than accepting truncated hostname data. Require zero exit status, then validate stdout as exactly one hostname line. Permit LF/CRLF termination, single-label or dotted ASCII names, and an optional final dot; enforce DNS label/name length bounds. Reject embedded whitespace, control bytes, banners, and extra lines. Do not use a warning substring heuristic or choose the final line: ambiguous stdout is failure. Sanitize displayed stderr independently; benign stderr warnings can accompany valid stdout and are displayed without becoming stored identity.

Successful verification stages a host entry and shows the alias/hostname distinction. It does not probe rsync compatibility or synchronize content as part of registration. Those failures belong to inspection or later synchronization diagnostics.

### 6. Follower views are genuine read-only inventories

Use PR32's transport and session boundary to request a read-only Library inspection, returning the inventory facts needed by the existing hierarchical table, diagnostics and path inspector. If the existing protocol cannot expose those facts without entering a mutating session, add a narrow read-only request and apply the protocol-version compatibility rules; do not scrape human CLI text or use a synchronization preview as a disguised inventory request. Selecting a tab performs only this inspection and releases remote resources on switch, error, cancel, and quit.

Follower tables retain Location/Source/Skill grouping but disable acquisition, visibility, and Location mutation actions. Cached results are labeled as stale while a new inspection runs; failures never replace them with local inventory. Missing/incompatible remote Skillator produces a diagnostic under that alias. Local, User, and Repo remain usable when a follower is unavailable. No connection runs automatically for every configured host at launch.

## Risks / Trade-offs

- SSH banners on stdout → Reject instead of guessing a hostname; guide the user to fix noninteractive shell output.
- SSH access succeeds but remote tooling is missing → Keep the verified host entry and report inspection prerequisites, rather than conflating SSH reachability with protocol readiness.
- Extra navigation/status rows reduce table height → Account for them in resize and minimum-size rendering; keep selected sub-tabs visible.
- Scope and host changes can misroute staged writes → Configuration-owned sessions, stable sub-tab identity, explicit dirty-transition guard, and isolation scenarios.
- Host metadata changes strict parsing → Update all host consumers, preserve destination-only files, and disclose older-build downgrade limitation.
- Remote read-only inventory may need a protocol extension → Integrate PR32 first and negotiate compatibility; incompatible followers remain diagnostic-only.

## Migration Plan

1. Integrate PR32's final host/protocol contracts into the implementation baseline; confirm the remote read-only request surface.
2. Add optional verified hostname metadata and safe host-registry persistence; existing Library/User/Repo files need no migration.
3. Cut over every renderer, navigation action, first-run/help message, and tab creation caller to the scope/sub-tab hierarchy. Remove obsolete flat-strip assumptions and hard-coded `.claude` default; keep Ctrl+L as the specified direct shortcut.
4. Exercise isolated HOME/repository sessions and SSH fixtures, including differing aliases/hostnames, warning/error output, dirty scope changes, and reload persistence. Update user documentation/changelog and complete OpenSpec synchronization/archive/review under repository policy when implementation is finished.
5. For rollback to an older PR32 build, back up host configuration and explicitly remove only added `hostname` metadata before downgrade; retain aliases, destinations and all skill state. Never silently rewrite configuration on load.
