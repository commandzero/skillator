## MODIFIED Requirements

### Requirement: User Scope inheritance is explicit and read-only in Repository tabs

On a User Scope tab, desired Enablements SHALL remain editable and display ordinary `[✓] link` or `[✓] copy` state.
On a Repository tab, a Skill active only through User Scope SHALL display `[u] user` and SHALL not persist a Repository Enablement until the user explicitly stages and saves one.
User Scope itself SHALL remain read-only from Repository tabs.

Pressing `m` on an inherited row whose Skill is valid, Registered, and available through the Library SHALL stage a Linked Repository Enablement in the selected Skill Directory using the same Source Key and Skill path.
The row SHALL display `[✓] link` and a pending Enable action.
Save SHALL use ordinary repository link resolution and reconciliation, without changing User Scope configuration or materializations.
If the Skill cannot resolve to a valid, Registered, available Library Skill, `m` SHALL leave the row unchanged and explain why the link cannot be staged.

When the same Skill also has an explicit Repository Enablement, the explicit `[✓] link` or `[✓] copy` state SHALL remain visible with an `also active in User Scope` warning.
Ordinary repository mode changes SHALL remain available on that explicit Enablement.
Space on an explicit Enablement SHALL stage removal of only the Repository Enablement and restore `[u] user` when User Scope still enables the Skill.
Removing a saved override SHALL retain a pending Disable action until save; canceling a newly staged override SHALL clear its pending Enable action.
Space on an inherited-only row SHALL leave it unchanged and direct the user to its User tab for User Scope edits or to `m` for a repository link.
All edits SHALL remain staged until save and SHALL obey existing reconciliation safeguards.

#### Scenario: Inherited User Skill

- **WHEN** a Skill is enabled in User Scope but has no Enablement in the selected Repository Skill Directory
- **THEN** its Repository row displays `[u] user` without a pending action or saved Repository Enablement

#### Scenario: Explicit and inherited Skill

- **WHEN** the same Skill is enabled in User Scope and explicitly enabled in the selected Repository Skill Directory
- **THEN** its Repository row displays the explicit Repository mode and warns that the Skill is also active in User Scope

#### Scenario: Stage and save a repository link

- **WHEN** the user presses `m` on an available inherited Skill and saves
- **THEN** Skillator saves a Linked Enablement only in the selected Repository Skill Directory and links the Library Skill there
- **AND** reloading shows `[✓] link` with the User Scope warning and leaves User Scope configuration and materializations unchanged

#### Scenario: Discard a staged override

- **WHEN** the user stages a link from an inherited row and discards the edit
- **THEN** the row returns to `[u] user` and no configuration or materialization changes occur

#### Scenario: Remove an override

- **WHEN** the user presses Space on a saved repository override while User Scope still enables the Skill
- **THEN** the row displays `[u] user` with a pending Disable action
- **AND** saving removes only the Repository Enablement and its managed materialization under existing removal safeguards

#### Scenario: Cancel a new override

- **WHEN** the user presses `m` on an inherited row and then presses Space before saving
- **THEN** the row returns to `[u] user` with no pending action and saving performs no work for that Skill

#### Scenario: Unavailable inherited Skill

- **WHEN** the user presses `m` on an inherited Skill that cannot resolve to a valid, Registered, available Library Skill
- **THEN** Skillator explains the resolution problem and stages no Repository Enablement

#### Scenario: Occupied repository destination

- **WHEN** saving an override encounters unmanaged content at the expected repository entry
- **THEN** existing reconciliation rules require explicit confirmation before replacing recoverable conflicting content and preserve it if confirmation is declined
- **AND** Blocked conflicts remain blocked even with confirmation, and User Scope stays unchanged
