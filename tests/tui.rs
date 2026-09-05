use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::style::{Color, Modifier};
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
        action_for_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL)),
        Some(Action::NewTargetTab)
    );
    assert_eq!(
        action_for_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL)),
        Some(Action::Save { fast: true })
    );
    assert_eq!(
        action_for_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE)),
        Some(Action::Save { fast: false })
    );

    for (key, action) in [
        (KeyCode::Left, Action::Collapse),
        (KeyCode::Down, Action::MoveDown),
        (KeyCode::Up, Action::MoveUp),
        (KeyCode::Right, Action::Expand),
    ] {
        assert_eq!(
            action_for_key(KeyEvent::new(key, KeyModifiers::NONE)),
            Some(action)
        );
    }
    for (key, action) in [
        (KeyCode::Left, Action::Collapse),
        (KeyCode::Down, Action::NextGroup),
        (KeyCode::Up, Action::PreviousGroup),
        (KeyCode::Right, Action::Expand),
    ] {
        assert_eq!(
            action_for_key(KeyEvent::new(key, KeyModifiers::SHIFT)),
            Some(action)
        );
    }
    for key in [KeyCode::Left, KeyCode::Down, KeyCode::Up, KeyCode::Right] {
        assert_eq!(
            action_for_key(KeyEvent::new(key, KeyModifiers::CONTROL)),
            None
        );
    }
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
    assert!(!screen.contains("Enabled"));
    assert!(screen.contains("Mode"));
    assert!(screen.contains("Skill"));
    assert!(screen.contains("Description"));
    assert!(screen.contains("Action"));
    assert!(screen.contains("local/library"));
    assert!(screen.contains("release-checklist"));
}

#[test]
fn temporary_messages_use_the_final_status_line() {
    let model = Model::new(
        Workspace::Target,
        vec![
            Row::diagnostic("skill folder needs attention"),
            Row::source("local/library", CheckState::Unchecked),
        ],
    );
    let backend = TestBackend::new(100, 14);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| render(frame, &model)).unwrap();
    let lines = terminal
        .backend()
        .buffer()
        .content()
        .chunks(100)
        .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
        .collect::<Vec<_>>();

    assert!(
        lines
            .last()
            .unwrap()
            .contains("skill folder needs attention")
    );
    assert!(
        lines[..lines.len() - 1]
            .iter()
            .all(|line| !line.contains("skill folder needs attention"))
    );
    assert!(!lines.iter().any(|line| line.contains("Diagnostic")));
}

#[test]
fn notice_messages_use_the_final_status_line_without_a_modal() {
    let mut model = Model::new(
        Workspace::Target,
        vec![Row::inherited_user(
            "local/library",
            "release-checklist",
            "Prepare a release",
            true,
            "User account",
        )],
    );
    reduce(&mut model, Action::Toggle);

    let backend = TestBackend::new(100, 14);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| render(frame, &model)).unwrap();
    let lines = terminal
        .backend()
        .buffer()
        .content()
        .chunks(100)
        .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
        .collect::<Vec<_>>();

    assert!(lines.last().unwrap().contains("User tab"));
    assert!(lines.iter().all(|line| !line.contains("Notice")));
}

#[test]
fn skill_row_color_is_driven_by_pending_action_not_description_prose() {
    let mut model = Model::new(
        Workspace::Target,
        vec![
            Row::source("local/library", CheckState::Checked),
            Row::skill(
                "local/library",
                "cpr-loop",
                "Run a Copilot Pull-request Review Loop until complete with no unresolved Copilot-authored threads.",
                true,
                true,
                MaterializationKind::Linked,
                "In Sync",
            ),
        ],
    );
    let backend = TestBackend::new(120, 14);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal.draw(|frame| render(frame, &model)).unwrap();
    let skill_y = (0..terminal.backend().buffer().area.height)
        .find(|y| {
            (0..terminal.backend().buffer().area.width)
                .map(|x| terminal.backend().buffer()[(x, *y)].symbol())
                .collect::<String>()
                .contains("cpr-loop")
        })
        .expect("Skill row is rendered");
    assert!(
        (0..terminal.backend().buffer().area.width)
            .all(|x| terminal.backend().buffer()[(x, skill_y)].fg != Color::Indexed(220)),
        "status-like prose in Description must not color an In-Sync row as a warning"
    );

    reduce(&mut model, Action::MoveDown);
    reduce(&mut model, Action::Toggle);
    reduce(&mut model, Action::MoveUp);
    terminal.draw(|frame| render(frame, &model)).unwrap();
    assert!(
        (0..terminal.backend().buffer().area.width)
            .any(|x| terminal.backend().buffer()[(x, skill_y)].fg == Color::Indexed(196)),
        "a Disable action should use the removed-action accent"
    );
}

#[test]
fn rendered_palette_preserves_semantics_and_structure() {
    let mut model = Model::new(
        Workspace::Target,
        vec![
            Row::source("local/library", CheckState::Unchecked),
            Row::skill(
                "local/library",
                "release-checklist",
                "Prepare a release",
                false,
                true,
                MaterializationKind::Linked,
                "Missing",
            ),
        ],
    );
    let backend = TestBackend::new(120, 14);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| render(frame, &model)).unwrap();
    let cells = terminal.backend().buffer().content();

    assert!(
        cells
            .iter()
            .any(|cell| cell.symbol() == "▛" && cell.fg == Color::Indexed(99))
    );
    assert_eq!(cells[0].fg, Color::Indexed(230));
    assert!(cells.iter().any(|cell| {
        matches!(cell.symbol(), "[" | "└")
            && cell.fg == Color::Indexed(240)
            && !cell.modifier.contains(Modifier::DIM)
    }));
    assert!(
        cells
            .iter()
            .any(|cell| cell.bg == Color::Indexed(24) && cell.fg != Color::Black)
    );

    reduce(&mut model, Action::Help);
    terminal.draw(|frame| render(frame, &model)).unwrap();
    assert!(
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .any(|cell| { cell.symbol() == "┌" && cell.fg == Color::Indexed(33) })
    );
}

#[test]
fn inherited_user_skill_renders_as_read_only_user_enablement() {
    let mut model = Model::new(
        Workspace::Target,
        vec![
            Row::inherited_user(
                "local/library",
                "release-checklist",
                "Prepare a release",
                true,
                "User account",
            ),
            Row::source("another/library", CheckState::Unchecked),
        ],
    );
    reduce(&mut model, Action::MoveDown);
    let backend = TestBackend::new(90, 14);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| render(frame, &model)).unwrap();
    let screen = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(screen.contains("[u]"));
    assert!(screen.contains("user"));
    assert!(
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .any(|cell| cell.symbol() == "r" && cell.fg == Color::Indexed(240)),
        "inherited User account entries should use the dim foreground"
    );

    reduce(&mut model, Action::MoveUp);
    assert!(reduce(&mut model, Action::Toggle).is_empty());
    assert_eq!(model.rows()[0].check(), Some(CheckState::User));
    std::assert_matches!(
        model.overlay(),
        Overlay::Notice(message) if message.contains("User tab")
    );
    assert!(reduce(&mut model, Action::MoveDown).is_empty());
    assert_eq!(
        model.selected_row().unwrap().check(),
        Some(CheckState::Unchecked)
    );
    assert_eq!(model.overlay(), &Overlay::None);
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

    reduce(&mut model, Action::NewTargetTab);
    assert_eq!(
        model.overlay(),
        &Overlay::DirectoryEditor {
            edit: false,
            input: ".claude".to_owned(),
        }
    );
    assert_eq!(
        reduce(&mut model, Action::Confirm),
        [Effect::ApplyDirectoryEdit {
            edit: false,
            value: ".claude".to_owned(),
        }]
    );
}

#[test]
fn editor_modal_renders_a_visible_input_cursor() {
    let mut model = Model::new(Workspace::Library, Vec::new());
    reduce(&mut model, Action::AddDirectory);
    let backend = TestBackend::new(80, 16);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal.draw(|frame| render(frame, &model)).unwrap();

    assert!(
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .any(|cell| cell.symbol() == "▌" && cell.fg == Color::Indexed(230))
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
fn library_footer_explains_how_to_apply_pending_changes() {
    let model = Model::new(Workspace::Library, vec![Row::location("./library")]);
    let backend = TestBackend::new(140, 14);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| render(frame, &model)).unwrap();
    let lines = terminal
        .backend()
        .buffer()
        .content()
        .chunks(140)
        .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
        .collect::<Vec<_>>();
    let screen = lines.join("\n");

    assert!(screen.contains("s save"));
    assert!(screen.contains("Ctrl+S save & exit"));
    assert!(screen.contains("m mode"));
    assert!(screen.contains("/ filter"));
    assert!(!screen.contains("PgUp/PgDn"));
    assert!(!screen.contains("move/copy/link"));
    assert!(screen.contains("Location"));
    assert!(!screen.contains("Name"));
    let action_line = lines
        .iter()
        .position(|line| line.contains("Ctrl+S save & exit"))
        .unwrap();
    assert!(lines[action_line].ends_with("? help ▟"));
    assert!(
        lines
            .iter()
            .skip(action_line + 1)
            .all(|line| !line.contains('▄'))
    );
    assert!(
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .any(|cell| { cell.symbol() == "▄" && cell.fg == Color::Indexed(33) })
    );
}

#[test]
fn target_footer_keeps_modes_and_navigation_in_help() {
    let model = Model::new(
        Workspace::Target,
        vec![Row::source("local/library", CheckState::Unchecked)],
    );
    let backend = TestBackend::new(140, 14);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| render(frame, &model)).unwrap();
    let screen = terminal
        .backend()
        .buffer()
        .content()
        .chunks(140)
        .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n");

    assert!(screen.contains("m mode"));
    assert!(screen.contains("/ filter"));
    assert!(!screen.contains("PgUp/PgDn"));
    assert!(!screen.contains("link/copy/repo"));
}

#[test]
fn help_scrolls_to_the_full_mode_reference_and_q_closes_it() {
    let mut model = Model::new(Workspace::Target, Vec::new());
    reduce(&mut model, Action::Help);
    for _ in 0..6 {
        reduce(&mut model, Action::MoveDown);
    }

    let backend = TestBackend::new(100, 12);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| render(frame, &model)).unwrap();
    let screen = terminal
        .backend()
        .buffer()
        .content()
        .chunks(100)
        .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n");

    assert!(screen.contains("Cycle modes: link / copy / repo"));
    assert!(screen.contains("q/Esc close"));

    reduce(&mut model, Action::Quit);
    assert_eq!(model.overlay(), &Overlay::None);
}

#[test]
fn library_table_uses_a_blue_frame() {
    let model = Model::new(Workspace::Library, vec![Row::location("./library")]);
    let backend = TestBackend::new(80, 12);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal.draw(|frame| render(frame, &model)).unwrap();

    assert!(
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .any(|cell| cell.symbol() == "▛" && cell.fg == Color::Indexed(33))
    );
}

#[test]
fn library_keeps_the_default_location_in_the_table_until_edit_is_requested() {
    let mut model = Model::new(Workspace::Library, vec![Row::location("./library")]);
    let backend = TestBackend::new(140, 14);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| render(frame, &model)).unwrap();
    let screen = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();

    assert!(!screen.contains("Edit library folder"));
    assert!(screen.contains("./library"));
    assert!(screen.contains("a/e/d location"));

    reduce(&mut model, Action::EditDirectory);
    terminal.draw(|frame| render(frame, &model)).unwrap();
    let editor = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(editor.contains("Edit library folder"));
    assert!(editor.contains("Enter apply · Esc cancel"));
}

#[test]
fn confirmation_uses_a_descriptive_title_and_bottom_border_controls() {
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
    reduce(&mut model, Action::ToggleWorkspace);

    let backend = TestBackend::new(100, 14);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| render(frame, &model)).unwrap();
    let lines = terminal
        .backend()
        .buffer()
        .content()
        .chunks(100)
        .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
        .collect::<Vec<_>>();

    let title_line = lines
        .iter()
        .position(|line| line.contains("Discard changes"))
        .unwrap();
    let footer_line = lines
        .iter()
        .position(|line| line.contains("y/Enter discard"))
        .unwrap();
    assert!(lines[title_line].contains("┌ Discard changes "));
    assert!(lines[footer_line].contains("└"));
    assert!(lines[footer_line].contains("n/Esc return"));
    assert!(
        lines[title_line + 1..footer_line]
            .iter()
            .all(|line| !line.contains("y/Enter") && !line.contains("n/Esc"))
    );
}

#[test]
fn target_edits_show_pending_actions_and_preserve_observed_state() {
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
    assert_eq!(model.rows()[0].action(), "Enable link");
    assert_eq!(model.rows()[0].state(), "Disabled");
    reduce(&mut model, Action::SwitchMode);
    assert_eq!(model.rows()[0].action(), "Enable copy");
    assert_eq!(model.rows()[0].state(), "Disabled");
    reduce(&mut model, Action::Toggle);
    assert_eq!(model.rows()[0].action(), "");
    assert_eq!(model.rows()[0].state(), "Disabled");
}

#[test]
fn pending_filter_keeps_group_context_and_hides_no_op_skills() {
    let mut model = Model::new(
        Workspace::Target,
        vec![
            Row::source("local/library", CheckState::Unchecked),
            Row::skill(
                "local/library",
                "pending-skill",
                "Pending description",
                false,
                true,
                MaterializationKind::Linked,
                "Disabled",
            ),
            Row::skill(
                "local/library",
                "quiet-skill",
                "Quiet description",
                false,
                true,
                MaterializationKind::Linked,
                "Disabled",
            ),
        ],
    );
    reduce(&mut model, Action::MoveDown);
    reduce(&mut model, Action::Toggle);
    reduce(&mut model, Action::StartFilter);
    for character in "pending".chars() {
        reduce(&mut model, Action::Input(character));
    }
    reduce(&mut model, Action::Confirm);

    let backend = TestBackend::new(100, 14);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| render(frame, &model)).unwrap();
    let screen = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();

    assert!(screen.contains("local/library"));
    assert!(screen.contains("pending-skill"));
    assert!(screen.contains("Enable link"));
    assert!(!screen.contains("quiet-skill"));
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
