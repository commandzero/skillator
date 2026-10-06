//! Shared terminal rendering, palettes and read-only inspectors.

use super::input::chooser_matches;
use super::model::{
    CheckState, Model, Overlay, Row, RowKind, Scope, TargetTabScope, Workspace, row_identity,
};
use crate::domain::MaterializationKind;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
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

fn footer_help(model: &Model) -> Line<'static> {
    let entries: &[(&str, &str)] = match model.workspace {
        Workspace::Target => &[
            ("s", "save"),
            ("Ctrl+S", "save & exit"),
            ("u", "undo"),
            ("Space", "toggle"),
            ("m", "mode"),
            ("/", "filter"),
            ("Ctrl+T", "new tab"),
            ("?", "help"),
        ],
        Workspace::Library if model.host_index != 0 => &[
            ("s", "sync"),
            ("Ctrl+S", "sync"),
            ("/", "filter"),
            ("Esc", "cancel sync"),
            ("?", "help"),
        ],
        Workspace::Library => &[
            ("s", "save"),
            ("Ctrl+S", "save & exit"),
            ("u", "undo"),
            ("Space", "show/hide"),
            ("a/e/d", "location"),
            ("m", "mode"),
            ("/", "filter"),
            ("Ctrl+T", "follower"),
            ("r", "refresh"),
            ("?", "help"),
        ],
    };
    let mut spans = vec![Span::raw(" ")];
    for (index, (key, description)) in entries.iter().enumerate() {
        if index > 0 {
            spans.push(dim_span(" · "));
        }
        spans.push(Span::styled(
            (*key).to_owned(),
            Style::default().fg(BONE).add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::raw(format!(" {description}")));
    }
    spans.push(Span::raw(" "));
    Line::from(spans)
}

pub fn render(frame: &mut Frame<'_>, model: &Model) {
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
        Paragraph::new(target_tabs_for_width(model, areas[1].width)),
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
    let key_help = footer_help(model).right_aligned();
    let border_color = model.scope.color();
    let mut table_state = TableState::default();
    table_state.select(visible.iter().position(|index| *index == model.selected));
    frame.render_stateful_widget(
        Table::new(rows, widths).header(header).block(
            Block::default()
                .borders(Borders::ALL)
                .border_set(TAB_TOP_BORDER)
                .border_style(Style::default().fg(border_color))
                .title_bottom(key_help),
        ),
        areas[2],
        &mut table_state,
    );
    frame.render_widget(Paragraph::new(status_line(model, areas[3].width)), areas[3]);
    render_overlay(frame, model);
}

fn target_tabs_for_width(model: &Model, width: u16) -> Line<'static> {
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
    for (position, (index, label)) in labels.into_iter().enumerate().skip(first) {
        let text = format!(" {label} ");
        let required = Line::from(text.as_str()).width() + 1;
        if position != first && used + required > available {
            if used < available {
                spans.push(Span::styled("›", dim_style()));
            }
            break;
        }
        let style = if index == active {
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

pub(super) fn render_overlay(frame: &mut Frame<'_>, model: &Model) {
    let (title, body, footer, confirmation) = match &model.overlay {
        Overlay::None => return,
        Overlay::Welcome => (
            "I AM SKILLATOR!".to_owned(),
            "Your Library starts at ./library. Press e to edit this location. After closing this welcome, Ctrl+H/Ctrl+L move between Library, User and Repo.".to_owned(),
            Some("Enter continue · Esc exit".to_owned()),
            false,
        ),
        Overlay::Help => return render_help(frame, model, model.detail_scroll),
        Overlay::Filter => return render_filter(frame, &model.filter),
        Overlay::ConfirmSave => (
            "Save changes".to_owned(),
            "Save your changes?".to_owned(),
            Some("y/Enter save · n/Esc return".to_owned()),
            true,
        ),
        Overlay::ConfirmSaveWarning(message) => save_warning_modal(message),
        Overlay::GuardedConfirmation(message) => {
            let (body, footer) = split_confirmation_message(message);
            (
                "Review changes".to_owned(),
                remove_first_line(&body),
                Some(footer),
                true,
            )
        }
        Overlay::DiscardTarget => (
            "Discard pending changes".to_owned(),
            "Discard unsaved changes before opening another repository?".to_owned(),
            Some("y/Enter discard · n/Esc return".to_owned()),
            true,
        ),
        Overlay::ScopeSwitch { .. } => (
            "Switch scope".to_owned(),
            "This scope has unsaved changes.".to_owned(),
            Some("Enter save and switch · d discard and switch · Esc return".to_owned()),
            true,
        ),
        Overlay::DirectoryChooser { input, selected } => {
            return render_directory_chooser(frame, input, *selected, model);
        }
        Overlay::FollowerEditor(input) => {
            return render_input(frame, "Follower name",
                "SSH alias from ~/.ssh/config · existing credentials and host trust required", input);
        }
        Overlay::FollowerProbe { name } => (
            "Connecting to follower".to_owned(),
            format!("Checking SSH alias {name} (read-only hostname probe). Esc cancels."),
            Some("Esc cancel".to_owned()),
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
            Some("y/Enter sync · n/Esc skip".to_owned()),
            true,
        ),
        Overlay::DirectoryEditor { edit, input } => {
            let mode = if *edit { "Edit" } else { "New" };
            return render_input(
                frame,
                &format!("{mode} skill tab"),
                "agents | .claude | key,path,label",
                input,
            );
        }
        Overlay::LocationEditor { edit, input } => {
            let mode = if *edit { "Edit" } else { "Add" };
            return render_input(
                frame,
                &format!("{mode} library folder"),
                "path",
                input,
            );
        }
        Overlay::SourceKeyEditor(input) => {
            return render_input(
                frame,
                "Choose a unique source name",
                "owner/repository",
                input,
            );
        }
        Overlay::TargetPicker(input) => {
            return render_input(frame, "Change repository", "directory", input);
        }
        Overlay::ConfirmDelete => (
            "Remove from configuration".to_owned(),
            "Remove the selected folder from the configuration? This does not delete the folder or its files.".to_owned(),
            Some("y/Enter delete · n/Esc return".to_owned()),
            true,
        ),
        Overlay::Busy => (
            "Another Skillator process is saving changes".to_owned(),
            "Another Skillator process is saving changes here. Wait for it to finish, then retry.".to_owned(),
            Some("Enter retry · Esc return to editing".to_owned()),
            true,
        ),
        Overlay::Details {
            title,
            path,
            document,
            warnings,
            errors,
        } => {
            return render_skill_details(frame, title, path, document, warnings, errors, model.detail_scroll);
        }
        Overlay::Diagnostic { title, body } | Overlay::FollowerSyncResult { title, body } => {
            let area = centered(frame.area(), 76, 70);
            frame.render_widget(Clear, area);
            frame.render_widget(
                Paragraph::new(body.as_str())
                    .wrap(ratatui::widgets::Wrap { trim: true })
                    .scroll((model.detail_scroll, 0))
                    .block(modal_block(title, Some("j/k scroll · PgUp/PgDn page · Enter/Esc close"))),
                area,
            );
            return;
        }
        Overlay::Notice(_) => return,
        Overlay::Result(message) => (
            "Save result".to_owned(),
            message.to_owned(),
            Some("Enter exit".to_owned()),
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
            .block(modal_block(&title, footer.as_deref())),
        area,
    );
}

fn render_help(frame: &mut Frame<'_>, model: &Model, scroll: u16) {
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
    let mut state = TableState::default().with_offset(usize::from(scroll));
    frame.render_stateful_widget(
        Table::new(rows, [Constraint::Length(18), Constraint::Min(1)])
            .header(
                TableRow::new(["Key", "Action"])
                    .style(Style::default().fg(BONE).add_modifier(Modifier::BOLD)),
            )
            .column_spacing(1)
            .block(modal_block(
                "Help",
                Some("j/k scroll · PgUp/PgDn page · q/Esc close"),
            )),
        area,
        &mut state,
    );
}

fn render_skill_details(
    frame: &mut Frame<'_>,
    title: &str,
    path: &str,
    document: &str,
    warnings: &[String],
    errors: &[String],
    scroll: u16,
) {
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
    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .wrap(ratatui::widgets::Wrap { trim: true })
            .scroll((scroll, 0))
            .block(modal_block(
                &format!("Skill details: {title}"),
                Some("j/k or ↑/↓ scroll · PgUp/PgDn page · Enter/Esc close"),
            )),
        area,
    );
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

fn save_warning_modal(message: &str) -> (String, String, Option<String>, bool) {
    let (body, footer) = split_confirmation_message(message);
    if message.starts_with("Initialize Skillator") {
        (
            "Initialize Skillator".to_owned(),
            remove_first_line(&body),
            Some(footer),
            true,
        )
    } else if message.starts_with("Save library changes?") {
        (
            "Save library changes".to_owned(),
            remove_question_from_first_line(&body),
            Some(footer),
            true,
        )
    } else {
        ("Confirm save".to_owned(), body, Some(footer), true)
    }
}

fn split_confirmation_message(message: &str) -> (String, String) {
    let mut lines = message.lines().collect::<Vec<_>>();
    while lines.last().is_some_and(|line| line.is_empty()) {
        lines.pop();
    }
    let footer = if lines
        .last()
        .is_some_and(|line| line.contains("Enter") || line.contains("Esc"))
    {
        lines.pop().unwrap_or_default().to_owned()
    } else {
        String::new()
    };
    while lines.last().is_some_and(|line| line.is_empty()) {
        lines.pop();
    }
    (lines.join("\n"), footer)
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

pub(super) fn confirmation_prompt_line(line: &str) -> Line<'static> {
    let mut spans = Vec::new();
    for (index, segment) in line.split(" · ").enumerate() {
        if index > 0 {
            spans.push(Span::styled(" · ", Style::default().fg(DIM_FOREGROUND)));
        }
        let (key, description) = segment.split_once(' ').unwrap_or((segment, ""));
        spans.push(Span::styled(
            key.to_owned(),
            Style::default().fg(BONE).add_modifier(Modifier::BOLD),
        ));
        if !description.is_empty() {
            spans.push(Span::styled(
                format!(" {description}"),
                Style::default().fg(DIM_FOREGROUND),
            ));
        }
    }
    Line::from(spans)
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

fn modal_block(title: &str, footer: Option<&str>) -> Block<'static> {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(BLUE))
        .title_style(Style::default().fg(BONE))
        .title(Line::styled(
            format!(" {title} "),
            Style::default().fg(BONE).add_modifier(Modifier::BOLD),
        ));
    match footer {
        Some(footer) => block.title_bottom(confirmation_prompt_line(footer).right_aligned()),
        None => block,
    }
}

fn render_input(frame: &mut Frame<'_>, title: &str, hint: &str, input: &str) {
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
        .block(modal_block(
            title,
            Some("Tab complete · Enter apply · Esc cancel"),
        )),
        area,
    );
}

fn render_filter(frame: &mut Frame<'_>, filter: &str) {
    let area = Rect::new(
        2,
        frame.area().height.saturating_sub(3),
        frame.area().width.saturating_sub(4),
        3,
    );
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(format!("/{filter}"))
            .style(Style::default().fg(BONE))
            .block(modal_block("Filter", Some("Enter apply · Esc clear"))),
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

fn render_directory_chooser(frame: &mut Frame<'_>, input: &str, selected: usize, model: &Model) {
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
    frame.render_widget(
        Paragraph::new(lines).block(modal_block(
            "Choose skill directory",
            Some("↑/↓ select · Enter stage · Esc cancel"),
        )),
        area,
    );
}
