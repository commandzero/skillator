//! Scrolling behavior scenarios.

use super::*;

#[test]
fn table_wheel_moves_one_visible_row_and_stops_at_each_end() {
    let mut model = Model::new(
        Workspace::Target,
        vec![
            Row::skill(
                "source",
                "first",
                "First",
                false,
                true,
                MaterializationKind::Linked,
                "",
            ),
            Row::skill(
                "source",
                "second",
                "Second",
                false,
                true,
                MaterializationKind::Linked,
                "",
            ),
        ],
    );
    let mut terminal = Terminal::new(TestBackend::new(100, 14)).unwrap();
    let mut interactions = InteractionMap::default();
    terminal
        .draw(|frame| render(frame, &model, &mut interactions))
        .unwrap();
    let first_row = rendered_row(terminal.backend().buffer(), "first").unwrap();

    let wheel_down = action_for_mouse(
        mouse_event(MouseEventKind::ScrollDown, 30, first_row),
        &interactions,
    )
    .expect("table wheel target");
    reduce(&mut model, wheel_down);
    assert_eq!(model.selected_row().unwrap().name(), "second");
    assert_eq!(
        model
            .rows()
            .iter()
            .filter(|row| row.check() == Some(CheckState::Checked))
            .count(),
        0
    );

    terminal
        .draw(|frame| render(frame, &model, &mut interactions))
        .unwrap();
    let second_row = rendered_row(terminal.backend().buffer(), "second").unwrap();
    let wheel_down = action_for_mouse(
        mouse_event(MouseEventKind::ScrollDown, 30, second_row),
        &interactions,
    )
    .expect("table wheel target at final row");
    reduce(&mut model, wheel_down);
    assert_eq!(model.selected_row().unwrap().name(), "second");

    let wheel_up = action_for_mouse(
        mouse_event(MouseEventKind::ScrollUp, 30, second_row),
        &interactions,
    )
    .expect("table wheel target at final row");
    reduce(&mut model, wheel_up);
    assert_eq!(model.selected_row().unwrap().name(), "first");
}

#[test]
fn filtered_hidden_selection_enters_at_first_visible_row_for_either_wheel_direction() {
    for initial_direction in [MouseEventKind::ScrollDown, MouseEventKind::ScrollUp] {
        let mut model = filtered_model_with_hidden_selection();

        let mut terminal = Terminal::new(TestBackend::new(100, 14)).unwrap();
        let mut interactions = InteractionMap::default();
        let movements = [
            (initial_direction, "source"),
            (MouseEventKind::ScrollDown, "match-first"),
            (MouseEventKind::ScrollDown, "match-last"),
            (MouseEventKind::ScrollDown, "match-last"),
            (MouseEventKind::ScrollUp, "match-first"),
            (MouseEventKind::ScrollUp, "source"),
            (MouseEventKind::ScrollUp, "source"),
        ];
        for (direction, expected) in movements {
            terminal
                .draw(|frame| render(frame, &model, &mut interactions))
                .unwrap();
            let y = rendered_row(terminal.backend().buffer(), "match-first").unwrap();
            let action = action_for_mouse(mouse_event(direction, 30, y), &interactions).unwrap();
            assert!(reduce(&mut model, action).is_empty());
            assert_eq!(model.selected_row().unwrap().name(), expected);
        }
        assert!(
            model
                .rows()
                .iter()
                .filter_map(|row| row.check())
                .all(|check| check == CheckState::Unchecked)
        );
    }
}

#[test]
fn help_mode_reference_remains_reachable_after_scroll_and_resize() {
    let mut model = Model::new(Workspace::Target, Vec::new());
    reduce(&mut model, Action::Help);
    let mut terminal = Terminal::new(TestBackend::new(34, 9)).unwrap();
    let mut interactions = InteractionMap::default();

    let down_events = 40;
    for _ in 0..down_events {
        terminal
            .draw(|frame| render(frame, &model, &mut interactions))
            .unwrap();
        let action = action_for_mouse(
            mouse_event(MouseEventKind::ScrollDown, 17, 4),
            &interactions,
        )
        .expect("Help content wheel target");
        reduce(&mut model, action);
    }
    terminal
        .draw(|frame| render(frame, &model, &mut interactions))
        .unwrap();
    terminal.backend_mut().resize(100, 20);
    terminal
        .resize(ratatui::layout::Rect::new(0, 0, 100, 20))
        .unwrap();
    terminal
        .draw(|frame| render(frame, &model, &mut interactions))
        .unwrap();
    let resized_area = terminal.backend().buffer().area;
    let (x, y) = rendered_wheel_target(
        &interactions,
        MouseEventKind::ScrollUp,
        resized_area.width,
        resized_area.height,
    );
    for _ in 0..down_events {
        let action = action_for_mouse(mouse_event(MouseEventKind::ScrollUp, x, y), &interactions)
            .expect("resized Help content wheel target");
        reduce(&mut model, action);
        terminal
            .draw(|frame| render(frame, &model, &mut interactions))
            .unwrap();
    }

    assert_help_reference_reachable(&mut model, &mut terminal, &mut interactions, resized_area);
}

#[test]
fn help_wheel_moves_exactly_one_displayed_row_in_both_directions() {
    let mut model = Model::new(Workspace::Target, Vec::new());
    reduce(&mut model, Action::Help);
    let mut terminal = Terminal::new(TestBackend::new(100, 20)).unwrap();
    let mut interactions = InteractionMap::default();
    terminal
        .draw(|frame| render(frame, &model, &mut interactions))
        .unwrap();
    let (x, y) = rendered_wheel_target(&interactions, MouseEventKind::ScrollDown, 100, 20);
    let rendered_line = |terminal: &Terminal<TestBackend>, row: u16| {
        let buffer = terminal.backend().buffer();
        (0..buffer.area.width)
            .map(|column| buffer[(column, row)].symbol())
            .collect::<String>()
    };
    let initial = [
        rendered_line(&terminal, y),
        rendered_line(&terminal, y + 1),
        rendered_line(&terminal, y + 2),
    ];
    for (direction, expected) in [
        (MouseEventKind::ScrollDown, &initial[1]),
        (MouseEventKind::ScrollDown, &initial[2]),
        (MouseEventKind::ScrollUp, &initial[1]),
        (MouseEventKind::ScrollUp, &initial[0]),
        (MouseEventKind::ScrollUp, &initial[0]),
    ] {
        let action = action_for_mouse(mouse_event(direction, x, y), &interactions).unwrap();
        reduce(&mut model, action);
        terminal
            .draw(|frame| render(frame, &model, &mut interactions))
            .unwrap();
        assert_eq!(rendered_line(&terminal, y), *expected);
    }
}

#[test]
fn short_details_content_remains_visible_when_wheel_does_not_overrun() {
    let mut model = Model::new(
        Workspace::Target,
        vec![Row::skill(
            "source",
            "brief",
            "Short café λ 東京",
            false,
            true,
            MaterializationKind::Linked,
            "Missing",
        )],
    );
    reduce(&mut model, Action::Confirm);
    let mut terminal = Terminal::new(TestBackend::new(48, 12)).unwrap();
    let mut interactions = InteractionMap::default();
    terminal
        .draw(|frame| render(frame, &model, &mut interactions))
        .unwrap();
    assert!(interactions.detail_scroll_max.is_some());

    for _ in 0..8 {
        let action = action_for_mouse(
            mouse_event(MouseEventKind::ScrollDown, 24, 6),
            &interactions,
        )
        .expect("short Details content wheel target");
        reduce(&mut model, action);
        terminal
            .draw(|frame| render(frame, &model, &mut interactions))
            .unwrap();
    }
    let rendered = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(rendered.contains("Short"));
    assert!(rendered.contains("café"));
}

#[test]
fn rendered_details_wheel_preserves_unicode_content_at_both_bounds() {
    let document = (0..16)
        .map(|index| format!("row {index}: café λ 東京"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut model = Model::new(
        Workspace::Target,
        vec![Row::skill(
            "source",
            "detail-skill",
            document,
            false,
            true,
            MaterializationKind::Linked,
            "Missing",
        )],
    );
    reduce(&mut model, Action::Confirm);
    let mut terminal = Terminal::new(TestBackend::new(40, 10)).unwrap();
    let mut interactions = InteractionMap::default();

    let down_events = 40;
    for _ in 0..down_events {
        terminal
            .draw(|frame| render(frame, &model, &mut interactions))
            .unwrap();
        let action = action_for_mouse(
            mouse_event(MouseEventKind::ScrollDown, 20, 5),
            &interactions,
        )
        .expect("Details content wheel target");
        reduce(&mut model, action);
    }
    terminal
        .draw(|frame| render(frame, &model, &mut interactions))
        .unwrap();
    let rendered = rendered_screen(&terminal, 40);
    assert!(rendered.contains("row 15"));
    assert!(rendered.contains("café"));
    assert!(rendered.contains("東京"));

    let original_area = terminal.backend().buffer().area;
    terminal.backend_mut().resize(40, 18);
    terminal
        .resize(ratatui::layout::Rect::new(0, 0, 40, 18))
        .unwrap();
    terminal
        .draw(|frame| render(frame, &model, &mut interactions))
        .unwrap();
    let resized_area = terminal.backend().buffer().area;
    assert_details_resized(original_area, resized_area);
    let (x, y) = rendered_wheel_target(
        &interactions,
        MouseEventKind::ScrollUp,
        terminal.backend().buffer().area.width,
        terminal.backend().buffer().area.height,
    );
    for _ in 0..down_events {
        let action = action_for_mouse(mouse_event(MouseEventKind::ScrollUp, x, y), &interactions)
            .expect("resized Details content wheel target");
        reduce(&mut model, action);
        terminal
            .draw(|frame| render(frame, &model, &mut interactions))
            .unwrap();
    }
    let rendered = rendered_screen(&terminal, 40);
    assert!(rendered.contains("row 0"));
    assert!(rendered.contains("café"));
    assert!(rendered.contains("λ"));
    assert!(rendered.contains("東京"));
}

#[test]
fn chooser_wheel_stops_at_last_match_without_staging() {
    let mut model = Model::new(Workspace::Target, Vec::new());
    reduce(&mut model, Action::NewTargetTab);
    let mut terminal = Terminal::new(TestBackend::new(120, 24)).unwrap();
    let mut interactions = InteractionMap::default();
    let mut last_selected = 0;

    for _ in 0..40 {
        terminal
            .draw(|frame| render(frame, &model, &mut interactions))
            .unwrap();
        let action = action_for_mouse(
            mouse_event(MouseEventKind::ScrollDown, 60, 13),
            &interactions,
        )
        .expect("chooser suggestion wheel target");
        assert!(reduce(&mut model, action).is_empty());
        if let Overlay::DirectoryChooser { selected, .. } = model.overlay() {
            last_selected = *selected;
        } else {
            panic!("wheel must leave the chooser open");
        }
    }
    terminal
        .draw(|frame| render(frame, &model, &mut interactions))
        .unwrap();
    let action = action_for_mouse(
        mouse_event(MouseEventKind::ScrollDown, 60, 13),
        &interactions,
    )
    .expect("chooser wheel target at final suggestion");
    assert!(reduce(&mut model, action).is_empty());
    assert!(matches!(
        model.overlay(),
        Overlay::DirectoryChooser { selected, .. } if *selected == last_selected
    ));
    let mut custom = Model::new(Workspace::Target, Vec::new());
    reduce(&mut custom, Action::NewTargetTab);
    for character in "custom-agent/skills".chars() {
        reduce(&mut custom, Action::Input(character));
    }
    terminal
        .draw(|frame| render(frame, &custom, &mut interactions))
        .unwrap();
    assert_eq!(
        action_for_mouse(
            mouse_event(MouseEventKind::ScrollDown, 60, 13),
            &interactions,
        ),
        None
    );
    let (stage_x, stage_y) = rendered_text_location(terminal.backend().buffer(), "stage");
    let stage = action_for_mouse(
        mouse_event(MouseEventKind::Down(MouseButton::Left), stage_x, stage_y),
        &interactions,
    )
    .expect("Stage custom chooser input");
    assert_eq!(
        reduce(&mut custom, stage),
        [Effect::ApplyDirectoryEdit {
            edit: false,
            value: "custom-agent/skills".into(),
        }]
    );
}

#[test]
fn confirmation_ignores_wheel_without_changing_its_choice() {
    let mut model = Model::new(
        Workspace::Target,
        vec![Row::skill(
            "source",
            "skill",
            "Description",
            false,
            true,
            MaterializationKind::Linked,
            "Missing",
        )],
    );
    reduce(&mut model, Action::Toggle);
    reduce(&mut model, Action::NextScope);
    let mut terminal = Terminal::new(TestBackend::new(80, 14)).unwrap();
    let mut interactions = InteractionMap::default();
    terminal
        .draw(|frame| render(frame, &model, &mut interactions))
        .unwrap();

    assert_eq!(
        action_for_mouse(mouse_event(MouseEventKind::ScrollDown, 2, 5), &interactions),
        None
    );
    assert_eq!(
        action_for_mouse(
            mouse_event(MouseEventKind::ScrollDown, 40, 7),
            &interactions
        ),
        None
    );
    assert!(matches!(model.overlay(), Overlay::ScopeSwitch { .. }));
}

#[test]
fn background_wheel_is_ignored_while_help_editor_or_filter_is_visible() {
    for (workspace, overlay_action) in [
        (Workspace::Target, Action::Help),
        (Workspace::Library, Action::AddDirectory),
        (Workspace::Target, Action::StartFilter),
    ] {
        let mut model = Model::new(
            workspace,
            vec![Row::skill(
                "source",
                "skill",
                "Description",
                false,
                true,
                MaterializationKind::Linked,
                "Missing",
            )],
        );
        let mut terminal = Terminal::new(TestBackend::new(100, 18)).unwrap();
        let mut interactions = InteractionMap::default();
        terminal
            .draw(|frame| render(frame, &model, &mut interactions))
            .unwrap();
        let checkbox_y = rendered_row(terminal.backend().buffer(), "[ ]").unwrap();
        let checkbox_x = rendered_checkbox_x(terminal.backend().buffer(), checkbox_y);
        reduce(&mut model, overlay_action);
        let selected = model.selected_row().map(Row::name);

        terminal
            .draw(|frame| render(frame, &model, &mut interactions))
            .unwrap();
        assert_eq!(
            action_for_mouse(
                mouse_event(MouseEventKind::ScrollDown, checkbox_x, checkbox_y),
                &interactions,
            ),
            None
        );
        assert_eq!(model.selected_row().map(Row::name), selected);
        assert_ne!(model.overlay(), &Overlay::None);
    }
}

fn assert_help_reference_reachable(
    model: &mut Model,
    terminal: &mut Terminal<TestBackend>,
    interactions: &mut InteractionMap,
    resized_area: ratatui::layout::Rect,
) {
    let max_scroll = interactions.detail_scroll_max.expect("Help scroll bounds");
    let mut found_reference = false;
    for _ in 0..=max_scroll {
        terminal
            .draw(|frame| render(frame, model, interactions))
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
        let (x, y) = rendered_wheel_target(
            interactions,
            MouseEventKind::ScrollDown,
            resized_area.width,
            resized_area.height,
        );
        let action = action_for_mouse(mouse_event(MouseEventKind::ScrollDown, x, y), interactions)
            .expect("resized Help content wheel target");
        reduce(model, action);
    }
    assert!(found_reference, "Help exposes the complete mode reference");
}

fn rendered_screen(terminal: &Terminal<TestBackend>, width: usize) -> String {
    terminal
        .backend()
        .buffer()
        .content()
        .chunks(width)
        .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

fn assert_details_resized(
    original_area: ratatui::layout::Rect,
    resized_area: ratatui::layout::Rect,
) {
    assert_ne!(resized_area, original_area);
    assert_eq!(resized_area.width, 40);
    assert_eq!(resized_area.height, 18);
}
