//! Navigation behavior scenarios.

use super::*;

#[test]
fn workspace_save_commands_keep_distinct_preparation_semantics() {
    for (label, fast) in [("save", false), ("save & exit", true)] {
        let mut model = Model::new(Workspace::Target, vec![]);
        let mut terminal = Terminal::new(TestBackend::new(120, 14)).unwrap();
        let mut interactions = InteractionMap::default();
        terminal
            .draw(|frame| render(frame, &model, &mut interactions))
            .unwrap();
        let (x, y) = rendered_text_location(terminal.backend().buffer(), label);
        let action = action_for_mouse(
            mouse_event(MouseEventKind::Down(MouseButton::Left), x, y),
            &interactions,
        )
        .expect("visible save command");
        assert_eq!(reduce(&mut model, action), [Effect::PrepareSave { fast }]);
    }
}

#[test]
fn status_notice_does_not_block_workspace_tab_navigation() {
    let mut model = Model::new(
        Workspace::Target,
        vec![Row::inherited_user(
            "local/library",
            "skill",
            "Description",
            true,
            "User account",
        )],
    );
    reduce(&mut model, Action::Toggle);
    assert!(matches!(model.overlay(), Overlay::Notice(_)));

    let mut terminal = Terminal::new(TestBackend::new(120, 18)).unwrap();
    let mut interactions = InteractionMap::default();
    terminal
        .draw(|frame| render(frame, &model, &mut interactions))
        .unwrap();
    let y = rendered_row(terminal.backend().buffer(), "User").expect("User scope tab");
    let x = (0..terminal.backend().buffer().area.width)
        .find(|&x| {
            (x..terminal.backend().buffer().area.width)
                .map(|column| terminal.backend().buffer()[(column, y)].symbol())
                .collect::<String>()
                .starts_with("User")
        })
        .expect("User tab label");
    let click = action_for_mouse(
        mouse_event(MouseEventKind::Down(MouseButton::Left), x, y),
        &interactions,
    )
    .expect("visible User tab while notice is shown");
    assert_eq!(
        reduce(&mut model, click),
        [Effect::SwitchToScope {
            destination: skillator::tui::model::Scope::User,
            host_to: None,
            save: false,
            discard: false,
        }]
    );
}

#[test]
fn zero_height_tab_strips_do_not_leave_border_click_targets() {
    let model = Model::new(Workspace::Target, vec![]);
    let mut terminal = Terminal::new(TestBackend::new(100, 5)).unwrap();
    let mut interactions = InteractionMap::default();
    terminal
        .draw(|frame| render(frame, &model, &mut interactions))
        .unwrap();

    let buffer = terminal.backend().buffer();
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            if matches!(buffer[(x, y)].symbol(), "─" | "│" | "┌" | "┐" | "└" | "┘") {
                assert!(
                    interactions.action_at(x, y).is_none(),
                    "border cell at ({x}, {y}) must not activate a hidden tab"
                );
            }
        }
    }
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
    for key in [KeyCode::Down, KeyCode::Up] {
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
    let effects = reduce(&mut model, Action::NextScope);
    assert!(effects.is_empty());
    assert!(matches!(model.overlay(), Overlay::ScopeSwitch { .. }));

    reduce(&mut model, Action::Escape);
    let effects = reduce(&mut model, Action::Save { fast: false });
    assert_eq!(effects, [Effect::PrepareSave { fast: false }]);
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

#[test]
fn keyboard_hidden_selection_preserves_directional_offsets() {
    for (key, expected) in [('j', "match-first"), ('k', "source")] {
        let mut model = filtered_model_with_hidden_selection();
        let action = action_for_key(KeyEvent::new(KeyCode::Char(key), KeyModifiers::NONE)).unwrap();
        reduce(&mut model, action);
        assert_eq!(model.selected_row().unwrap().name(), expected);
    }
}

#[test]
fn clipped_save_exit_label_remains_clickable_at_narrow_width() {
    let mut model = Model::new(Workspace::Target, vec![]);
    let mut terminal = Terminal::new(TestBackend::new(90, 32)).unwrap();
    let mut interactions = InteractionMap::default();
    terminal
        .draw(|frame| render(frame, &model, &mut interactions))
        .unwrap();

    let (label_x, label_y) = rendered_text_location(terminal.backend().buffer(), "save & exit");
    let action = action_for_mouse(
        mouse_event(MouseEventKind::Down(MouseButton::Left), label_x, label_y),
        &interactions,
    )
    .expect("visible save & exit label");
    assert_eq!(
        reduce(&mut model, action),
        [Effect::PrepareSave { fast: true }]
    );
}

#[test]
fn modal_outside_click_isolated_and_return_preserves_staged_work() {
    let mut model = Model::new(
        Workspace::Target,
        vec![Row::skill(
            "source",
            "skill",
            "Description",
            false,
            true,
            MaterializationKind::Linked,
            "",
        )],
    );
    reduce(&mut model, Action::Toggle);

    let mut terminal = Terminal::new(TestBackend::new(120, 18)).unwrap();
    let mut interactions = InteractionMap::default();
    terminal
        .draw(|frame| render(frame, &model, &mut interactions))
        .unwrap();
    let checkbox_y = rendered_row(terminal.backend().buffer(), "[✓]").unwrap();
    let checkbox_x = rendered_checkbox_x(terminal.backend().buffer(), checkbox_y);
    assert_eq!(
        terminal.backend().buffer()[(checkbox_x + 1, checkbox_y)].symbol(),
        "✓"
    );

    reduce(
        &mut model,
        Action::SelectScope(skillator::tui::model::Scope::User),
    );
    assert!(matches!(model.overlay(), Overlay::ScopeSwitch { .. }));

    terminal
        .draw(|frame| render(frame, &model, &mut interactions))
        .unwrap();
    assert_eq!(
        action_for_mouse(
            mouse_event(
                MouseEventKind::Down(MouseButton::Left),
                checkbox_x,
                checkbox_y,
            ),
            &interactions
        ),
        None
    );
    assert_eq!(model.rows()[0].check(), Some(CheckState::Checked));

    let (x, y) = rendered_text_location(terminal.backend().buffer(), "return");
    let return_action = action_for_mouse(
        mouse_event(MouseEventKind::Down(MouseButton::Left), x, y),
        &interactions,
    )
    .expect("visible modal return choice");
    reduce(&mut model, return_action);
    assert_eq!(model.overlay(), &Overlay::None);
    assert_eq!(model.rows()[0].check(), Some(CheckState::Checked));
}

#[test]
fn help_scrolls_to_the_full_mode_reference_and_q_closes_it() {
    let mut model = Model::new(Workspace::Target, Vec::new());
    reduce(&mut model, Action::Help);
    let backend = TestBackend::new(100, 12);
    let mut terminal = Terminal::new(backend).unwrap();
    let mut interactions = InteractionMap::default();
    terminal
        .draw(|frame| render(frame, &model, &mut interactions))
        .unwrap();
    let max_scroll = interactions.detail_scroll_max.expect("Help scroll bounds");
    let mut found_reference = false;
    for _ in 0..=max_scroll {
        terminal
            .draw(|frame| render(frame, &model, &mut interactions))
            .unwrap();
        let screen = terminal
            .backend()
            .buffer()
            .content()
            .chunks(100)
            .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n");
        if screen.contains("Cycle modes: link / copy / repo") {
            found_reference = true;
            break;
        }
        reduce(&mut model, Action::MoveDown);
    }

    assert!(found_reference, "Help exposes the complete mode reference");

    reduce(&mut model, Action::Quit);
    assert_eq!(model.overlay(), &Overlay::None);
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
