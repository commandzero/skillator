//! Shared terminal rendering, palettes and read-only inspectors.

use super::controls::{overlay_controls, workspace_controls};
use super::input::chooser_matches;
use super::interaction::InteractionMap;
use super::model::{
    Action, CheckState, Model, Overlay, Row, RowAction, RowKind, Scope, TargetTabScope, Workspace,
    row_identity,
};
use crate::domain::MaterializationKind;
use ratatui::Frame;
use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::symbols::border;
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{
    Block, Borders, Cell, Clear, Paragraph, Row as TableRow, Table, TableState,
};
use std::collections::BTreeSet;

pub(super) const PURPLE: Color = Color::Indexed(99);
pub(super) const BLUE: Color = Color::Indexed(33);
pub(super) const BONE: Color = Color::Indexed(230);
pub(super) const ADD: Color = Color::Indexed(114);
pub(super) const MODIFY: Color = Color::Indexed(45);
pub(super) const WARNING: Color = Color::Indexed(220);
pub(super) const ERROR: Color = Color::Indexed(196);
pub(super) const DIM_FOREGROUND: Color = Color::Indexed(240);
pub(super) const DARK_MAGENTA: Color = Color::Indexed(90);
pub(super) const SELECTED_BACKGROUND: Color = Color::Indexed(24);
const TAB_TOP_BORDER: border::Set = border::Set {
    top_left: "▛",
    top_right: "▜",
    horizontal_top: "▀",
    vertical_left: "▌",
    vertical_right: "▐",
    bottom_left: "▙",
    bottom_right: "▟",
    horizontal_bottom: "▄",
};

impl Scope {
    fn color(self) -> Color {
        match self {
            Self::Library => Color::Indexed(245),
            Self::User => BLUE,
            Self::Repo => PURPLE,
        }
    }
}

fn dim_style() -> Style {
    Style::default().fg(DIM_FOREGROUND)
}

fn dim_span(text: impl Into<String>) -> Span<'static> {
    Span::styled(text.into(), dim_style())
}

fn row_cells(
    row: &Row,
    check: &str,
    mode: &str,
    collapsed: &BTreeSet<String>,
    filter: &str,
    last_child: bool,
    selected: bool,
) -> Vec<Cell<'static>> {
    let subdued = Style::default().fg(if selected {
        Color::Indexed(7)
    } else {
        DIM_FOREGROUND
    });
    if row.kind == RowKind::Location {
        return vec![
            Cell::from(Span::styled("─────────", subdued)),
            Cell::from(Span::styled("──────", subdued)),
            Cell::from(Line::from(vec![
                Span::styled("── ", subdued),
                Span::raw(row.name.clone()),
                Span::styled(" ", subdued),
            ])),
            Cell::from(Span::styled(
                "────────────────────────────────────────────────────────────────",
                subdued,
            )),
            Cell::from(Span::styled("────────────────────────────────", subdued)),
        ];
    }

    let check = if row.check == Some(CheckState::Unchecked) {
        Cell::from(Span::styled(check.to_owned(), subdued))
    } else {
        Cell::from(check.to_owned())
    };
    let name = match row.kind {
        RowKind::Source => {
            let glyph = if row_identity(row).is_some_and(|identity| collapsed.contains(identity))
                && filter.is_empty()
            {
                "▸"
            } else {
                "▾"
            };
            Cell::from(Line::from(vec![
                Span::styled(format!("{glyph} "), subdued),
                Span::raw(row.name.clone()),
            ]))
        }
        RowKind::Skill => Cell::from(Line::from(vec![
            Span::styled(if last_child { "  └─ " } else { "  ├─ " }, subdued),
            Span::raw(row.name.clone()),
        ])),
        RowKind::Diagnostic => Cell::from(format!("! {}", row.name)),
        RowKind::Location => unreachable!(),
    };
    vec![
        check,
        Cell::from(mode.to_owned()),
        name,
        Cell::from(row.description.clone()),
        Cell::from(row.action.clone()),
    ]
}

pub(super) fn row_style(row: &Row, selected: bool) -> Style {
    let mut style = if row_is_error(row) {
        Style::default().fg(ERROR)
    } else if row_is_conflict(row) {
        Style::default().fg(WARNING)
    } else if row.check == Some(CheckState::User) || !row.available {
        dim_style()
    } else if let Some(color) = pending_action_color(&row.action) {
        Style::default().fg(color)
    } else {
        Style::default()
    };
    if selected {
        style = Style::default()
            .fg(Color::Indexed(15))
            .bg(SELECTED_BACKGROUND);
    }
    style
}

pub(super) fn row_is_error(row: &Row) -> bool {
    if !row.valid || row.check == Some(CheckState::Invalid) || row.key_collision {
        return true;
    }
    matches!(
        row.state.as_str(),
        "Error" | "Invalid" | "Blocked" | "Failed" | "Recovery Required"
    )
}

fn row_is_conflict(row: &Row) -> bool {
    row.kind == RowKind::Diagnostic
        && [
            row.name.as_str(),
            row.description.as_str(),
            row.state.as_str(),
        ]
        .into_iter()
        .any(|value| {
            contains_any(
                value,
                &[
                    "conflict",
                    "needs confirmation",
                    "duplicate name",
                    "collision",
                ],
            )
        })
}

fn pending_action_color(action: &str) -> Option<Color> {
    if action.is_empty() {
        return None;
    }

    if action == "Disable" || action.starts_with("Unregister") {
        return Some(ERROR);
    }
    if action.starts_with("Move to")
        || action.starts_with("Convert to")
        || action.starts_with("Repair")
    {
        return Some(MODIFY);
    }
    if action.starts_with("Enable")
        || action == "Register"
        || action.starts_with("Register ")
        || action.starts_with("Track ")
        || action.starts_with("Copy to")
        || action.starts_with("Link to")
    {
        return Some(ADD);
    }

    None
}

fn contains_any(value: &str, needles: &[&str]) -> bool {
    let value = value.to_ascii_lowercase();
    needles.iter().any(|needle| value.contains(needle))
}

fn controls_line(
    controls: &[super::controls::Control],
    width: u16,
    origin_x: u16,
    y: u16,
    interactions: &mut InteractionMap,
) -> Line<'static> {
    let visible_width = usize::from(width.saturating_sub(1));
    let controls_text: Vec<String> = controls
        .iter()
        .map(|control| format!("{} {}", control.key, control.label))
        .collect();
    let control_widths: Vec<usize> = controls_text
        .iter()
        .map(|text| Line::from(text.as_str()).width())
        .collect();
    let controls_width: usize = control_widths.iter().sum();
    let wide_separator = Line::from(" · ").width();
    let compact_separator = Line::from(" ").width();
    let wide_width =
        controls_width.saturating_add(controls.len().saturating_sub(1) * wide_separator);
    let separator = if wide_width <= visible_width {
        " · "
    } else {
        " "
    };
    let separator_width = if separator == " · " {
        wide_separator
    } else {
        compact_separator
    };
    let total_width =
        controls_width.saturating_add(controls.len().saturating_sub(1) * separator_width);
    let clip = total_width.saturating_sub(visible_width);
    let padding = visible_width.saturating_sub(total_width);
    let mut cursor = 0usize;
    let mut spans = Vec::new();
    for (index, (control, text, cells)) in controls
        .iter()
        .zip(controls_text)
        .zip(control_widths)
        .map(|((control, text), cells)| (control, text, cells))
        .enumerate()
    {
        if index > 0 {
            spans.push(dim_span(separator));
            cursor += separator_width;
        }
        let end = cursor + cells;
        if cursor >= clip && end <= clip.saturating_add(visible_width) && cells > 0 {
            interactions.add(
                Rect::new(
                    origin_x.saturating_add((padding + cursor.saturating_sub(clip)) as u16),
                    y,
                    cells as u16,
                    1,
                ),
                control.action.clone(),
            );
        }
        spans.push(Span::styled(
            text,
            Style::default().fg(BONE).add_modifier(Modifier::BOLD),
        ));
        cursor = end;
    }
    spans.push(Span::raw(" "));
    Line::from(spans)
}

pub fn render(frame: &mut Frame<'_>, model: &Model, interactions: &mut InteractionMap) {
    interactions.reset(frame.area());
    let areas = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(5),
        Constraint::Length(1),
    ])
    .split(frame.area());
    let scope = model.scope;
    let available = usize::from(areas[0].width);
    let compact = available < 27;
    let mut labels = vec![Span::raw(if compact { "" } else { " " })];
    let mut header_width = usize::from(!compact);
    for candidate in [Scope::Library, Scope::User, Scope::Repo] {
        let label = if compact {
            format!(
                "{}{}",
                if candidate == Scope::Library { "" } else { " " },
                candidate.label()
            )
        } else {
            format!(" {} ", candidate.label())
        };
        let width = Line::from(label.clone()).width();
        if header_width + width > available {
            break;
        }
        let target = Rect::new(
            areas[0].x.saturating_add(header_width as u16),
            areas[0].y,
            width as u16,
            1,
        );
        if let Some(target) = visible_target(target, areas[0]) {
            interactions.add(target, Action::SelectScope(candidate));
        }
        labels.push(Span::styled(
            label,
            if candidate == scope {
                Style::default()
                    .fg(Color::Black)
                    .bg(candidate.color())
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(candidate.color())
            },
        ));
        header_width += width;
    }
    let title = "Skillator";
    if available >= header_width + 1 + title.len() {
        labels.push(Span::raw(
            " ".repeat(available - header_width - title.len()),
        ));
        labels.push(Span::styled(
            title,
            Style::default().fg(BONE).add_modifier(Modifier::BOLD),
        ));
    }
    frame.render_widget(Paragraph::new(Line::from(labels)), areas[0]);
    frame.render_widget(
        Paragraph::new(target_tabs_for_width(
            model,
            areas[1].width,
            areas[1],
            interactions,
        )),
        areas[1],
    );
    let header = match model.workspace {
        Workspace::Target => TableRow::new(["", "Mode", "Skill", "Description", "Action"]),
        Workspace::Library => TableRow::new(["", "Mode", "Location", "Description", "Action"]),
    }
    .style(Style::default().fg(BONE).add_modifier(Modifier::BOLD));
    let visible = model.visible_indices();
    let rows = visible.iter().map(|index| {
        let row = &model.rows[*index];
        let selected = *index == model.selected;
        let check = match row.check {
            Some(CheckState::Checked) => "[✓]",
            Some(CheckState::User) => "[u]",
            Some(CheckState::Repository) => "[r]",
            Some(CheckState::Unchecked) => "[ ]",
            Some(CheckState::Mixed) => "[-]",
            Some(CheckState::Invalid) => "[!]",
            None => "",
        };
        let mode = if row.check == Some(CheckState::User) {
            "user"
        } else if row.check == Some(CheckState::Repository) {
            "repo"
        } else if let Some(mode) = row.acquisition_mode {
            mode.label()
        } else {
            match row.mode {
                Some(MaterializationKind::Linked) => "link",
                Some(MaterializationKind::Copied) => "copy",
                None => "",
            }
        };
        let last_child = row.kind == RowKind::Skill
            && !model.rows.iter().skip(*index + 1).any(|candidate| {
                candidate.kind == RowKind::Skill && row_identity(candidate) == row_identity(row)
            });
        TableRow::new(row_cells(
            row,
            check,
            mode,
            &model.collapsed,
            &model.filter,
            last_child,
            selected,
        ))
        .style(row_style(row, selected))
    });
    let widths = [
        Constraint::Length(4),
        Constraint::Length(6),
        Constraint::Percentage(28),
        Constraint::Min(1),
        Constraint::Length(18),
    ];
    let table_area = areas[2];
    let key_help = if table_area.height > 0 {
        controls_line(
            workspace_controls(model),
            table_area.width.saturating_sub(2),
            table_area.x.saturating_add(1),
            table_area
                .y
                .saturating_add(table_area.height.saturating_sub(1)),
            interactions,
        )
        .right_aligned()
    } else {
        Line::default()
    };
    let border_color = model.scope.color();
    let mut table_state = TableState::default();
    table_state.select(visible.iter().position(|index| *index == model.selected));
    let inner = Rect::new(
        table_area.x.saturating_add(1),
        table_area.y.saturating_add(1),
        table_area.width.saturating_sub(2),
        table_area.height.saturating_sub(2),
    );
    // The table has no highlight symbol, so its first column starts at the
    // inner left edge even when a row is selected.
    let columns = Layout::horizontal(widths)
        .flex(Flex::Start)
        .spacing(1)
        .split(Rect::new(0, 0, inner.width, 1));
    let resolved_widths: Vec<_> = columns
        .iter()
        .map(|column| Constraint::Length(column.width))
        .collect();
    frame.render_stateful_widget(
        Table::new(rows, resolved_widths)
            .column_spacing(1)
            .flex(Flex::Start)
            .header(header)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_set(TAB_TOP_BORDER)
                    .border_style(Style::default().fg(border_color))
                    .title_bottom(key_help),
            ),
        table_area,
        &mut table_state,
    );
    let offset = table_state.offset();
    let body_y = inner.y.saturating_add(1);
    let body_height = inner.height.saturating_sub(1);
    interactions.add_scroll(
        Rect::new(inner.x, body_y, inner.width, body_height),
        Action::MoveUp,
        Action::MoveDown,
    );
    for (screen_row, visible_pos) in (offset..visible.len())
        .take(usize::from(body_height))
        .enumerate()
    {
        let row_index = visible[visible_pos];
        let y = body_y.saturating_add(screen_row as u16);
        interactions.add(
            Rect::new(inner.x, y, inner.width, 1),
            Action::ActivateRow {
                index: row_index,
                intent: RowAction::Select,
            },
        );
        let row = &model.rows[row_index];
        let column_x = |column: usize| inner.x.saturating_add(columns[column].x);
        if row.kind != RowKind::Location && row.check.is_some() {
            interactions.add(
                Rect::new(column_x(0), y, columns[0].width.min(3), 1),
                Action::ActivateRow {
                    index: row_index,
                    intent: RowAction::Toggle,
                },
            );
        }
        if row.kind == RowKind::Skill {
            interactions.add(
                Rect::new(column_x(1), y, columns[1].width, 1),
                Action::ActivateRow {
                    index: row_index,
                    intent: RowAction::SwitchMode,
                },
            );
        }
        if row.kind == RowKind::Source {
            interactions.add(
                Rect::new(column_x(2), y, columns[2].width.min(1), 1),
                Action::ActivateRow {
                    index: row_index,
                    intent: RowAction::ToggleDisclosure,
                },
            );
        }
    }
    frame.render_widget(Paragraph::new(status_line(model, areas[3].width)), areas[3]);
    if !matches!(&model.overlay, Overlay::None | Overlay::Notice(_)) {
        interactions.clear_targets();
    }
    render_overlay(frame, model, interactions);
}

fn visible_target(target: Rect, strip: Rect) -> Option<Rect> {
    let left = target.x.max(strip.x);
    let top = target.y.max(strip.y);
    let right = target.right().min(strip.right());
    let bottom = target.bottom().min(strip.bottom());
    (right > left && bottom > top).then(|| Rect::new(left, top, right - left, bottom - top))
}

fn target_tabs_for_width(
    model: &Model,
    width: u16,
    area: Rect,
    interactions: &mut InteractionMap,
) -> Line<'static> {
    let (labels, active): (Vec<(usize, String)>, usize) = if model.scope == Scope::Library {
        (
            model.host_labels.iter().cloned().enumerate().collect(),
            model.host_index,
        )
    } else {
        (
            model
                .directory_labels
                .iter()
                .enumerate()
                .filter(|(index, _)| {
                    model.directory_scopes.get(*index)
                        == Some(&match model.scope {
                            Scope::User => TargetTabScope::User,
                            _ => TargetTabScope::Repository,
                        })
                })
                .map(|(index, label)| (index, label.clone()))
                .collect(),
            model.directory_index,
        )
    };
    if labels.is_empty() {
        return Line::from(Span::styled(
            if model.scope == Scope::Repo && model.target_path.is_none() {
                " Repo unavailable · choose a Git worktree with t "
            } else {
                " No configured skill directories · Ctrl+T add "
            },
            Style::default().fg(model.scope.color()),
        ));
    }
    let available = usize::from(width);
    let total: usize = 1 + labels
        .iter()
        .map(|(_, label)| Line::from(label.as_str()).width() + 3)
        .sum::<usize>();
    let first = if total > available {
        labels
            .iter()
            .position(|(index, _)| *index == active)
            .unwrap_or(0)
    } else {
        0
    };
    let mut spans = vec![Span::raw(if first == 0 { " " } else { "‹ " })];
    let mut used = if first == 0 { 1 } else { 2 };
    if first > 0
        && let Some(target) = visible_target(Rect::new(area.x, area.y, 1, 1), area)
    {
        interactions.add(target, Action::PreviousDirectory);
    }
    for (position, (index, label)) in labels.iter().enumerate().skip(first) {
        let text = format!(" {label} ");
        let required = Line::from(text.as_str()).width() + 1;
        if position != first && used + required > available {
            if used < available {
                spans.push(Span::styled("›", dim_style()));
                let target = Rect::new(area.x.saturating_add(used as u16), area.y, 1, 1);
                if let Some(target) = visible_target(target, area) {
                    interactions.add(target, Action::NextDirectory);
                }
            }
            break;
        }
        let x = area.x.saturating_add(used as u16);
        let label_width = Line::from(text.as_str()).width() as u16;
        let action = if model.scope == Scope::Library {
            Action::SelectHost(*index)
        } else {
            Action::SelectDirectory(*index)
        };
        if let Some(target) = visible_target(Rect::new(x, area.y, label_width, 1), area) {
            interactions.add(target, action);
        }
        let style = if *index == active {
            Style::default()
                .fg(Color::Black)
                .bg(model.scope.color())
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(DIM_FOREGROUND)
        };
        spans.push(Span::styled(text, style));
        spans.push(Span::raw(" "));
        used += required;
    }
    Line::from(spans)
}

fn status_line(model: &Model, width: u16) -> Line<'static> {
    let anchor = match model.scope {
        Scope::Library if model.host_index == 0 => {
            "Library: Local · ~/.skillator/library.yaml".to_owned()
        }
        Scope::Library => {
            let alias = model
                .host_labels
                .get(model.host_index)
                .map(String::as_str)
                .unwrap_or("Unknown");
            let hostname = model
                .host_hostname
                .as_ref()
                .map_or(String::new(), |host| format!(" ({host})"));
            format!("Library: {alias}{hostname} · {alias}:~/.skillator/library/replica")
        }
        Scope::User => format!(
            "User: {}",
            model
                .directory_paths
                .get(model.directory_index)
                .map_or("unavailable".to_owned(), |path| format!("~/{path}"))
        ),
        Scope::Repo => format!(
            "Repo: {} · {}",
            model.target_path.as_deref().unwrap_or("unavailable"),
            if model.target_path.is_none() {
                "no Git worktree"
            } else if model.directory_scopes.get(model.directory_index)
                == Some(&TargetTabScope::Repository)
            {
                model
                    .directory_paths
                    .get(model.directory_index)
                    .map(String::as_str)
                    .unwrap_or("no skill directory")
            } else {
                "no skill directory"
            }
        ),
    };
    let diagnostic = model
        .rows
        .iter()
        .filter(|row| row.kind == RowKind::Diagnostic)
        .map(|row| row.description.as_str())
        .collect::<Vec<_>>()
        .join(" · ");
    let detail = if let Overlay::Notice(message) = &model.overlay {
        Some(message.as_str())
    } else if let Some(message) = &model.unavailable {
        Some(message.as_str())
    } else if !diagnostic.is_empty() {
        Some(diagnostic.as_str())
    } else if model.dirty || model.host_dirty {
        Some("Unsaved changes")
    } else {
        model.selected_row().map(|row| {
            if row.details.is_empty() {
                row.description.as_str()
            } else {
                row.details.as_str()
            }
        })
    };
    let text = match detail {
        Some(detail) if !detail.is_empty() => format!("{anchor} · {detail}"),
        _ => anchor,
    };
    if width == 0 {
        return Line::default();
    }
    let max = usize::from(width);
    let clipped = if Line::from(text.as_str()).width() <= max {
        text
    } else {
        // Search character boundaries by terminal-cell width instead of repeatedly
        // allocating and measuring an ever-growing prefix of a long path.
        let boundaries: Vec<_> = text
            .char_indices()
            .map(|(offset, _)| offset)
            .chain(std::iter::once(text.len()))
            .collect();
        let mut lower = 0;
        let mut upper = boundaries.len() - 1;
        while lower < upper {
            let middle = lower + (upper - lower).div_ceil(2);
            if Line::from(&text[..boundaries[middle]]).width() < max {
                lower = middle;
            } else {
                upper = middle - 1;
            }
        }
        let mut result = String::from(&text[..boundaries[lower]]);
        result.push('…');
        result
    };
    Line::from(Span::styled(
        clipped,
        Style::default().fg(model.scope.color()),
    ))
}

pub(super) fn render_overlay(
    frame: &mut Frame<'_>,
    model: &Model,
    interactions: &mut InteractionMap,
) {
    if interactions.area != frame.area() {
        interactions.reset(frame.area());
    }
    if !matches!(model.overlay, Overlay::None | Overlay::Notice(_)) {
        interactions.clear_targets();
    }
    let (title, body, confirmation) = match &model.overlay {
        Overlay::None => return,
        Overlay::Welcome => (
            "I AM SKILLATOR!".to_owned(),
            "Your Library starts at ./library. Press e to edit this location. After closing this welcome, Ctrl+H/Ctrl+L move between Library, User and Repo.".to_owned(),
            false,
        ),
        Overlay::Help => return render_help(frame, model, interactions, model.detail_scroll),
        Overlay::Filter => return render_filter(frame, model, interactions),
        Overlay::ConfirmSave => (
            "Save changes".to_owned(),
            "Save your changes?".to_owned(),
            true,
        ),
        Overlay::ConfirmSaveWarning(message) => save_warning_modal(message),
        Overlay::GuardedConfirmation(message) => (
            "Review changes".to_owned(),
            remove_first_line(&confirmation_body(message)),
            true,
        ),
        Overlay::DiscardTarget => (
            "Discard pending changes".to_owned(),
            "Discard unsaved changes before opening another repository?".to_owned(),
            true,
        ),
        Overlay::ScopeSwitch { .. } => (
            "Switch scope".to_owned(),
            "This scope has unsaved changes.".to_owned(),
            true,
        ),
        Overlay::DirectoryChooser { input, selected } => {
            return render_directory_chooser(frame, input, *selected, model, interactions);
        }
        Overlay::FollowerEditor(input) => {
            return render_input(frame, model, interactions, "Follower name",
                "SSH alias from ~/.ssh/config · existing credentials and host trust required", input);
        }
        Overlay::FollowerProbe { name } => (
            "Connecting to follower".to_owned(),
            format!("Checking SSH alias {name} (read-only hostname probe)."),
            false,
        ),
        Overlay::ConfirmFollowerSync { alias, initial } => (
            if *initial {
                format!("Initialize follower {alias}?")
            } else {
                format!("Sync follower {alias}?")
            },
            format!(
                "Push the leader's current saved Library to {alias}? This overwrites follower edits and deletes stale files only inside its owned ~/.skillator/library/replica. Other host files and selections are untouched. Interrupted rsync may leave a partial replica."
            ),
            true,
        ),
        Overlay::DirectoryEditor { edit, input } => {
            let mode = if *edit { "Edit" } else { "New" };
            return render_input(
                frame,
                model,
                interactions,
                &format!("{mode} skill tab"),
                "agents | .claude | key,path,label",
                input,
            );
        }
        Overlay::LocationEditor { edit, input } => {
            let mode = if *edit { "Edit" } else { "Add" };
            return render_input(
                frame,
                model,
                interactions,
                &format!("{mode} library folder"),
                "path",
                input,
            );
        }
        Overlay::SourceKeyEditor(input) => {
            return render_input(
                frame,
                model,
                interactions,
                "Source key",
                "owner/repository",
                input,
            );
        }
        Overlay::TargetPicker(input) => {
            return render_input(frame, model, interactions, "Change repository", "directory", input);
        }
        Overlay::ConfirmDelete => (
            "Remove from configuration".to_owned(),
            "Remove the selected folder from the configuration? This does not delete the folder or its files.".to_owned(),
            true,
        ),
        Overlay::Busy => (
            "Another Skillator process is saving changes".to_owned(),
            "Another Skillator process is saving changes here. Wait for it to finish, then retry.".to_owned(),
            true,
        ),
        Overlay::Details { .. } => {
            return render_skill_details(frame, model, interactions);
        }
        Overlay::Diagnostic { title, body } | Overlay::FollowerSyncResult { title, body } => {
            let area = centered(frame.area(), 76, 70);
            frame.render_widget(Clear, area);
            let block = modal_controls_block(title, &model.overlay, area, interactions);
            let inner = block.inner(area);
            let paragraph = Paragraph::new(body.as_str())
                .wrap(ratatui::widgets::Wrap { trim: true });
            let max = paragraph
                .line_count(inner.width)
                .saturating_sub(usize::from(inner.height))
                .min(usize::from(u16::MAX)) as u16;
            interactions.detail_scroll_max = Some(max);
            let scroll = model.detail_scroll.min(max);
            interactions.add_scroll(inner, Action::MoveUp, Action::MoveDown);
            frame.render_widget(paragraph.scroll((scroll, 0)).block(block), area);
            return;
        }
        Overlay::Notice(_) => return,
        Overlay::Result(message) => (
            "Save result".to_owned(),
            message.to_owned(),
            false,
        ),
    };
    let text = if confirmation {
        confirmation_text(&body)
    } else {
        Text::styled(body, overlay_text_style(&model.overlay))
    };
    let height = if matches!(
        model.overlay,
        Overlay::GuardedConfirmation(_) | Overlay::Result(_)
    ) {
        70
    } else {
        30
    };
    let area = centered(frame.area(), 70, height);
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(text)
            .wrap(ratatui::widgets::Wrap { trim: true })
            .block(modal_controls_block(
                &title,
                &model.overlay,
                area,
                interactions,
            )),
        area,
    );
}

fn render_help(
    frame: &mut Frame<'_>,
    model: &Model,
    interactions: &mut InteractionMap,
    scroll: u16,
) {
    let mut entries = vec![
        ("Navigation", None),
        ("j/k · ↑/↓", Some("Move by row")),
        ("J/K · ⇧↑/⇧↓", Some("Move by source")),
        ("h/l · ←/→", Some("Collapse / expand a source")),
        ("PgUp/PgDn", Some("Page through the list")),
        (
            "Tab / Shift+Tab",
            Some("Switch directory or host within the active scope"),
        ),
        (
            "Ctrl+H / Ctrl+L",
            Some("Cycle Library, User, Repo left / right"),
        ),
    ];
    entries.extend([
        ("Mouse", None),
        (
            "Row body",
            Some("Select and inspect; pending actions are never run"),
        ),
        (
            "Checkbox",
            Some("Toggle; Source disclosure and Skill Mode cells have their own actions"),
        ),
        (
            "Tabs / commands",
            Some("Click a visible tab or labeled command"),
        ),
        (
            "Modal choices",
            Some("Click a labeled choice; outside clicks do nothing"),
        ),
        (
            "Vertical wheel",
            Some("Move rows, chooser suggestions, or scrollable content"),
        ),
        (
            "Text selection",
            Some("Mouse capture may require a terminal-specific bypass gesture"),
        ),
        ("Text entry", Some("Always uses the keyboard")),
    ]);
    match model.workspace {
        Workspace::Target => entries.extend([
            ("Skills", None),
            ("Space", Some("Enable / disable")),
            ("m", Some("Cycle modes: link / copy / repo")),
            ("m on user", Some("Stage a repository link")),
            (
                "Space on override",
                Some("Remove override; keep user skill"),
            ),
            ("Skill tabs", None),
            ("Ctrl+T", Some("Add skill tab")),
            (
                "Type to filter",
                Some("Generic/Codex .agents/skills or Claude .claude/skills"),
            ),
            (
                "No matches",
                Some("Enter a custom path relative to User home or Repo root"),
            ),
            ("t", Some("Change repository")),
            ("a / e / d", Some("Add / edit / remove skill tab")),
            ("Commands", None),
            ("s", Some("Save")),
            ("Ctrl+S", Some("Save and exit")),
            ("u", Some("Undo unsaved changes")),
            ("q", Some("Quit; close overlays like Esc")),
        ]),
        Workspace::Library if model.host_index != 0 => entries.extend([
            ("Follower replica", None),
            (
                "s / Ctrl+S",
                Some("Confirm sync of this saved follower; do not save or exit"),
            ),
            ("Esc", Some("Cancel active sync (outside overlays)")),
            (
                "Leave host/scope",
                Some("Cancel sync; interrupted replicas may be partial"),
            ),
            (
                "Space / m / a/e/d",
                Some("Read-only; edit the leader Library on Local"),
            ),
            ("Commands", None),
            ("q", Some("Quit; close overlays like Esc")),
        ]),
        Workspace::Library => entries.extend([
            ("Library", None),
            (
                "Ctrl+T",
                Some("Verify follower SSH alias (credentials and host trust required)"),
            ),
            ("Space", Some("Show / hide in skill selection")),
            ("m", Some("Cycle modes: move / copy / link / none")),
            ("a / e / d", Some("Add / edit / remove folder")),
            ("r", Some("Refresh locations")),
            ("Commands", None),
            ("s", Some("Save")),
            ("Ctrl+S", Some("Save and exit")),
            ("u", Some("Undo unsaved changes")),
            ("q", Some("Quit; close overlays like Esc")),
        ]),
    }
    let entry_count = entries.len();
    let rows = entries.into_iter().map(|(key, action)| match action {
        Some(action) => TableRow::new(vec![
            Cell::from(Span::styled(
                key.to_owned(),
                Style::default().fg(BONE).add_modifier(Modifier::BOLD),
            )),
            Cell::from(action),
        ]),
        None => TableRow::new(vec![
            Cell::from(Span::styled(
                key.to_owned(),
                Style::default().fg(MODIFY).add_modifier(Modifier::BOLD),
            )),
            Cell::default(),
        ]),
    });
    let area = centered(frame.area(), 70, 72);
    frame.render_widget(Clear, area);
    let block = modal_controls_block("Help", &model.overlay, area, interactions);
    let inner = block.inner(area);
    let visible_rows = inner.height.saturating_sub(1);
    let max = entry_count
        .saturating_sub(usize::from(visible_rows))
        .min(usize::from(u16::MAX)) as u16;
    interactions.detail_scroll_max = Some(max);
    let scroll = scroll.min(max);
    if visible_rows > 0 {
        interactions.add_scroll(
            Rect::new(
                inner.x,
                inner.y.saturating_add(1),
                inner.width,
                visible_rows,
            ),
            Action::MoveUp,
            Action::MoveDown,
        );
    }
    let mut state = TableState::default().with_offset(usize::from(scroll));
    frame.render_stateful_widget(
        Table::new(rows, [Constraint::Length(18), Constraint::Min(1)])
            .header(
                TableRow::new(["Key", "Action"])
                    .style(Style::default().fg(BONE).add_modifier(Modifier::BOLD)),
            )
            .column_spacing(1)
            .block(block),
        area,
        &mut state,
    );
}

fn render_skill_details(frame: &mut Frame<'_>, model: &Model, interactions: &mut InteractionMap) {
    let Overlay::Details {
        title,
        path,
        document,
        warnings,
        errors,
    } = &model.overlay
    else {
        return;
    };
    let mut lines = vec![Line::from(Span::styled(
        path.to_owned(),
        Style::default().fg(DIM_FOREGROUND),
    ))];
    if !errors.is_empty() {
        lines.push(Line::from(Span::styled(
            "Errors (blocking)",
            Style::default().fg(ERROR).add_modifier(Modifier::BOLD),
        )));
        for error in errors {
            lines.push(Line::from(Span::styled(
                format!("  • {error}"),
                Style::default().fg(ERROR),
            )));
        }
    }
    if !warnings.is_empty() {
        lines.push(Line::from(Span::styled(
            "Warnings (advisory)",
            Style::default().fg(WARNING).add_modifier(Modifier::BOLD),
        )));
        for warning in warnings {
            lines.push(Line::from(Span::styled(
                format!("  • {warning}"),
                Style::default().fg(WARNING),
            )));
        }
    }
    if !errors.is_empty() || !warnings.is_empty() {
        lines.push(Line::raw(""));
        lines.push(Line::from(Span::styled(
            "SKILL.md (original document)",
            Style::default().fg(BONE).add_modifier(Modifier::BOLD),
        )));
    }
    if document.is_empty() {
        lines.push(Line::from("Cannot find or read this skill's SKILL.md."));
    } else {
        let mut frontmatter = false;
        let mut fenced_code = false;
        for (index, line) in document.lines().enumerate() {
            if index == 0 && line == "---" {
                frontmatter = true;
                lines.push(Line::raw(line.to_owned()));
            } else if frontmatter && line == "---" {
                frontmatter = false;
                lines.push(Line::raw(line.to_owned()));
            } else if frontmatter {
                if let Some((key, value)) = line.split_once(':') {
                    lines.push(Line::from(vec![
                        Span::styled(key.to_owned(), Style::default().fg(MODIFY)),
                        Span::raw(":"),
                        Span::raw(value.to_owned()),
                    ]));
                } else {
                    lines.push(Line::raw(line.to_owned()));
                }
            } else {
                lines.push(markdown_detail_line(line, &mut fenced_code));
            }
        }
    }
    let area = centered(frame.area(), 76, 70);
    frame.render_widget(Clear, area);
    let block = modal_controls_block(
        &format!("Skill details: {title}"),
        &model.overlay,
        area,
        interactions,
    );
    let inner = block.inner(area);
    let paragraph = Paragraph::new(Text::from(lines)).wrap(ratatui::widgets::Wrap { trim: true });
    let max = paragraph
        .line_count(inner.width)
        .saturating_sub(usize::from(inner.height))
        .min(usize::from(u16::MAX)) as u16;
    interactions.detail_scroll_max = Some(max);
    let scroll = model.detail_scroll.min(max);
    interactions.add_scroll(inner, Action::MoveUp, Action::MoveDown);
    frame.render_widget(paragraph.scroll((scroll, 0)).block(block), area);
}

pub(super) fn markdown_detail_line(line: &str, fenced_code: &mut bool) -> Line<'static> {
    let trimmed = line.trim_start();
    if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
        *fenced_code = !*fenced_code;
        return Line::from(Span::styled(line.to_owned(), Style::default().fg(ADD)));
    }
    if *fenced_code {
        return Line::from(Span::styled(line.to_owned(), Style::default().fg(ADD)));
    }
    if let Some((prefix, content)) = markdown_heading(line) {
        let mut spans = vec![Span::styled(prefix, Style::default().fg(MODIFY))];
        spans.extend(markdown_inline_spans(
            content,
            Style::default().fg(BONE).add_modifier(Modifier::BOLD),
        ));
        return Line::from(spans);
    }
    if let Some((prefix, content)) = markdown_quote(line) {
        let mut spans = vec![Span::styled(prefix, dim_style())];
        spans.extend(markdown_inline_spans(content, Style::default()));
        return Line::from(spans);
    }
    if let Some((prefix, content, numbered)) = markdown_bullet(line) {
        let marker_style = if numbered {
            Style::default().fg(DARK_MAGENTA)
        } else {
            dim_style()
        };
        let mut spans = vec![Span::styled(prefix, marker_style)];
        spans.extend(markdown_inline_spans(content, Style::default()));
        return Line::from(spans);
    }
    Line::from(markdown_inline_spans(line, Style::default()))
}

fn markdown_heading(line: &str) -> Option<(String, &str)> {
    let indent = line.len() - line.trim_start().len();
    let rest = &line[indent..];
    let hashes = rest.bytes().take_while(|byte| *byte == b'#').count();
    (hashes > 0 && rest.as_bytes().get(hashes) == Some(&b' ')).then(|| {
        (
            format!("{}{} ", &line[..indent], "#".repeat(hashes)),
            &rest[hashes + 1..],
        )
    })
}

fn markdown_quote(line: &str) -> Option<(String, &str)> {
    let indent = line.len() - line.trim_start().len();
    let rest = &line[indent..];
    rest.strip_prefix("> ")
        .map(|content| (format!("{}> ", &line[..indent]), content))
}

fn markdown_bullet(line: &str) -> Option<(String, &str, bool)> {
    let indent = line.len() - line.trim_start().len();
    let rest = &line[indent..];
    if let Some((marker, content)) = ["- ", "* ", "+ "]
        .iter()
        .find_map(|marker| rest.strip_prefix(marker).map(|content| (*marker, content)))
    {
        return Some((format!("{}{marker}", &line[..indent]), content, false));
    }
    let digits = rest
        .bytes()
        .take_while(|byte| byte.is_ascii_digit())
        .count();
    let marker_end = digits + 1;
    (digits > 0
        && matches!(rest.as_bytes().get(digits), Some(b'.' | b')'))
        && rest.as_bytes().get(marker_end) == Some(&b' '))
    .then(|| {
        (
            format!("{}{}", &line[..indent], &rest[..=marker_end]),
            &rest[marker_end + 1..],
            true,
        )
    })
}

pub(super) fn markdown_inline_spans(text: &str, normal: Style) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut remainder = text;
    while !remainder.is_empty() {
        if let Some(after_tick) = remainder.strip_prefix('`')
            && let Some(end) = after_tick.find('`')
        {
            let code = &after_tick[..end];
            spans.push(Span::styled(
                format!("`{code}`"),
                Style::default().fg(PURPLE),
            ));
            remainder = &after_tick[end + 1..];
            continue;
        }
        if let Some(after_open) = remainder.strip_prefix('[')
            && let Some(label_end) = after_open.find("](")
        {
            let label = &after_open[..label_end];
            let after_label = &after_open[label_end + 2..];
            if let Some(url_end) = after_label.find(')') {
                spans.push(Span::styled("[", dim_style()));
                spans.push(Span::styled(label.to_owned(), Style::default().fg(BLUE)));
                spans.push(Span::styled("](", dim_style()));
                spans.push(Span::styled(after_label[..url_end].to_owned(), dim_style()));
                spans.push(Span::styled(")", dim_style()));
                remainder = &after_label[url_end + 1..];
                continue;
            }
        }
        if let Some(after_bold) = remainder
            .strip_prefix("**")
            .or_else(|| remainder.strip_prefix("__"))
        {
            let delimiter = if remainder.starts_with("**") {
                "**"
            } else {
                "__"
            };
            if let Some(end) = after_bold.find(delimiter) {
                spans.push(Span::styled(delimiter, dim_style()));
                spans.push(Span::styled(
                    after_bold[..end].to_owned(),
                    Style::default()
                        .fg(Color::Indexed(15))
                        .add_modifier(Modifier::BOLD),
                ));
                spans.push(Span::styled(delimiter, dim_style()));
                remainder = &after_bold[end + delimiter.len()..];
                continue;
            }
        }
        if !remainder.starts_with("**")
            && !remainder.starts_with("__")
            && let Some(after_italic) = remainder
                .strip_prefix('*')
                .or_else(|| remainder.strip_prefix('_'))
        {
            let delimiter = if remainder.starts_with('*') { "*" } else { "_" };
            if let Some(end) = after_italic.find(delimiter) {
                spans.push(Span::styled(delimiter, dim_style()));
                spans.push(Span::styled(
                    after_italic[..end].to_owned(),
                    Style::default()
                        .fg(DIM_FOREGROUND)
                        .add_modifier(Modifier::ITALIC),
                ));
                spans.push(Span::styled(delimiter, dim_style()));
                remainder = &after_italic[end + delimiter.len()..];
                continue;
            }
        }
        let next = remainder
            .char_indices()
            .find_map(|(index, character)| {
                matches!(character, '`' | '[' | '*' | '_').then_some(index)
            })
            .filter(|index| *index > 0)
            .unwrap_or(remainder.len());
        spans.push(Span::styled(remainder[..next].to_owned(), normal));
        remainder = &remainder[next..];
    }
    spans
}

fn save_warning_modal(message: &str) -> (String, String, bool) {
    let body = confirmation_body(message);
    if message.starts_with("Initialize Skillator") {
        (
            "Initialize Skillator".to_owned(),
            remove_first_line(&body),
            true,
        )
    } else if message.starts_with("Save library changes?") {
        (
            "Save library changes".to_owned(),
            remove_question_from_first_line(&body),
            true,
        )
    } else {
        ("Confirm save".to_owned(), body, true)
    }
}

fn confirmation_body(message: &str) -> String {
    let mut lines = message.lines().collect::<Vec<_>>();
    while lines.last().is_some_and(|line| line.is_empty()) {
        lines.pop();
    }
    if lines
        .last()
        .is_some_and(|line| line.contains("Enter") || line.contains("Esc"))
    {
        lines.pop();
    }
    while lines.last().is_some_and(|line| line.is_empty()) {
        lines.pop();
    }
    lines.join("\n")
}

fn remove_first_line(text: &str) -> String {
    text.split_once('\n')
        .map_or_else(String::new, |(_, remainder)| remainder.to_owned())
}

fn remove_question_from_first_line(text: &str) -> String {
    let (first, remainder) = text.split_once('\n').unwrap_or((text, ""));
    let detail = first
        .split_once('?')
        .map_or("", |(_, detail)| detail.trim());
    [detail, remainder]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

pub(super) fn confirmation_text(text: &str) -> Text<'static> {
    let lines = text
        .lines()
        .map(|line| {
            if line.is_empty() {
                return Line::default();
            }
            if line.starts_with("• Needs confirmation")
                || contains_any(line, &["warning", "conflict"])
            {
                return Line::styled(line.to_owned(), Style::default().fg(WARNING));
            }
            if line.starts_with("• Cannot change")
                || contains_any(
                    line,
                    &[
                        "error",
                        "invalid",
                        "failed",
                        "recovery required",
                        "collision",
                    ],
                )
            {
                return Line::styled(line.to_owned(), Style::default().fg(ERROR));
            }
            Line::raw(line.to_owned())
        })
        .collect::<Vec<_>>();
    Text::from(lines)
}

fn overlay_text_style(overlay: &Overlay) -> Style {
    match overlay {
        Overlay::Help => Style::default().fg(BONE),
        Overlay::Result(message)
            if !contains_any(
                message,
                &[
                    "error",
                    "invalid",
                    "blocked",
                    "failed",
                    "recovery required",
                    "collision",
                ],
            ) =>
        {
            Style::default().fg(ADD)
        }
        Overlay::Notice(message) | Overlay::Result(message)
            if contains_any(
                message,
                &[
                    "error",
                    "invalid",
                    "blocked",
                    "failed",
                    "recovery required",
                    "collision",
                ],
            ) =>
        {
            Style::default().fg(ERROR)
        }
        Overlay::Notice(_) => Style::default().fg(WARNING),
        _ => Style::default(),
    }
}

fn modal_controls_block(
    title: &str,
    overlay: &Overlay,
    area: Rect,
    interactions: &mut InteractionMap,
) -> Block<'static> {
    let line = if area.height > 0 && area.width > 2 {
        controls_line(
            overlay_controls(overlay),
            area.width.saturating_sub(2),
            area.x.saturating_add(1),
            area.y.saturating_add(area.height.saturating_sub(1)),
            interactions,
        )
        .right_aligned()
    } else {
        Line::default()
    };
    modal_block(title).title_bottom(line)
}

fn modal_block(title: &str) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(BLUE))
        .title_style(Style::default().fg(BONE))
        .title(Line::styled(
            format!(" {title} "),
            Style::default().fg(BONE).add_modifier(Modifier::BOLD),
        ))
}

fn render_input(
    frame: &mut Frame<'_>,
    model: &Model,
    interactions: &mut InteractionMap,
    title: &str,
    hint: &str,
    input: &str,
) {
    let area = centered(frame.area(), 70, 30);
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(Text::from(vec![
            Line::raw(hint),
            Line::from(vec![
                Span::raw("> "),
                Span::raw(input),
                Span::styled("▌", Style::default().fg(BONE)),
            ]),
        ]))
        .style(Style::default().fg(BONE))
        .block(modal_controls_block(
            title,
            &model.overlay,
            area,
            interactions,
        )),
        area,
    );
}

fn render_filter(frame: &mut Frame<'_>, model: &Model, interactions: &mut InteractionMap) {
    let area = Rect::new(
        2,
        frame.area().height.saturating_sub(3),
        frame.area().width.saturating_sub(4),
        3,
    );
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(format!("/{}", model.filter))
            .style(Style::default().fg(BONE))
            .block(modal_controls_block(
                "Filter",
                &model.overlay,
                area,
                interactions,
            )),
        area,
    );
}

fn centered(area: Rect, percent_x: u16, percent_y: u16) -> Rect {
    let vertical = Layout::vertical([
        Constraint::Percentage((100 - percent_y) / 2),
        Constraint::Percentage(percent_y),
        Constraint::Percentage((100 - percent_y) / 2),
    ])
    .split(area);
    Layout::horizontal([
        Constraint::Percentage((100 - percent_x) / 2),
        Constraint::Percentage(percent_x),
        Constraint::Percentage((100 - percent_x) / 2),
    ])
    .split(vertical[1])[1]
}

fn render_directory_chooser(
    frame: &mut Frame<'_>,
    input: &str,
    selected: usize,
    model: &Model,
    interactions: &mut InteractionMap,
) {
    let area = centered(frame.area(), 76, 48);
    frame.render_widget(Clear, area);
    let matches = chooser_matches(input);
    let rows: Vec<_> = if matches.is_empty() {
        vec![Line::from(Span::styled(
            format!(" > Custom: {}", input.trim()),
            Style::default().fg(BONE),
        ))]
    } else {
        matches
            .iter()
            .enumerate()
            .map(|(index, (agent, path))| {
                let configured =
                    model
                        .directory_paths
                        .iter()
                        .enumerate()
                        .any(|(existing, candidate)| {
                            candidate == path
                                && model.directory_scopes.get(existing)
                                    == Some(&if model.scope == Scope::User {
                                        TargetTabScope::User
                                    } else {
                                        TargetTabScope::Repository
                                    })
                        });
                Line::from(Span::styled(
                    format!(
                        " {} {agent} · {}{}",
                        if index == selected { ">" } else { " " },
                        if model.scope == Scope::User {
                            format!("~/{path}")
                        } else {
                            path.to_string()
                        },
                        if configured {
                            " (already configured)"
                        } else {
                            ""
                        }
                    ),
                    if index == selected {
                        Style::default().fg(BONE).bg(SELECTED_BACKGROUND)
                    } else {
                        dim_style()
                    },
                ))
            })
            .collect()
    };
    let mut lines = vec![Line::from(format!(" Search: {input}▌")), Line::raw("")];
    lines.extend(rows);
    let block = modal_controls_block("Choose skill directory", &model.overlay, area, interactions);
    let inner = block.inner(area);
    let mut scroll = 0usize;
    if !matches.is_empty() && inner.height > 2 {
        let list_height = inner.height.saturating_sub(2);
        let start = selected
            .saturating_sub(usize::from(list_height.saturating_sub(1)))
            .min(matches.len().saturating_sub(usize::from(list_height)));
        scroll = start;
        let row_y = if start == 0 {
            inner.y.saturating_add(2)
        } else {
            inner.y
        };
        let row_height = inner.height.saturating_sub(row_y.saturating_sub(inner.y));
        interactions.add_scroll(
            Rect::new(inner.x, row_y, inner.width, row_height),
            Action::MoveUp,
            Action::MoveDown,
        );
        for row in 0..usize::from(row_height).min(matches.len().saturating_sub(start)) {
            interactions.add(
                Rect::new(inner.x, row_y.saturating_add(row as u16), inner.width, 1),
                Action::SelectSuggestion(start + row),
            );
        }
    }
    frame.render_widget(
        Paragraph::new(lines)
            .scroll((scroll.min(usize::from(u16::MAX)) as u16, 0))
            .block(block),
        area,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::model::Effect;

    #[test]
    fn narrow_overflow_arrow_advances_one_scoped_tab() {
        for workspace in [Workspace::Target, Workspace::Library] {
            let mut model = Model::new(workspace, Vec::new());
            let mut interactions = InteractionMap::default();
            interactions.reset(Rect::new(0, 0, 12, 1));
            if workspace == Workspace::Library {
                model.host_labels = vec!["a".into(), "b".into(), "c".into()];
                model.host_index = 0;
                model.scope = Scope::Library;
            } else {
                model.directory_labels = vec!["a".into(), "b".into(), "c".into()];
                model.directory_scopes = vec![
                    TargetTabScope::User,
                    TargetTabScope::User,
                    TargetTabScope::User,
                ];
                model.directory_index = 0;
                model.scope = Scope::User;
            }

            let area = Rect::new(0, 0, 12, 1);
            let _ = target_tabs_for_width(&model, 12, area, &mut interactions);
            assert_eq!(interactions.action_at(9, 0), Some(Action::NextDirectory));
            crate::tui::reducer::reduce(&mut model, Action::NextDirectory);

            if workspace == Workspace::Library {
                assert_eq!(model.host_index, 1);
            } else {
                assert_eq!(model.directory_index, 1);
            }
        }
    }
    fn rendered_control_position(
        terminal: &ratatui::Terminal<ratatui::backend::TestBackend>,
        text: &str,
    ) -> Option<(u16, u16)> {
        let buffer = terminal.backend().buffer();
        let chars: Vec<_> = text.chars().collect();
        if chars.len() > usize::from(buffer.area.width) {
            return None;
        }
        for y in 0..buffer.area.height {
            for x in 0..=buffer.area.width.saturating_sub(chars.len() as u16) {
                if chars.iter().enumerate().all(|(offset, character)| {
                    buffer[(x + offset as u16, y)].symbol() == character.to_string()
                }) {
                    return Some((x, y));
                }
            }
        }
        None
    }

    #[test]
    fn library_controls_follow_drawn_layout_and_hit_targets() {
        let controls_model = Model::new(Workspace::Library, Vec::new());
        for width in [140, 100, 80] {
            let mut row = Row::location("./library");
            row.location_index = Some(0);
            let model = Model::new(Workspace::Library, vec![row]);
            let mut terminal =
                ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, 24)).unwrap();
            let mut interactions = InteractionMap::default();
            terminal
                .draw(|frame| render(frame, &model, &mut interactions))
                .unwrap();

            let controls = workspace_controls(&controls_model);
            let mut visible_controls = Vec::new();
            let mut control_targets = Vec::new();
            for control in controls {
                let text = format!("{} {}", control.key, control.label);
                let position = rendered_control_position(&terminal, &text);
                let length = text.chars().count() as u16;
                if let Some((x, y)) = position {
                    let click_x = x + (length / 2);
                    assert_eq!(
                        interactions.action_at(click_x, y),
                        Some(control.action.clone()),
                        "wrong hit target for visible control {text:?} at width {width}"
                    );
                    visible_controls.push((control.action.clone(), x, y, length));
                }
                control_targets.push((control.action.clone(), position, length));
            }
            let buffer = terminal.backend().buffer();
            if let Some(y) = visible_controls.first().map(|entry| entry.2) {
                for (action, position, length) in control_targets {
                    for x in 0..buffer.area.width {
                        if interactions.action_at(x, y).as_ref() == Some(&action) {
                            assert!(
                                position.is_some_and(|(label_x, label_y)| {
                                    label_y == y && label_x <= x && x < label_x + length
                                }),
                                "incomplete control fragment has a hit target at width {width}"
                            );
                        }
                    }
                }
            }

            let edit_position = rendered_control_position(&terminal, "e edit");
            let save_position = rendered_control_position(&terminal, "s save");
            if width == 140 {
                assert!(edit_position.is_some(), "e edit must be fully visible");
                assert!(save_position.is_some(), "s save must be fully visible");
            }
            if let Some((x, y)) = edit_position {
                let mut edit_model = model.clone();
                let action = interactions.action_at(x + 1, y).unwrap();
                crate::tui::reducer::reduce(&mut edit_model, action);
                assert_eq!(
                    edit_model.overlay,
                    Overlay::LocationEditor {
                        edit: true,
                        input: "./library".to_owned()
                    }
                );
            }
            if let Some((x, y)) = save_position {
                let mut save_model = model.clone();
                let action = interactions.action_at(x + 1, y).unwrap();
                assert_eq!(
                    crate::tui::reducer::reduce(&mut save_model, action),
                    [Effect::PrepareSave { fast: false }]
                );
            }
        }
    }
}
