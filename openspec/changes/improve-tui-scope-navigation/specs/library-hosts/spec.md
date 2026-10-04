# Spec Delta

## Purpose

Defines host-local follower registration, reliable SSH hostname discovery, and remote Library inspection without conflating connection aliases with remote machine identities.

## ADDED Requirements

### Requirement: Followers share the synchronization host registry
Follower configurations SHALL use the PR32 host registry at `~/.skillator/config.yaml`. Each entry SHALL retain its unique alias and SSH `destination`, with optional `hostname` metadata containing the verified remote hostname. Existing destination-only entries SHALL load unchanged. Unknown fields and malformed values SHALL remain errors; `local` SHALL remain reserved.

#### Scenario: Existing PR32 configuration
- **WHEN** the registry contains destination-only host entries
- **THEN** Library shows those follower aliases as sub-tabs without requiring a migration or automatically probing them

#### Scenario: SSH alias differs from reported hostname
- **WHEN** the user registers `build` and the probe returns `worker-07.example.net`
- **THEN** the entry retains alias `build` and destination `build`, stores hostname `worker-07.example.net`, and subsequent SSH and rsync use `build`

#### Scenario: Duplicate or reserved follower name
- **WHEN** a new follower name duplicates a configured alias or is `local`
- **THEN** setup explains the collision without overwriting an entry or starting SSH

### Requirement: Library tab creation verifies follower access
`Ctrl+T` in Library SHALL ask for `Follower name` with a hint that credentials and connection settings must exist in the user's SSH config. The entered name SHALL identify an SSH alias and registry alias. Setup SHALL validate it before a bounded, cancellable noninteractive `ssh -T {destination} hostname` probe, using existing SSH trust and credentials without modifying them. Successful verification SHALL stage the follower for explicit save.

#### Scenario: Follower setup hint
- **WHEN** the user opens follower setup
- **THEN** it explains that the name resolves through `~/.ssh/config`, credentials must already work, and host trust must be established outside Skillator

#### Scenario: Successful verification
- **WHEN** a valid follower name passes SSH verification
- **THEN** setup displays the entered destination and reported hostname, adds a pending follower tab, and writes nothing until save

#### Scenario: Untrusted host or authentication failure
- **WHEN** SSH requires a password, host-trust confirmation, or fails authentication
- **THEN** setup returns an actionable diagnostic without an interactive SSH prompt, saved entry, or trust-file change

#### Scenario: Timeout or cancellation
- **WHEN** verification times out or the user cancels it
- **THEN** the process is stopped, no follower is staged, and the TUI returns to editing without freezing

#### Scenario: Unsafe destination input
- **WHEN** the input contains option syntax, whitespace, control characters, or shell expressions
- **THEN** setup rejects it before invoking any subprocess

### Requirement: Hostname discovery separates data from diagnostics
A probe SHALL succeed only with a zero exit status and stdout containing exactly one nonempty valid hostname line, allowing one trailing LF or CRLF. Hostname labels SHALL contain ASCII letters, digits, or hyphens separated by dots, with alphanumeric label ends; one final dot SHALL be allowed. Stderr SHALL remain diagnostic-only. Control sequences, extra stdout lines, malformed names, and empty output SHALL prevent registration.

#### Scenario: Warning on stderr with valid hostname
- **WHEN** SSH exits zero, stdout is `worker-07\n`, and stderr contains a warning
- **THEN** setup stages hostname `worker-07`, presents the sanitized warning separately, and never stores the warning as hostname

#### Scenario: Nonzero exit with plausible stdout
- **WHEN** SSH exits nonzero even though stdout contains `worker-07\n`
- **THEN** setup reports failure and stages no follower

#### Scenario: Banner or warning contaminates stdout
- **WHEN** stdout contains a banner followed by a hostname, multiple hostname-like lines, or only whitespace
- **THEN** setup rejects ambiguous output rather than selecting its first or last line

#### Scenario: Terminal escape output
- **WHEN** either stream contains terminal escape sequences
- **THEN** hostname validation rejects contaminated stdout and displayed diagnostics cannot execute terminal controls

### Requirement: Follower registration preserves local state
Saving a staged follower SHALL atomically update only the initiating user's host registry using stale-write protection and existing containment safeguards. It SHALL preserve other entries and Library/User/Repo state. Missing configuration SHALL be created with version 1 and the staged entry; invalid configuration SHALL remain diagnostic-only. Discard or unsaved quit SHALL preserve bytes. Registration SHALL NOT install dependencies or run rsync.

#### Scenario: Save and restart
- **WHEN** a verified follower is saved and Skillator restarts
- **THEN** its alias, SSH destination, and reported hostname are restored as a Library sub-tab without changing skill content or user enablements

#### Scenario: Concurrent host configuration change
- **WHEN** another process changes the registry after follower setup began
- **THEN** save refuses a stale overwrite and retains the pending follower for user resolution

#### Scenario: Invalid existing registry
- **WHEN** the registry is malformed or unsupported
- **THEN** Library displays its diagnostic and follower creation cannot overwrite or repair it implicitly

#### Scenario: Discard verified follower
- **WHEN** the user discards a verified pending follower
- **THEN** the pending tab disappears and configuration and skill files remain unchanged

### Requirement: Follower tabs show host-specific Library state
Library SHALL show the initiating host as `Local` first and configured followers by alias. Selecting a follower SHALL inspect that host's Library inventory read-only and identify its alias, known hostname, and host-qualified Library configuration path. It SHALL NOT substitute local inventory, edit remote enablements, or initiate rsync. Unavailable or incompatible followers SHALL retain their tabs with actionable diagnostics.

#### Scenario: Inspect follower inventory
- **WHEN** a follower is selected and remote inspection succeeds
- **THEN** its Locations, Sources, Skills, and diagnostics appear with a read-only indicator and its host-qualified path

#### Scenario: Offline follower
- **WHEN** remote inspection fails
- **THEN** its selected tab identifies the failed host and reason without displaying local inventory as remote content

#### Scenario: Follower lacks compatible Skillator
- **WHEN** SSH hostname verification succeeds but Library inspection finds missing or incompatible remote dependencies
- **THEN** registration remains valid and the follower tab explains the inspection failure without installing software

#### Scenario: Attempt remote mutation
- **WHEN** the user invokes a Library inventory mutation or save of inventory changes on a follower tab
- **THEN** the TUI explains that the remote inventory is read-only and changes no local or remote inventory
