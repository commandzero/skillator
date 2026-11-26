//! Rendering behavior scenarios.

use super::*;

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
    assert_visible_labels(
        &screen,
        &[
            "Mode",
            "Skill",
            "Description",
            "Action",
            "local/library",
            "release-checklist",
        ],
    );
    assert_absent_labels(&screen, &["Enabled"]);
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

    assert_brand_palette(cells);
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

    assert_visible_labels(&screen, &["m mode", "/ filter"]);
    assert_absent_labels(&screen, &["PgUp/PgDn", "link/copy/repo"]);
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

    assert_visible_labels(&screen, &["local/library", "pending-skill", "Enable link"]);
    assert_absent_labels(&screen, &["quiet-skill"]);
}

fn assert_visible_labels(screen: &str, labels: &[&str]) {
    for label in labels {
        assert!(screen.contains(label), "missing {label}: {screen}");
    }
}
fn assert_absent_labels(screen: &str, labels: &[&str]) {
    for label in labels {
        assert!(!screen.contains(label), "unexpected {label}: {screen}");
    }
}

fn assert_brand_palette(cells: &[ratatui::buffer::Cell]) {
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
}
