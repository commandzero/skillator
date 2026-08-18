use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{Terminal, backend::TestBackend};
use skillator::domain::MaterializationKind;
use skillator::tui::{
    Action, CheckState, Effect, Model, Overlay, Row, Workspace, action_for_key, reduce, render,
};

#[test]
fn source_bulk_toggle_includes_filtered_and_collapsed_children() {
    let rows = vec![
        Row::source("elastic/agent-skills", CheckState::Mixed),
        Row::skill(
            "elastic/agent-skills",
            "esql",
            "Write ESQL",
            true,
            true,
            MaterializationKind::Copied,
            "In Sync",
        ),
        Row::skill(
            "elastic/agent-skills",
            "release-manager",
            "Coordinate releases",
            false,
            true,
            MaterializationKind::Linked,
            "Missing",
        ),
    ];
    let mut model = Model::new(Workspace::Target, rows);
    reduce(&mut model, Action::Collapse);
    reduce(&mut model, Action::StartFilter);
    for character in "esql".chars() {
        reduce(&mut model, Action::Input(character));
    }
    reduce(&mut model, Action::Confirm);
    reduce(&mut model, Action::MoveUp);
    reduce(&mut model, Action::Toggle);

    let skills: Vec<_> = model.rows().iter().filter(|row| row.is_skill()).collect();
    assert!(
        skills
            .iter()
            .all(|row| row.check() == Some(CheckState::Checked))
    );
    assert_eq!(skills[0].mode(), Some(MaterializationKind::Copied));
    assert_eq!(skills[1].mode(), Some(MaterializationKind::Linked));

    reduce(&mut model, Action::Escape);
    assert!(model.is_collapsed("elastic/agent-skills"));
}

#[test]
fn vim_keys_and_save_keys_map_to_the_approved_actions() {
    assert_eq!(
        action_for_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE)),
        Some(Action::MoveDown)
    );
    assert_eq!(
        action_for_key(KeyEvent::new(KeyCode::Char('K'), KeyModifiers::SHIFT)),
        Some(Action::PreviousGroup)
    );
    assert_eq!(
        action_for_key(KeyEvent::new(KeyCode::Char('l'), KeyModifiers::NONE)),
        Some(Action::Expand)
    );
    assert_eq!(
        action_for_key(KeyEvent::new(KeyCode::Char('l'), KeyModifiers::CONTROL)),
        Some(Action::ToggleWorkspace)
    );
    assert_eq!(
        action_for_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL)),
        Some(Action::Save { fast: true })
    );
    assert_eq!(
        action_for_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE)),
        Some(Action::Save { fast: false })
    );
}

#[test]
fn staged_workspace_switch_requires_discard_or_return() {
    let mut model = Model::new(
        Workspace::Target,
        vec![Row::skill(
            "local/library",
            "skill",
            "Description",
            false,
            true,
            MaterializationKind::Linked,
            "Missing",
        )],
    );
    reduce(&mut model, Action::Toggle);
    let effects = reduce(&mut model, Action::ToggleWorkspace);
    assert!(effects.is_empty());
    assert_eq!(model.overlay(), &Overlay::DiscardWorkspace);

    reduce(&mut model, Action::Escape);
    let effects = reduce(&mut model, Action::Save { fast: false });
    assert_eq!(effects, [Effect::PrepareSave { fast: false }]);
}

#[test]
fn representative_target_table_renders_columns_and_tree_context() {
    let model = Model::new(
        Workspace::Target,
        vec![
            Row::source("local/library", CheckState::Checked),
            Row::skill(
                "local/library",
                "release-checklist",
                "Prepare a consistent project release",
                true,
                true,
                MaterializationKind::Linked,
                "In Sync",
            ),
        ],
    );
    let backend = TestBackend::new(90, 14);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| render(frame, &model)).unwrap();
    let screen = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .enumerate()
        .fold(String::new(), |mut output, (index, cell)| {
            if index > 0 && index % 90 == 0 {
                output.push('\n');
            }
            output.push_str(cell.symbol());
            output
        });
    assert!(screen.contains("Enabled"));
    assert!(screen.contains("Mode"));
    assert!(screen.contains("Skill"));
    assert!(screen.contains("local/library"));
    assert!(screen.contains("release-checklist"));
}

#[test]
fn target_and_directory_editors_collect_input_before_emitting_effects() {
    let mut model = Model::new(Workspace::Target, Vec::new());
    assert!(reduce(&mut model, Action::ChangeTarget).is_empty());
    for character in "../other".chars() {
        reduce(&mut model, Action::Input(character));
    }
    assert_eq!(
        reduce(&mut model, Action::Confirm),
        [Effect::ChangeTargetTo("../other".to_owned())]
    );

    reduce(&mut model, Action::AddDirectory);
    for character in "docs,.docs/skills,Docs".chars() {
        reduce(&mut model, Action::Input(character));
    }
    assert_eq!(
        reduce(&mut model, Action::Confirm),
        [Effect::ApplyDirectoryEdit {
            edit: false,
            value: "docs,.docs/skills,Docs".to_owned(),
        }]
    );
}

#[test]
fn library_add_uses_location_editor_and_delete_requires_confirmation() {
    let mut model = Model::new(Workspace::Library, vec![Row::location("./library")]);
    reduce(&mut model, Action::AddDirectory);
    for character in "~/skills".chars() {
        reduce(&mut model, Action::Input(character));
    }
    assert_eq!(
        reduce(&mut model, Action::Confirm),
        [Effect::ApplyLocationEdit {
            edit: false,
            value: "~/skills".to_owned(),
        }]
    );
    assert!(reduce(&mut model, Action::DeleteDirectory).is_empty());
    assert_eq!(model.overlay(), &Overlay::ConfirmDelete);
    assert_eq!(
        reduce(&mut model, Action::Confirm),
        [Effect::DeleteDirectory]
    );
}

#[test]
fn target_edits_show_staged_outcomes_and_restore_observed_state_when_reverted() {
    let mut model = Model::new(
        Workspace::Target,
        vec![Row::skill(
            "local/library",
            "release-checklist",
            "Prepare a release",
            false,
            true,
            MaterializationKind::Linked,
            "Disabled",
        )],
    );
    reduce(&mut model, Action::Toggle);
    assert_eq!(model.rows()[0].state(), "Staged Enable");
    reduce(&mut model, Action::SwitchMode);
    assert_eq!(model.rows()[0].state(), "Staged Enable");
    reduce(&mut model, Action::Toggle);
    assert_eq!(model.rows()[0].state(), "Disabled");
}

#[test]
fn changing_target_requires_discard_confirmation_when_dirty() {
    let mut model = Model::new(
        Workspace::Target,
        vec![Row::skill(
            "local/library",
            "skill",
            "Description",
            false,
            true,
            MaterializationKind::Linked,
            "Disabled",
        )],
    );
    reduce(&mut model, Action::Toggle);
    assert!(reduce(&mut model, Action::ChangeTarget).is_empty());
    assert_eq!(model.overlay(), &Overlay::DiscardTarget);
    reduce(&mut model, Action::Confirm);
    assert_eq!(model.overlay(), &Overlay::TargetPicker(String::new()));
}

#[test]
fn next_group_from_a_child_selects_the_immediately_following_source() {
    let mut model = Model::new(
        Workspace::Target,
        vec![
            Row::source("first/source", CheckState::Checked),
            Row::skill(
                "first/source",
                "first-skill",
                "First",
                true,
                true,
                MaterializationKind::Linked,
                "In Sync",
            ),
            Row::source("second/source", CheckState::Unchecked),
            Row::skill(
                "second/source",
                "second-skill",
                "Second",
                false,
                true,
                MaterializationKind::Linked,
                "Missing",
            ),
        ],
    );
    reduce(&mut model, Action::MoveDown);

    reduce(&mut model, Action::NextGroup);

    assert_eq!(model.selected_row().unwrap().name(), "second/source");
}
