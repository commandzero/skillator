//! Editors behavior scenarios.

use super::*;

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
    for height in [7, 10, 11] {
        for initial_selection in 0..2 {
            let mut model = Model::new(Workspace::Target, vec![]);
            reduce(&mut model, Action::NewTargetTab);
            if initial_selection == 1 {
                reduce(&mut model, Action::MoveDown);
            }
            let mut terminal = Terminal::new(TestBackend::new(140, height)).unwrap();
            let mut interactions = InteractionMap::default();
            terminal
                .draw(|frame| render(frame, &model, &mut interactions))
                .unwrap();
            let (initial_label, direction, next_label, path) = if initial_selection == 0 {
                (
                    "Generic",
                    MouseEventKind::ScrollDown,
                    "Claude",
                    ".claude/skills",
                )
            } else {
                (
                    "Claude",
                    MouseEventKind::ScrollUp,
                    "Generic",
                    ".agents/skills",
                )
            };
            let (x, y) = rendered_text_location(terminal.backend().buffer(), initial_label);
            let movement = action_for_mouse(mouse_event(direction, x, y), &interactions).unwrap();
            reduce(&mut model, movement);
            terminal
                .draw(|frame| render(frame, &model, &mut interactions))
                .unwrap();
            rendered_text_location(terminal.backend().buffer(), next_label);
            let (x, y) = rendered_text_location(terminal.backend().buffer(), "stage");
            let stage = action_for_mouse(
                mouse_event(MouseEventKind::Down(MouseButton::Left), x, y),
                &interactions,
            )
            .unwrap();
            assert_eq!(
                reduce(&mut model, stage),
                [Effect::ApplyDirectoryEdit {
                    edit: false,
                    value: path.to_owned(),
                }]
            );
        }
    }
}

#[test]
fn fully_clipped_chooser_never_activates_background_commands() {
    let mut model = Model::new(Workspace::Target, vec![]);
    reduce(&mut model, Action::NewTargetTab);
    let mut terminal = Terminal::new(TestBackend::new(140, 1)).unwrap();
    let mut interactions = InteractionMap::default();
    terminal
        .draw(|frame| render(frame, &model, &mut interactions))
        .unwrap();
    let (x, y) = rendered_text_location(terminal.backend().buffer(), "help");
    assert_eq!(
        action_for_mouse(
            mouse_event(MouseEventKind::Down(MouseButton::Left), x, y),
            &interactions,
        ),
        None
    );
    assert!(matches!(model.overlay(), Overlay::DirectoryChooser { .. }));
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
