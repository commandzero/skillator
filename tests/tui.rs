use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::style::{Color, Modifier};
use ratatui::{Terminal, backend::TestBackend};
use skillator::domain::MaterializationKind;
use skillator::tui::input::action_for_key;
use skillator::tui::input::action_for_mouse;
use skillator::tui::interaction::InteractionMap;
use skillator::tui::model::{Action, CheckState, Effect, Model, Overlay, Row, Workspace};
use skillator::tui::reducer::reduce;
use skillator::tui::render::render;

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
}
fn rendered_row(buffer: &ratatui::buffer::Buffer, text: &str) -> Option<u16> {
    (0..buffer.area.height).find(|&y| {
        let line: String = (0..buffer.area.width)
            .map(|x| buffer[(x, y)].symbol())
            .collect();
        line.contains(text)
    })
}

fn rendered_checkbox_x(buffer: &ratatui::buffer::Buffer, y: u16) -> u16 {
    (0..buffer.area.width.saturating_sub(2))
        .find(|&x| buffer[(x, y)].symbol() == "[" && buffer[(x + 2, y)].symbol() == "]")
        .expect("rendered checkbox")
}

fn rendered_wheel_target(
    interactions: &InteractionMap,
    kind: MouseEventKind,
    width: u16,
    height: u16,
) -> (u16, u16) {
    (0..height)
        .find_map(|y| {
            (0..width).find_map(|x| {
                action_for_mouse(mouse_event(kind, x, y), interactions).map(|_| (x, y))
            })
        })
        .expect("visible content wheel target")
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
fn resized_frame_replaces_old_hit_targets() {
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
    let mut terminal = Terminal::new(TestBackend::new(100, 14)).unwrap();
    let mut interactions = InteractionMap::default();
    terminal
        .draw(|frame| render(frame, &model, &mut interactions))
        .unwrap();
    terminal
        .resize(ratatui::layout::Rect::new(0, 0, 1, 1))
        .unwrap();
    terminal
        .draw(|frame| render(frame, &model, &mut interactions))
        .unwrap();
    let stale = interactions.action_at(2, 5);
    if let Some(action) = stale {
        reduce(&mut model, action);
    }
    assert_eq!(model.rows()[0].check(), Some(CheckState::Unchecked));
}

fn mouse_event(kind: MouseEventKind, column: u16, row: u16) -> MouseEvent {
    MouseEvent {
        kind,
        column,
        row,
        modifiers: KeyModifiers::NONE,
    }
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

fn rendered_text_location(buffer: &ratatui::buffer::Buffer, text: &str) -> (u16, u16) {
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            let suffix = (x..buffer.area.width)
                .map(|column| buffer[(column, y)].symbol())
                .collect::<String>();
            if suffix.starts_with(text) {
                return (x, y);
            }
        }
    }
    panic!("rendered text {text:?}");
}

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
fn chooser_suggestion_selects_before_stage_applies_its_path() {
    let mut model = Model::new(Workspace::Target, vec![]);
    reduce(&mut model, Action::NewTargetTab);
    assert!(matches!(model.overlay(), Overlay::DirectoryChooser { .. }));
    let mut terminal = Terminal::new(TestBackend::new(120, 24)).unwrap();
    let mut interactions = InteractionMap::default();
    terminal
        .draw(|frame| render(frame, &model, &mut interactions))
        .unwrap();
    let (suggestion_x, suggestion_y) =
        rendered_text_location(terminal.backend().buffer(), "Generic / Codex");
    let suggestion = action_for_mouse(
        mouse_event(
            MouseEventKind::Down(MouseButton::Left),
            suggestion_x,
            suggestion_y,
        ),
        &interactions,
    )
    .expect("visible directory suggestion");
    reduce(&mut model, suggestion);
    assert!(matches!(
        model.overlay(),
        Overlay::DirectoryChooser { selected: 0, .. }
    ));

    terminal
        .draw(|frame| render(frame, &model, &mut interactions))
        .unwrap();
    let (stage_x, stage_y) = rendered_text_location(terminal.backend().buffer(), "stage");
    let stage = action_for_mouse(
        mouse_event(MouseEventKind::Down(MouseButton::Left), stage_x, stage_y),
        &interactions,
    )
    .expect("visible Stage command");
    assert_eq!(
        reduce(&mut model, stage),
        [Effect::ApplyDirectoryEdit {
            edit: false,
            value: ".agents/skills".into(),
        }]
    );
    assert_eq!(model.overlay(), &Overlay::None);
}

#[test]
fn short_chooser_clicks_only_visible_suggestions_and_stages_selected_path() {
    let mut model = Model::new(Workspace::Target, vec![]);
    reduce(&mut model, Action::NewTargetTab);
    reduce(&mut model, Action::MoveDown);
    let mut terminal = Terminal::new(TestBackend::new(140, 11)).unwrap();
    let mut interactions = InteractionMap::default();
    terminal
        .draw(|frame| render(frame, &model, &mut interactions))
        .unwrap();

    let (generic_x, generic_y) = rendered_text_location(terminal.backend().buffer(), "Generic");
    let blank_y = generic_y - 1;
    assert_eq!(
        terminal.backend().buffer()[(generic_x, blank_y)].symbol(),
        " "
    );
    assert_eq!(
        action_for_mouse(
            mouse_event(MouseEventKind::Down(MouseButton::Left), generic_x, blank_y),
            &interactions,
        ),
        None
    );
    assert!(matches!(
        model.overlay(),
        Overlay::DirectoryChooser { selected: 1, .. }
    ));

    let generic = action_for_mouse(
        mouse_event(
            MouseEventKind::Down(MouseButton::Left),
            generic_x,
            generic_y,
        ),
        &interactions,
    )
    .expect("visible Generic suggestion");
    reduce(&mut model, generic);
    assert!(matches!(
        model.overlay(),
        Overlay::DirectoryChooser { selected: 0, .. }
    ));

    terminal
        .draw(|frame| render(frame, &model, &mut interactions))
        .unwrap();
    let (stage_x, stage_y) = rendered_text_location(terminal.backend().buffer(), "stage");
    let stage = action_for_mouse(
        mouse_event(MouseEventKind::Down(MouseButton::Left), stage_x, stage_y),
        &interactions,
    )
    .expect("visible Stage command");
    assert_eq!(
        reduce(&mut model, stage),
        [Effect::ApplyDirectoryEdit {
            edit: false,
            value: ".agents/skills".into(),
        }]
    );
    assert!(matches!(model.overlay(), Overlay::None));
}

#[test]
fn chooser_keeps_selected_suggestion_visible_at_short_height_boundaries() {
    let mut model = Model::new(Workspace::Target, vec![]);
    reduce(&mut model, Action::NewTargetTab);
    reduce(&mut model, Action::MoveDown);

    for height in [10, 7] {
        let mut terminal = Terminal::new(TestBackend::new(140, height)).unwrap();
        terminal
            .draw(|frame| render(frame, &model, &mut InteractionMap::default()))
            .unwrap();
        rendered_text_location(terminal.backend().buffer(), "Claude");
    }
}

#[test]
fn clicking_an_editor_input_keeps_keyboard_text_literal() {
    let mut model = Model::new(Workspace::Library, vec![]);
    reduce(&mut model, Action::AddDirectory);
    assert!(matches!(model.overlay(), Overlay::LocationEditor { .. }));
    let mut terminal = Terminal::new(TestBackend::new(100, 18)).unwrap();
    let mut interactions = InteractionMap::default();
    terminal
        .draw(|frame| render(frame, &model, &mut interactions))
        .unwrap();
    let (x, y) = rendered_text_location(terminal.backend().buffer(), "> ");
    let click = action_for_mouse(
        mouse_event(MouseEventKind::Down(MouseButton::Left), x, y),
        &interactions,
    );
    if let Some(action) = click {
        reduce(&mut model, action);
    }
    assert!(matches!(model.overlay(), Overlay::LocationEditor { .. }));
    assert!(reduce(&mut model, Action::Input('q')).is_empty());
    assert!(matches!(
        model.overlay(),
        Overlay::LocationEditor { input, .. } if input == "q"
    ));
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
    terminal
        .draw(|frame| render(frame, &model, &mut InteractionMap::default()))
        .unwrap();
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
    terminal
        .draw(|frame| render(frame, &model, &mut InteractionMap::default()))
        .unwrap();
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
    terminal
        .draw(|frame| render(frame, &model, &mut InteractionMap::default()))
        .unwrap();
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

    terminal
        .draw(|frame| render(frame, &model, &mut InteractionMap::default()))
        .unwrap();
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
    terminal
        .draw(|frame| render(frame, &model, &mut InteractionMap::default()))
        .unwrap();
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
    terminal
        .draw(|frame| render(frame, &model, &mut InteractionMap::default()))
        .unwrap();
    let cells = terminal.backend().buffer().content();

    assert!(
        cells
            .iter()
            .any(|cell| cell.symbol() == "▛" && cell.fg == Color::Indexed(99))
    );
    assert!(
        cells
            .iter()
            .any(|cell| cell.symbol() == "S" && cell.fg == Color::Indexed(230))
    );
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
    terminal
        .draw(|frame| render(frame, &model, &mut InteractionMap::default()))
        .unwrap();
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
    terminal
        .draw(|frame| render(frame, &model, &mut InteractionMap::default()))
        .unwrap();
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
fn target_and_directory_chooser_collect_input_before_emitting_effects() {
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
    for character in "cla".chars() {
        reduce(&mut model, Action::Input(character));
    }
    assert_eq!(
        reduce(&mut model, Action::Confirm),
        [Effect::ApplyDirectoryEdit {
            edit: false,
            value: ".claude/skills".to_owned(),
        }]
    );

    reduce(&mut model, Action::NewTargetTab);
    assert!(
        matches!(model.overlay(), Overlay::DirectoryChooser { input, selected: 0 } if input.is_empty())
    );
    for character in "custom-agent/skills".chars() {
        reduce(&mut model, Action::Input(character));
    }
    assert_eq!(
        reduce(&mut model, Action::Confirm),
        [Effect::ApplyDirectoryEdit {
            edit: false,
            value: "custom-agent/skills".to_owned(),
        }]
    );
}

#[test]
fn editor_modal_renders_a_visible_input_cursor() {
    let mut model = Model::new(Workspace::Library, Vec::new());
    reduce(&mut model, Action::AddDirectory);
    let backend = TestBackend::new(80, 16);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal
        .draw(|frame| render(frame, &model, &mut InteractionMap::default()))
        .unwrap();

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
fn target_footer_keeps_modes_and_navigation_in_help() {
    let model = Model::new(
        Workspace::Target,
        vec![Row::source("local/library", CheckState::Unchecked)],
    );
    let backend = TestBackend::new(140, 14);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| render(frame, &model, &mut InteractionMap::default()))
        .unwrap();
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
fn library_keeps_the_default_location_in_the_table_until_edit_is_requested() {
    let mut model = Model::new(Workspace::Library, vec![Row::location("./library")]);
    let backend = TestBackend::new(140, 14);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| render(frame, &model, &mut InteractionMap::default()))
        .unwrap();
    let screen = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();

    assert_eq!(model.overlay(), &Overlay::None);
    assert!(screen.contains("./library"));

    reduce(&mut model, Action::EditDirectory);
    assert!(matches!(model.overlay(), Overlay::LocationEditor { .. }));
}

#[test]
fn confirmation_uses_a_descriptive_title() {
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
    reduce(&mut model, Action::NextScope);

    let backend = TestBackend::new(100, 14);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| render(frame, &model, &mut InteractionMap::default()))
        .unwrap();
    let lines = terminal
        .backend()
        .buffer()
        .content()
        .chunks(100)
        .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
        .collect::<Vec<_>>();
    let title_line = lines
        .iter()
        .position(|line| line.contains("Switch scope"))
        .expect("visible scope-switch title");
    assert!(lines[title_line].contains("┌ Switch scope "));
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
    terminal
        .draw(|frame| render(frame, &model, &mut InteractionMap::default()))
        .unwrap();
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

#[test]
fn rendered_help_wheel_stays_within_wrapped_content_after_resize() {
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
    let narrow = terminal
        .backend()
        .buffer()
        .content()
        .chunks(34)
        .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(narrow.contains("Esc"), "narrow Help retains its close key");

    let original_area = terminal.backend().buffer().area;
    terminal.backend_mut().resize(100, 20);
    terminal
        .resize(ratatui::layout::Rect::new(0, 0, 100, 20))
        .unwrap();
    terminal
        .draw(|frame| render(frame, &model, &mut interactions))
        .unwrap();
    let resized_area = terminal.backend().buffer().area;
    assert_ne!(resized_area, original_area);
    assert_eq!(resized_area.width, 100);
    assert_eq!(resized_area.height, 20);
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
        let (x, y) = rendered_wheel_target(
            &interactions,
            MouseEventKind::ScrollDown,
            resized_area.width,
            resized_area.height,
        );
        let action = action_for_mouse(mouse_event(MouseEventKind::ScrollDown, x, y), &interactions)
            .expect("resized Help content wheel target");
        reduce(&mut model, action);
    }
    assert!(found_reference, "Help exposes the complete mode reference");
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
    let rendered = terminal
        .backend()
        .buffer()
        .content()
        .chunks(40)
        .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n");
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
    assert_ne!(resized_area, original_area);
    assert_eq!(resized_area.width, 40);
    assert_eq!(resized_area.height, 18);
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
    let rendered = terminal
        .backend()
        .buffer()
        .content()
        .chunks(40)
        .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n");
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
