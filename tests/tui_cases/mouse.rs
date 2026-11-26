//! Mouse behavior scenarios.

use super::*;

#[test]
fn rendered_checkbox_and_row_body_address_the_visible_row() {
    let rows = vec![
        Row::source("source", CheckState::Mixed),
        Row::skill(
            "source",
            "skill",
            "Description",
            false,
            true,
            MaterializationKind::Linked,
            "Missing",
        ),
    ];
    let mut model = Model::new(Workspace::Target, rows);
    let backend = TestBackend::new(100, 14);
    let mut terminal = Terminal::new(backend).unwrap();
    let mut interactions = InteractionMap::default();
    terminal
        .draw(|frame| render(frame, &model, &mut interactions))
        .unwrap();

    let toggle = interactions
        .action_at(2, 5)
        .expect("visible checkbox target");
    reduce(&mut model, toggle);
    assert_eq!(model.selected_row().unwrap().name(), "skill");
    assert_eq!(model.rows()[1].check(), Some(CheckState::Checked));

    let select = interactions
        .action_at(20, 5)
        .expect("visible row body target");
    reduce(&mut model, select);
    assert_eq!(model.selected_row().unwrap().name(), "skill");
    assert_eq!(model.rows()[1].check(), Some(CheckState::Checked));
    assert!(matches!(
        model.overlay(),
        Overlay::Details { title, document, .. }
            if title == "skill" && document == "Description"
    ));
}

#[test]
fn skill_body_click_selects_before_inspecting_without_changing_desired_state() {
    let mut model = two_skill_target();
    let mut terminal = Terminal::new(TestBackend::new(100, 16)).unwrap();
    let mut interactions = InteractionMap::default();
    for label in ["source", "first", "second"] {
        terminal
            .draw(|frame| render(frame, &model, &mut interactions))
            .unwrap();
        let (x, y) = rendered_text_location(terminal.backend().buffer(), label);
        let action = action_for_mouse(
            mouse_event(MouseEventKind::Down(MouseButton::Left), x, y),
            &interactions,
        )
        .expect("visible row body");
        reduce(&mut model, action);
        assert_eq!(model.selected_row().unwrap().name(), label);
        assert_eq!(model.overlay(), &Overlay::None);
    }

    terminal
        .draw(|frame| render(frame, &model, &mut interactions))
        .unwrap();
    let (x, y) = rendered_text_location(terminal.backend().buffer(), "second");
    let inspect = action_for_mouse(
        mouse_event(MouseEventKind::Down(MouseButton::Left), x, y),
        &interactions,
    )
    .expect("already-selected skill body");
    reduce(&mut model, inspect);
    assert!(matches!(
        model.overlay(),
        Overlay::Details { title, document, .. }
            if title == "second" && document == "Second info"
    ));
    assert!(
        model
            .rows()
            .iter()
            .filter_map(|row| row.check())
            .all(|check| check == CheckState::Unchecked)
    );

    reduce(&mut model, Action::Escape);
    terminal
        .draw(|frame| render(frame, &model, &mut interactions))
        .unwrap();
    let x = rendered_checkbox_x(terminal.backend().buffer(), y);
    let toggle = action_for_mouse(
        mouse_event(MouseEventKind::Down(MouseButton::Left), x, y),
        &interactions,
    )
    .expect("selected skill checkbox");
    reduce(&mut model, toggle);
    assert_eq!(model.rows()[2].check(), Some(CheckState::Checked));
    assert_eq!(model.overlay(), &Overlay::None);
}

#[test]
fn source_disclosure_glyph_is_a_collapse_target() {
    let mut model = Model::new(
        Workspace::Target,
        vec![
            Row::source("source", CheckState::Mixed),
            Row::skill(
                "source",
                "skill",
                "Description",
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
    let buffer = terminal.backend().buffer();
    let source_y = rendered_row(buffer, "source").expect("rendered Source row");
    let arrow_x = (0..buffer.area.width)
        .find(|&x| matches!(buffer[(x, source_y)].symbol(), "▾" | "▸"))
        .expect("rendered Source disclosure glyph");
    reduce(
        &mut model,
        interactions
            .action_at(arrow_x, source_y)
            .expect("Source disclosure glyph hit target"),
    );
    assert!(model.is_collapsed("source"));
}

#[test]
fn checkbox_hit_testing_tracks_keyboard_scrolled_filtered_rows() {
    let mut rows = Vec::new();
    for index in 0..24 {
        let name = format!("skill-{index:02}");
        rows.push(Row::skill(
            "source",
            name.clone(),
            name,
            false,
            true,
            MaterializationKind::Linked,
            "",
        ));
    }
    let mut model = Model::new(Workspace::Target, rows);
    for _ in 0..18 {
        reduce(&mut model, Action::MoveDown);
    }
    let backend = TestBackend::new(100, 12);
    let mut terminal = Terminal::new(backend).unwrap();
    let mut interactions = InteractionMap::default();
    terminal
        .draw(|frame| render(frame, &model, &mut interactions))
        .unwrap();
    let buffer = terminal.backend().buffer();
    let y = rendered_row(buffer, "skill-18").expect("scrolled selected skill is shown");
    let x = rendered_checkbox_x(buffer, y);
    for checkbox_cell in x..=x + 2 {
        assert!(
            interactions.action_at(checkbox_cell, y).is_some(),
            "checkbox glyph cell at x={checkbox_cell} has a hit target"
        );
    }
    let click = interactions.action_at(x, y).expect("checkbox hit target");
    reduce(&mut model, click);
    assert_eq!(model.rows()[18].check(), Some(CheckState::Checked));
    assert!(
        model.rows()[..18]
            .iter()
            .all(|row| row.check() == Some(CheckState::Unchecked))
    );
}

#[test]
fn filtered_checkbox_respects_stored_collapse_after_filter_clears() {
    let mut model = Model::new(
        Workspace::Target,
        vec![
            Row::source("source", CheckState::Mixed),
            Row::skill(
                "source",
                "needle",
                "Needle",
                false,
                true,
                MaterializationKind::Linked,
                "",
            ),
            Row::skill(
                "source",
                "other",
                "Other",
                false,
                true,
                MaterializationKind::Linked,
                "",
            ),
        ],
    );
    reduce(&mut model, Action::Collapse);
    reduce(&mut model, Action::StartFilter);
    reduce(&mut model, Action::Input('n'));
    reduce(&mut model, Action::Input('e'));
    reduce(&mut model, Action::Input('e'));
    reduce(&mut model, Action::Input('d'));
    reduce(&mut model, Action::Input('l'));
    reduce(&mut model, Action::Input('e'));
    reduce(&mut model, Action::Confirm);

    let mut terminal = Terminal::new(TestBackend::new(100, 14)).unwrap();
    let mut interactions = InteractionMap::default();
    terminal
        .draw(|frame| render(frame, &model, &mut interactions))
        .unwrap();
    let buffer = terminal.backend().buffer();
    let y = rendered_row(buffer, "needle").expect("matching collapsed child is visible");
    let x = rendered_checkbox_x(buffer, y);
    reduce(
        &mut model,
        interactions
            .action_at(x, y)
            .expect("filtered checkbox target"),
    );
    assert_eq!(model.rows()[1].check(), Some(CheckState::Checked));
    assert_eq!(model.rows()[2].check(), Some(CheckState::Unchecked));

    reduce(&mut model, Action::Escape);
    assert!(model.is_collapsed("source"));
}

#[test]
fn table_headers_status_empty_space_and_borders_are_not_row_actions() {
    let mut model = Model::new(
        Workspace::Target,
        vec![Row::skill(
            "source",
            "skill",
            "Skill",
            false,
            true,
            MaterializationKind::Linked,
            "",
        )],
    );
    let initial = model.selected_row().unwrap().name().to_owned();
    let mut terminal = Terminal::new(TestBackend::new(100, 14)).unwrap();
    let mut interactions = InteractionMap::default();
    terminal
        .draw(|frame| render(frame, &model, &mut interactions))
        .unwrap();
    for (x, y) in [(2, 2), (95, 7), (0, 0)] {
        if let Some(action) = interactions.action_at(x, y) {
            reduce(&mut model, action);
        }
    }
    assert_eq!(model.selected_row().unwrap().name(), initial);
    assert_eq!(model.rows()[0].check(), Some(CheckState::Unchecked));
}

#[test]
fn resize_to_zero_rendered_columns_leaves_blank_row_space_inert() {
    let rows = ["first", "second"]
        .into_iter()
        .map(|name| {
            Row::skill(
                "source",
                name,
                "",
                false,
                true,
                MaterializationKind::Linked,
                "",
            )
        })
        .collect();
    let mut model = Model::new(Workspace::Target, rows);
    let mut terminal = Terminal::new(TestBackend::new(100, 14)).unwrap();
    let mut interactions = InteractionMap::default();
    terminal
        .draw(|frame| render(frame, &model, &mut interactions))
        .unwrap();
    let (_, second_y) = rendered_text_location(terminal.backend().buffer(), "second");
    terminal.backend_mut().resize(3, 14);
    terminal
        .resize(ratatui::layout::Rect::new(0, 0, 3, 14))
        .unwrap();
    terminal
        .draw(|frame| render(frame, &model, &mut interactions))
        .unwrap();
    assert_eq!(terminal.backend().buffer()[(1, second_y)].symbol(), " ");
    assert_eq!(
        action_for_mouse(
            mouse_event(MouseEventKind::Down(MouseButton::Left), 1, second_y),
            &interactions,
        ),
        None
    );
    assert_eq!(model.selected_row().unwrap().name(), "first");

    terminal.backend_mut().resize(100, 14);
    terminal
        .resize(ratatui::layout::Rect::new(0, 0, 100, 14))
        .unwrap();
    terminal
        .draw(|frame| render(frame, &model, &mut interactions))
        .unwrap();
    let (x, y) = rendered_text_location(terminal.backend().buffer(), "second");
    let select = action_for_mouse(
        mouse_event(MouseEventKind::Down(MouseButton::Left), x, y),
        &interactions,
    )
    .expect("rendered row body becomes selectable after widening");
    reduce(&mut model, select);
    assert_eq!(model.selected_row().unwrap().name(), "second");
    assert!(
        model
            .rows()
            .iter()
            .filter_map(|row| row.check())
            .all(|check| check == CheckState::Unchecked)
    );
}

#[test]
fn mouse_press_toggles_the_rendered_checkbox_once() {
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
    let mut terminal = Terminal::new(TestBackend::new(100, 14)).unwrap();
    let mut interactions = InteractionMap::default();
    terminal
        .draw(|frame| render(frame, &model, &mut interactions))
        .unwrap();
    let buffer = terminal.backend().buffer();
    let y = rendered_row(buffer, "skill").expect("visible skill row");
    let x = rendered_checkbox_x(buffer, y);

    let press = action_for_mouse(
        mouse_event(MouseEventKind::Down(MouseButton::Left), x, y),
        &interactions,
    )
    .expect("rendered checkbox press");
    reduce(&mut model, press);
    assert_eq!(model.rows()[0].check(), Some(CheckState::Checked));
    assert_eq!(
        action_for_mouse(
            mouse_event(MouseEventKind::Up(MouseButton::Left), x, y),
            &interactions
        ),
        None
    );
    assert_eq!(model.rows()[0].check(), Some(CheckState::Checked));
}

#[test]
fn refreshed_row_insertion_rebuilds_checkbox_targets() {
    let mut model = Model::new(
        Workspace::Target,
        vec![Row::skill(
            "source",
            "beta",
            "Beta",
            false,
            true,
            MaterializationKind::Linked,
            "",
        )],
    );
    let mut terminal = Terminal::new(TestBackend::new(100, 14)).unwrap();
    let mut interactions = InteractionMap::default();
    terminal
        .draw(|frame| render(frame, &model, &mut interactions))
        .unwrap();
    model = Model::new(
        Workspace::Target,
        vec![
            Row::skill(
                "source",
                "alpha",
                "Alpha",
                false,
                true,
                MaterializationKind::Linked,
                "",
            ),
            Row::skill(
                "source",
                "beta",
                "Beta",
                false,
                true,
                MaterializationKind::Linked,
                "",
            ),
        ],
    );
    terminal
        .draw(|frame| render(frame, &model, &mut interactions))
        .unwrap();
    let buffer = terminal.backend().buffer();
    let y = rendered_row(buffer, "alpha").expect("refreshed row is rendered");
    let x = rendered_checkbox_x(buffer, y);
    reduce(
        &mut model,
        interactions
            .action_at(x, y)
            .expect("refreshed checkbox target"),
    );
    assert_eq!(model.rows()[0].check(), Some(CheckState::Checked));
    assert_eq!(model.rows()[1].check(), Some(CheckState::Unchecked));
}

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

fn two_skill_target() -> Model {
    Model::new(
        Workspace::Target,
        vec![
            Row::source("source", CheckState::Unchecked),
            Row::skill(
                "source",
                "first",
                "First info",
                false,
                true,
                MaterializationKind::Linked,
                "",
            ),
            Row::skill(
                "source",
                "second",
                "Second info",
                false,
                true,
                MaterializationKind::Linked,
                "",
            ),
        ],
    )
}
