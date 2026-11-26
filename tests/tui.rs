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

#[path = "tui_cases/editors.rs"]
mod editors;
#[path = "tui_cases/mouse.rs"]
mod mouse;
#[path = "tui_cases/navigation.rs"]
mod navigation;
#[path = "tui_cases/rendering.rs"]
mod rendering;
#[path = "tui_cases/scrolling.rs"]
mod scrolling;

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

fn mouse_event(kind: MouseEventKind, column: u16, row: u16) -> MouseEvent {
    MouseEvent {
        kind,
        column,
        row,
        modifiers: KeyModifiers::NONE,
    }
}

fn filtered_model_with_hidden_selection() -> Model {
    let mut rows = vec![Row::source("source", CheckState::Unchecked)];
    for name in ["hidden", "match-first", "match-last"] {
        rows.push(Row::skill(
            "source",
            name,
            "",
            false,
            true,
            MaterializationKind::Linked,
            "",
        ));
    }
    let mut model = Model::new(Workspace::Target, rows);
    reduce(&mut model, Action::MoveDown);
    reduce(&mut model, Action::StartFilter);
    for character in "match".chars() {
        reduce(&mut model, Action::Input(character));
    }
    reduce(&mut model, Action::Confirm);
    assert_eq!(model.selected_row().unwrap().name(), "hidden");
    model
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
