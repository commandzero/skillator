//! Behavior coverage for scope workflows and shared interaction.
use super::input::*;
use super::library::*;
use super::model::*;
use super::reducer::*;
use super::render::*;
use super::target::*;
use super::*;
use crate::acquisition::LibraryAcquisitionMode;
use crate::app::{AppPaths, LibraryWorkflow, TargetWorkflow, UserScopeWorkflow};
use crate::config::{LibraryConfig, LibraryLocationConfig, RepositoryConfig, SkillDirectoryConfig};
use crate::domain::MaterializationKind;
use crate::remote::hosts::{self, HostRegistry, ReplicaInventory};
use crate::target::{Target, observe};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::style::{Color, Modifier, Style};
use std::collections::BTreeSet;
#[test]
fn inherited_override_modes_and_cancellation_remain_staged() {
    let row = Row::inherited_user("local/library", "demo", "Demo", true, "User account");
    let mut model = Model::new(Workspace::Target, vec![row.clone()]);

    reduce(&mut model, Action::SwitchMode);
    assert_eq!(model.rows[0].check, Some(CheckState::Checked));
    assert_eq!(model.rows[0].mode, Some(MaterializationKind::Linked));
    assert_eq!(model.rows[0].action, "Enable link");
    assert_eq!(reduce(&mut model, Action::Undo), vec![Effect::Undo]);

    reduce(&mut model, Action::SwitchMode);
    assert_eq!(model.rows[0].mode, Some(MaterializationKind::Copied));
    assert_eq!(model.rows[0].action, "Enable copy");
    reduce(&mut model, Action::SwitchMode);
    assert_eq!(model.rows[0].mode, Some(MaterializationKind::Linked));

    reduce(&mut model, Action::Toggle);
    assert_eq!(model.rows[0], row);
}

#[test]
fn unavailable_inherited_skill_explains_why_linking_is_blocked() {
    let row = Row::inherited_user("missing/source", "demo", "Demo", false, "User account");
    let mut model = Model::new(Workspace::Target, vec![row.clone()]);
    reduce(&mut model, Action::SwitchMode);
    assert_eq!(model.rows[0], row);
    assert!(!model.dirty);
    assert!(
        matches!(model.overlay, Overlay::Notice(ref message) if message.contains("not available"))
    );
}

#[cfg(unix)]
#[test]
fn inherited_override_save_reload_remove_and_conflict_preserve_user_scope() {
    let home = tempfile::tempdir().unwrap();
    let paths = AppPaths::new(home.path().to_owned());
    let skill = home.path().join(".skillator/library/demo");
    std::fs::create_dir_all(&skill).unwrap();
    let document = "---\nname: demo\ndescription: Demo skill\n---\n";
    std::fs::write(skill.join("SKILL.md"), document).unwrap();
    let session = LibraryWorkflow::load(&paths).unwrap();
    LibraryWorkflow::save(&paths, &session, &session.config, true).unwrap();
    let selector = crate::app::SkillSelector::parse("local/library:demo").unwrap();
    let report = UserScopeWorkflow::mutate_enablement(
        &paths,
        &selector,
        Some(MaterializationKind::Copied),
        crate::app::SyncMode::Apply { force: false },
    )
    .unwrap();
    assert_eq!(report.exit_status, 0);
    let user_bytes = std::fs::read(paths.user_config()).unwrap();
    let user_skill = home.path().join(".agents/skills/demo");
    let repository = home.path().join("project");
    std::fs::create_dir(&repository).unwrap();
    assert!(
        std::process::Command::new("git")
            .args(["init", "-q"])
            .arg(&repository)
            .status()
            .unwrap()
            .success()
    );

    let load = || {
        let library_config = LibraryWorkflow::load(&paths).unwrap().config;
        let library = LibraryWorkflow::snapshot(&paths, &library_config);
        build_target_state(
            UserScopeWorkflow::load(&paths).unwrap(),
            TargetWorkflow::load(&repository).unwrap(),
            &library,
            &library_config,
        )
    };
    let select = |state: &LoadedTargetState| {
        let mut model = initial_target_model(&state.tabs);
        model.selected = model
            .rows
            .iter()
            .position(|row| row.is_skill() && row.name == "demo")
            .unwrap();
        model
    };
    let mut state = load();
    let mut model = select(&state);
    assert_eq!(model.rows[model.selected].check, Some(CheckState::User));
    reduce(&mut model, Action::SwitchMode);
    assert!(!repository.join(".agents/skillator.yaml").exists());
    // Discard rebuilds from saved state, without a repository declaration.
    let discarded = select(&load());
    assert_eq!(
        discarded.rows[discarded.selected].check,
        Some(CheckState::User)
    );
    store_active_target_tab(&model, &mut state.tabs);
    let prepared =
        prepare_scope_save(&paths, &state, &state.tabs, TargetTabScope::Repository).unwrap();
    let report = prepared.commit(&paths).unwrap();
    assert_eq!(report.exit_status, 0);
    let destination = repository.join(".agents/skills/demo");
    assert!(
        destination
            .symlink_metadata()
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(
        destination.canonicalize().unwrap(),
        skill.canonicalize().unwrap()
    );

    let mut state = load();
    let mut model = select(&state);
    assert_eq!(model.rows[model.selected].check, Some(CheckState::Checked));
    assert_eq!(
        model.rows[model.selected].mode,
        Some(MaterializationKind::Linked)
    );
    assert!(
        model.rows[model.selected]
            .details
            .contains("also enabled for your user account")
    );
    reduce(&mut model, Action::Toggle);
    assert_eq!(model.rows[model.selected].check, Some(CheckState::User));
    assert_eq!(model.rows[model.selected].action, "Disable");
    store_active_target_tab(&model, &mut state.tabs);
    let prepared =
        prepare_scope_save(&paths, &state, &state.tabs, TargetTabScope::Repository).unwrap();
    assert_eq!(prepared.commit(&paths).unwrap().exit_status, 0);
    assert!(destination.symlink_metadata().is_err());
    assert!(load().repository.config.enablements().is_empty());

    // Canceling a new override produces no declaration or materialization.
    let mut state = load();
    let mut model = select(&state);
    reduce(&mut model, Action::SwitchMode);
    reduce(&mut model, Action::Toggle);
    store_active_target_tab(&model, &mut state.tabs);
    assert!(
        scope_config(&state.tabs, TargetTabScope::Repository)
            .unwrap()
            .enablements()
            .is_empty()
    );
    let prepared =
        prepare_scope_save(&paths, &state, &state.tabs, TargetTabScope::Repository).unwrap();
    assert_eq!(prepared.commit(&paths).unwrap().exit_status, 0);
    assert!(destination.symlink_metadata().is_err());

    std::fs::create_dir(&destination).unwrap();
    std::fs::write(destination.join("keep.txt"), "unmanaged").unwrap();
    let mut state = load();
    let mut model = select(&state);
    reduce(&mut model, Action::SwitchMode);
    store_active_target_tab(&model, &mut state.tabs);
    let prepared =
        prepare_scope_save(&paths, &state, &state.tabs, TargetTabScope::Repository).unwrap();
    assert!(
        prepared
            .plan()
            .items()
            .iter()
            .any(|item| item.safety() == crate::reconcile::Safety::Guarded)
    );
    model.overlay = save_review_overlay(prepared.plan());
    assert!(matches!(model.overlay, Overlay::GuardedConfirmation(_)));
    assert_eq!(reduce(&mut model, Action::Escape), vec![Effect::CancelSave]);
    drop(prepared);
    assert_eq!(
        std::fs::read_to_string(destination.join("keep.txt")).unwrap(),
        "unmanaged"
    );
    assert!(load().repository.config.enablements().is_empty());
    let prepared =
        prepare_scope_save(&paths, &state, &state.tabs, TargetTabScope::Repository).unwrap();
    model.overlay = save_review_overlay(prepared.plan());
    assert_eq!(
        reduce(&mut model, Action::Confirm),
        vec![Effect::CommitSave]
    );
    assert_eq!(prepared.commit(&paths).unwrap().exit_status, 0);
    assert_eq!(
        destination.canonicalize().unwrap(),
        skill.canonicalize().unwrap()
    );
    assert_eq!(std::fs::read(paths.user_config()).unwrap(), user_bytes);
    assert!(
        !user_skill
            .symlink_metadata()
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(
        std::fs::read_to_string(user_skill.join("SKILL.md")).unwrap(),
        document
    );
}

fn inventory_skill(inventory_id: &str, path: &str, check: CheckState) -> Row {
    Row::skill_inventory(SkillInventoryRow {
        group: "acme/skills".to_owned(),
        inventory_id: Some(inventory_id.to_owned()),
        path: path.to_owned(),
        name: path.to_owned(),
        description: String::new(),
        check,
        available: true,
        valid: true,
        mode: None,
        state: String::new(),
        details: String::new(),
        location_index: Some(0),
    })
}

#[test]
fn adding_a_library_location_preserves_checks_and_leaves_new_skills_unchecked() {
    let mut staged_checked = inventory_skill("0:existing", "checked", CheckState::Checked);
    staged_checked.initial_check = Some(CheckState::Unchecked);
    let staged_unchecked = inventory_skill("0:existing", "unchecked", CheckState::Unchecked);
    let mut staged_invalid = inventory_skill("1:added", "repaired", CheckState::Invalid);
    staged_invalid.valid = false;
    let previous = vec![staged_checked, staged_unchecked, staged_invalid];
    let previous_checks = library_skill_checks(&previous);
    let source = Row::source_inventory(
        "acme/skills".to_owned(),
        CheckState::Checked,
        0,
        "existing".to_owned(),
        true,
        false,
    );
    let mut invalid = inventory_skill("1:added", "invalid", CheckState::Invalid);
    invalid.valid = false;
    let mut rows = vec![
        source,
        inventory_skill("0:existing", "checked", CheckState::Checked),
        inventory_skill("0:existing", "unchecked", CheckState::Checked),
        inventory_skill("1:added", "new", CheckState::Checked),
        inventory_skill("1:added", "repaired", CheckState::Checked),
        invalid,
    ];

    preserve_library_checks_after_location_add(&mut rows, &previous_checks);

    let skills = rows
        .iter()
        .filter(|row| row.kind == RowKind::Skill)
        .collect::<Vec<_>>();
    assert_eq!(skills[0].check, Some(CheckState::Checked));
    assert_eq!(skills[0].initial_check, Some(CheckState::Unchecked));
    assert_eq!(skills[1].check, Some(CheckState::Unchecked));
    assert_eq!(skills[1].initial_check, Some(CheckState::Unchecked));
    assert_eq!(skills[2].check, Some(CheckState::Unchecked));
    assert_eq!(skills[2].initial_check, Some(CheckState::Unchecked));
    assert_eq!(skills[3].check, Some(CheckState::Unchecked));
    assert_eq!(skills[3].initial_check, Some(CheckState::Unchecked));
    assert_eq!(skills[4].check, Some(CheckState::Invalid));
    assert_eq!(skills[4].initial_check, Some(CheckState::Invalid));
}

#[test]
fn occupied_ignore_path_review_explains_replacement_and_preserves_the_folder() {
    let home = tempfile::tempdir().unwrap();
    let repository = home.path().join("repo");
    std::fs::create_dir_all(repository.join(".agents/skills")).unwrap();
    let git = std::process::Command::new("git")
        .args(["init", "--quiet"])
        .arg(&repository)
        .output()
        .unwrap();
    assert!(git.status.success());
    let ignore = repository.join(".agents/.gitignore");
    std::fs::create_dir(&ignore).unwrap();
    std::fs::write(ignore.join("custom-rule"), "keep me\n").unwrap();
    let paths = AppPaths::new(home.path().to_owned());
    let session = TargetWorkflow::load(&repository).unwrap();
    let prepared = TargetWorkflow::prepare_save(&paths, &session, session.config.clone()).unwrap();
    let overlay = save_review_overlay(prepared.plan());
    let Overlay::GuardedConfirmation(ref message) = overlay else {
        panic!("expected a confirmation prompt");
    };
    assert!(message.contains("Needs confirmation"), "{message}");
    assert!(message.contains("Saving will replace it"), "{message}");
    assert!(!message.to_lowercase().contains("guarded"), "{message}");
    assert!(!message.contains("canonical"), "{message}");
    assert_eq!(
        std::fs::read_to_string(ignore.join("custom-rule")).unwrap(),
        "keep me\n"
    );
    let mut model = Model::new(Workspace::Target, Vec::new());
    model.overlay = overlay;
    let backend = ratatui::backend::TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| render_overlay(frame, &model))
        .unwrap();
    let buffer = terminal.backend().buffer();
    let screen = (0..24)
        .map(|y| (0..80).map(|x| buffer[(x, y)].symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(screen.contains("ignore file"), "{screen}");
    assert!(screen.contains("confirm and save"), "{screen}");
}

#[test]
fn initial_target_tab_prefers_the_first_repository_tab() {
    let tabs = vec![
        TargetTab {
            scope: TargetTabScope::User,
            directory: SkillDirectoryConfig::agents_preset(),
            rows: Vec::new(),
        },
        TargetTab {
            scope: TargetTabScope::Repository,
            directory: SkillDirectoryConfig::claude_preset(),
            rows: Vec::new(),
        },
    ];

    assert_eq!(initial_target_tab_index(&tabs), 1);
    assert_eq!(initial_target_tab_index(&tabs[..1]), 0);
}

#[test]
fn target_title_uses_a_home_relative_repository_path() {
    assert_eq!(
        user_relative_path(
            Path::new("/Users/ada/Development/skillator"),
            Path::new("/Users/ada")
        ),
        "~/Development/skillator"
    );
    assert_eq!(
        user_relative_path(Path::new("/opt/work/skillator"), Path::new("/Users/ada")),
        "/opt/work/skillator"
    );
}

#[test]
fn skill_detail_markdown_uses_lightweight_structural_highlighting() {
    let inline = markdown_inline_spans(
        "Run `skillator sync` with [the guide](https://example.test).",
        Style::default(),
    );
    assert!(
        inline
            .iter()
            .any(|span| { span.content == "`skillator sync`" && span.style.fg == Some(PURPLE) })
    );
    assert!(
        inline
            .iter()
            .any(|span| span.content == "the guide" && span.style.fg == Some(BLUE))
    );
    assert!(inline.iter().any(|span| {
        span.content == "https://example.test" && span.style.fg == Some(DIM_FOREGROUND)
    }));
    let emphasis = markdown_inline_spans("**important** and *optional*", Style::default());
    assert!(emphasis.iter().any(|span| {
        span.content == "important"
            && span.style.fg == Some(Color::Indexed(15))
            && span.style.add_modifier.contains(Modifier::BOLD)
    }));
    assert!(emphasis.iter().any(|span| {
        span.content == "optional"
            && span.style.fg == Some(DIM_FOREGROUND)
            && span.style.add_modifier.contains(Modifier::ITALIC)
    }));
    let mut un_fenced = false;
    let numbered = markdown_detail_line("12. Prepare release", &mut un_fenced);
    assert_eq!(numbered.spans[0].content, "12. ");
    assert_eq!(numbered.spans[0].style.fg, Some(DARK_MAGENTA));

    let mut fenced = false;
    let fence = markdown_detail_line("```sh", &mut fenced);
    assert!(fenced);
    assert_eq!(fence.spans[0].style.fg, Some(ADD));
    let code = markdown_detail_line("skillator sync", &mut fenced);
    assert_eq!(code.spans[0].style.fg, Some(ADD));
    let heading = markdown_detail_line("## Setup", &mut fenced);
    assert_eq!(heading.spans[0].style.fg, Some(ADD));
    markdown_detail_line("```", &mut fenced);
    let heading = markdown_detail_line("## Setup", &mut fenced);
    assert_eq!(heading.spans[0].style.fg, Some(MODIFY));
    assert!(heading.spans[1].style.add_modifier.contains(Modifier::BOLD));
}

#[test]
fn scope_paths_appear_in_status_not_header() {
    let tabs = vec![
        TargetTab {
            scope: TargetTabScope::User,
            directory: SkillDirectoryConfig::user_preset(),
            rows: Vec::new(),
        },
        TargetTab {
            scope: TargetTabScope::Repository,
            directory: SkillDirectoryConfig::agents_preset(),
            rows: Vec::new(),
        },
    ];
    let mut model = initial_target_model(&tabs);
    model.target_path = Some("~/Development/project".to_owned());
    let backend = ratatui::backend::TestBackend::new(100, 12);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();

    terminal.draw(|frame| render(frame, &model)).unwrap();
    let repository_screen = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(repository_screen.contains("Repo: ~/Development/project · .agents/skills"));

    model.scope = Scope::User;
    activate_target_tab(&mut model, &tabs, &BTreeSet::new(), 0);
    terminal.draw(|frame| render(frame, &model)).unwrap();
    let user_screen = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(user_screen.contains("User: ~/.agents/skills"));
    assert!(!user_screen.contains("~/Development/project"));
}

#[test]
fn editor_keys_accept_literal_paths_and_complete_directories() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir(directory.path().join("library")).unwrap();
    let input = format!("{}/lib", directory.path().display());
    let expected = format!("{}/library/", directory.path().display());
    let mut model = Model::new(Workspace::Library, Vec::new());
    model.overlay = Overlay::LocationEditor { edit: false, input };

    assert_eq!(
        action_for_model_key(
            &model,
            KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE)
        ),
        Some(Action::Input('/'))
    );
    assert_eq!(
        action_for_model_key(
            &model,
            KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE)
        ),
        Some(Action::Input('q'))
    );
    assert_eq!(
        action_for_model_key(&model, KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)),
        Some(Action::CompletePath)
    );

    reduce(&mut model, Action::CompletePath);
    assert_eq!(
        model.overlay,
        Overlay::LocationEditor {
            edit: false,
            input: expected,
        }
    );
}

#[test]
fn row_selection_uses_a_light_foreground_on_the_selection_background() {
    let warning = Row::diagnostic("Conflict needs attention");
    let warning_style = row_style(&warning, true);
    assert_eq!(warning_style.fg, Some(Color::Indexed(15)));
    assert_eq!(warning_style.bg, Some(SELECTED_BACKGROUND));

    let mut error = Row::diagnostic("Cannot continue");
    error.state = "Invalid".to_owned();
    let error_style = row_style(&error, true);
    assert_eq!(error_style.fg, Some(Color::Indexed(15)));
    assert_eq!(error_style.bg, Some(SELECTED_BACKGROUND));

    let normal_style = row_style(&Row::location("./library"), true);
    assert_eq!(normal_style.fg, Some(Color::Indexed(15)));
    assert_eq!(normal_style.bg, Some(SELECTED_BACKGROUND));
}

#[test]
fn pending_actions_use_git_style_semantic_accents() {
    let mut added = Row::location("./library");
    added.action = "Enable link".to_owned();
    assert_eq!(row_style(&added, false).fg, Some(ADD));

    let mut removed = Row::location("./library");
    removed.action = "Unregister Source".to_owned();
    assert_eq!(row_style(&removed, false).fg, Some(ERROR));

    let mut modified = Row::location("./library");
    modified.action = "Move to Library".to_owned();
    assert_eq!(row_style(&modified, false).fg, Some(MODIFY));
}

#[test]
fn disabling_a_newly_enabled_skill_clears_its_temporary_mode() {
    let mut skill = Row::skill(
        "local/library",
        "release-checklist",
        "Prepare a release",
        false,
        true,
        MaterializationKind::Linked,
        "Missing",
    );
    skill.mode = None;
    skill.initial_mode = None;
    let mut model = Model::new(Workspace::Target, vec![skill]);

    reduce(&mut model, Action::Toggle);
    assert_eq!(model.rows[0].check, Some(CheckState::Checked));
    assert_eq!(model.rows[0].mode, Some(MaterializationKind::Linked));
    assert_eq!(model.rows[0].action, "Enable link");

    reduce(&mut model, Action::Toggle);
    assert_eq!(model.rows[0].check, Some(CheckState::Unchecked));
    assert_eq!(model.rows[0].mode, None);
    assert!(model.rows[0].action.is_empty());
}

#[test]
fn re_enabling_an_existing_skill_restores_its_initial_mode() {
    let skill = Row::skill(
        "local/library",
        "release-checklist",
        "Prepare a release",
        true,
        true,
        MaterializationKind::Copied,
        "In Sync",
    );
    let mut model = Model::new(Workspace::Target, vec![skill]);

    reduce(&mut model, Action::Toggle);
    assert_eq!(model.rows[0].check, Some(CheckState::Unchecked));
    assert_eq!(model.rows[0].mode, None);
    assert_eq!(model.rows[0].action, "Disable");

    reduce(&mut model, Action::Toggle);
    assert_eq!(model.rows[0].check, Some(CheckState::Checked));
    assert_eq!(model.rows[0].mode, Some(MaterializationKind::Copied));
    assert!(model.rows[0].action.is_empty());
}

#[test]
fn confirmation_distinguishes_desired_actions_from_warnings() {
    let text = confirmation_text(
        "• Move to Library: source → destination\n\
         • Needs confirmation: existing path\n\
         • Cannot change: tracked path",
    );

    assert_eq!(text.lines[0].style.fg, None);
    assert_eq!(text.lines[1].style.fg, Some(WARNING));
    assert_eq!(text.lines[2].style.fg, Some(ERROR));

    let footer = confirmation_prompt_line("y/Enter initialize · n/Esc return");
    assert!(footer.spans.iter().any(|span| span.style.fg == Some(BONE)));
    assert!(
        footer
            .spans
            .iter()
            .any(|span| span.style.fg == Some(DIM_FOREGROUND))
    );
}

#[test]
fn first_run_opens_the_normal_library_with_a_welcome() {
    let home = tempfile::tempdir().unwrap();
    let paths = AppPaths::new(home.path().to_owned());
    let session = LibraryWorkflow::load(&paths).unwrap();

    let model = initial_library_model(&session, None);

    assert_eq!(model.workspace, Workspace::Library);
    assert_eq!(model.overlay, Overlay::Welcome);
    assert_eq!(model.rows[model.selected].kind, RowKind::Location);
    assert_eq!(model.rows[model.selected].name, "./library");
    assert!(!model.dirty);
}

#[test]
fn first_run_shows_the_existing_default_library_inventory() {
    let home = tempfile::tempdir().unwrap();
    let paths = AppPaths::new(home.path().to_owned());
    let skill = home.path().join(".skillator/library/unslop");
    std::fs::create_dir_all(&skill).unwrap();
    std::fs::write(
        skill.join("SKILL.md"),
        "---\nname: unslop\ndescription: Cut AI tells\n---\n",
    )
    .unwrap();
    let session = LibraryWorkflow::load(&paths).unwrap();

    let snapshot = LibraryWorkflow::snapshot(&paths, &session.config);
    let model = initial_library_model(&session, Some(&snapshot));

    assert!(model.rows.iter().any(|row| {
        row.kind == RowKind::Skill && row.name == "unslop" && row.description == "Cut AI tells"
    }));
}

#[test]
fn naming_warnings_allow_toggles_but_unsafe_metadata_does_not() {
    let home = tempfile::tempdir().unwrap();
    let location = home.path().join("library");
    let skill = location.join("release-checklist");
    std::fs::create_dir_all(&skill).unwrap();
    let document =
        "---\nname: Make Bot UI\ndescription: Prepare releases\n---\n\n# Release checklist\n";
    std::fs::write(skill.join("SKILL.md"), document).unwrap();
    let library_config = LibraryConfig::new(vec![LibraryLocationConfig::new(
        location.display().to_string(),
        Vec::new(),
        false,
    )])
    .unwrap();
    let library = crate::library::scan_library(
        &library_config,
        &home.path().join("library.yaml"),
        home.path(),
        &BTreeMap::new(),
    );
    let config =
        RepositoryConfig::new(vec![SkillDirectoryConfig::agents_preset()], Vec::new()).unwrap();
    let target = crate::target::Target::user(home.path()).unwrap();
    let observed = observe(&target, &config, &library);
    let rows = rows_for_directory(
        config.skill_directories().first().unwrap(),
        &config,
        &library,
        &library_config,
        &observed,
        &BTreeSet::new(),
        None,
    );

    let selected = rows
        .iter()
        .position(|row| row.kind == RowKind::Skill && row.name == "Make Bot UI")
        .unwrap();
    let mut model = Model::new(Workspace::Target, rows);
    model.selected = selected;
    reduce(&mut model, Action::Toggle);
    assert_eq!(model.rows[selected].check, Some(CheckState::Checked));

    let mut local_model = Model::new(Workspace::Library, library_rows(&library_config, &library));
    local_model.selected = local_model
        .rows
        .iter()
        .position(|row| row.kind == RowKind::Skill && row.name == "Make Bot UI")
        .unwrap();
    reduce(&mut local_model, Action::Toggle);
    assert_eq!(
        local_model.rows[local_model.selected].check,
        Some(CheckState::Unchecked)
    );
    std::fs::write(
        skill.join("SKILL.md"),
        "---\nname: ../unsafe\ndescription: Prepare releases\n---\n",
    )
    .unwrap();
    let invalid_library = crate::library::scan_library(
        &library_config,
        &home.path().join("library.yaml"),
        home.path(),
        &BTreeMap::new(),
    );
    let invalid_rows = library_rows(&library_config, &invalid_library);
    let invalid_index = invalid_rows
        .iter()
        .position(|row| row.kind == RowKind::Skill)
        .unwrap();
    assert!(row_is_error(&invalid_rows[invalid_index]));
    let mut invalid_model = Model::new(Workspace::Library, invalid_rows);
    invalid_model.selected = invalid_index;
    reduce(&mut invalid_model, Action::Toggle);
    assert_eq!(
        invalid_model.rows[invalid_index].check,
        Some(CheckState::Invalid)
    );
}

#[test]
fn skill_details_document_end_remains_reachable_with_warnings() {
    let document = format!(
        "---\nname: Example Skill\ndescription: Notes\n---\n{}\nFinal body line\n",
        "body\n".repeat(15)
    );
    let mut model = Model::new(Workspace::Library, Vec::new());
    model.overlay = Overlay::Details {
        title: "Example Skill".to_owned(),
        path: "examples/SKILL.md".to_owned(),
        document,
        warnings: vec!["Name differs from its directory".to_owned()],
        errors: Vec::new(),
    };
    let backend = ratatui::backend::TestBackend::new(100, 24);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal.draw(|frame| render(frame, &model)).unwrap();
    let screen = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(!screen.contains("Final body line"));

    reduce(&mut model, Action::PageDown);
    reduce(&mut model, Action::PageDown);
    terminal.draw(|frame| render(frame, &model)).unwrap();
    let screen = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(screen.contains("Final body line"));
}

#[test]
fn welcome_dialog_continues_or_exits() {
    let mut model = Model::new(Workspace::Library, Vec::new());
    model.overlay = Overlay::Welcome;

    assert!(reduce(&mut model, Action::Confirm).is_empty());
    assert_eq!(model.overlay, Overlay::None);

    model.overlay = Overlay::Welcome;
    assert_eq!(
        reduce(&mut model, Action::Escape),
        [Effect::Quit { status: 0 }]
    );
}

#[test]
fn library_rows_do_not_persist_discovered_inventory() {
    let original = LibraryConfig::new(vec![LibraryLocationConfig::new(
        "./library".to_owned(),
        Vec::new(),
        false,
    )])
    .unwrap();
    let first_id = source_inventory_id(0, "old");
    let second_id = source_inventory_id(0, "new");
    let first = Row::source_inventory(
        "acme/skills".to_owned(),
        CheckState::Checked,
        0,
        "old".to_owned(),
        true,
        false,
    );
    let first_skill = Row::skill_inventory(SkillInventoryRow {
        group: "acme/skills".to_owned(),
        inventory_id: Some(first_id),
        path: "old-skill".to_owned(),
        name: "old-skill".to_owned(),
        description: String::new(),
        check: CheckState::Checked,
        available: true,
        valid: true,
        mode: None,
        state: String::new(),
        details: String::new(),
        location_index: Some(0),
    });
    let mut second = Row::source_inventory(
        "acme/skills".to_owned(),
        CheckState::Unchecked,
        0,
        "new".to_owned(),
        true,
        true,
    );
    second.name = "other/skills".to_owned();

    second.key_collision = false;
    let second_skill = Row::skill_inventory(SkillInventoryRow {
        group: "acme/skills".to_owned(),
        inventory_id: Some(second_id),
        path: "new-skill".to_owned(),
        name: "new-skill".to_owned(),
        description: String::new(),
        check: CheckState::Checked,
        available: true,
        valid: true,
        mode: None,
        state: String::new(),
        details: String::new(),
        location_index: Some(0),
    });

    let config =
        library_config_from_rows(&original, &[first, first_skill, second, second_skill]).unwrap();

    assert_eq!(config, original);
}

#[test]
fn escape_clears_an_active_filter_without_changing_collapse_state() {
    let mut model = Model::new(Workspace::Library, Vec::new());
    model.filter = "release".to_owned();
    model.collapsed.insert("source".to_owned());
    model.overlay = Overlay::Filter;

    reduce(&mut model, Action::Escape);

    assert!(model.filter.is_empty());
    assert_eq!(model.overlay, Overlay::None);
    assert!(model.collapsed.contains("source"));
}

#[test]
fn collapse_from_a_child_selects_and_collapses_its_source() {
    let source = Row::source("acme/skills", CheckState::Checked);
    let child = Row::skill(
        "acme/skills",
        "demo",
        "Demo Skill",
        false,
        true,
        MaterializationKind::Linked,
        "",
    );
    let mut model = Model::new(Workspace::Library, vec![source, child]);
    model.selected = 1;

    reduce(&mut model, Action::Collapse);

    assert!(model.is_collapsed("acme/skills"));
    assert_eq!(model.selected, 0);
}

#[test]
fn page_navigation_moves_through_visible_rows() {
    let rows = (0..20)
        .map(|index| Row::location(format!("location-{index}")))
        .collect();
    let mut model = Model::new(Workspace::Library, rows);

    reduce(&mut model, Action::PageDown);
    assert_eq!(model.selected, 10);
    reduce(&mut model, Action::PageUp);
    assert_eq!(model.selected, 0);
}

#[test]
fn invalid_children_are_excluded_from_source_rollups_and_bulk_toggles() {
    let source = Row::source("local/library", CheckState::Mixed);
    let valid = Row::skill(
        "local/library",
        "valid",
        "Valid",
        true,
        true,
        MaterializationKind::Linked,
        "Registered",
    );
    let mut invalid = Row::skill(
        "local/library",
        "invalid",
        "Invalid",
        false,
        false,
        MaterializationKind::Linked,
        "Invalid",
    );
    invalid.check = Some(CheckState::Invalid);
    invalid.valid = false;
    let mut model = Model::new(Workspace::Target, vec![source, valid, invalid]);

    reduce(&mut model, Action::Toggle);

    assert_eq!(model.rows[0].check, Some(CheckState::Unchecked));
    assert_eq!(model.rows[1].check, Some(CheckState::Unchecked));
    assert_eq!(model.rows[2].check, Some(CheckState::Invalid));
}

#[test]
fn returning_from_save_review_emits_cancellation_for_the_prepared_plan() {
    let mut model = Model::new(Workspace::Target, Vec::new());
    model.overlay = Overlay::ConfirmSave;

    let effects = reduce(&mut model, Action::ReturnToEditing);

    assert_eq!(effects, [Effect::CancelSave]);
    assert_eq!(model.overlay, Overlay::None);
}

#[test]
fn dirty_scope_switch_offers_save_discard_or_return() {
    let mut model = Model::new(Workspace::Target, Vec::new());
    model.dirty = true;
    assert!(reduce(&mut model, Action::PreviousScope).is_empty());
    assert_eq!(
        model.overlay,
        Overlay::ScopeSwitch {
            destination: Scope::User,
            host_to: None,
        }
    );
    assert_eq!(
        reduce(&mut model, Action::Confirm),
        [Effect::SwitchToScope {
            destination: Scope::User,
            host_to: None,
            save: true,
            discard: false,
        }]
    );
    model.overlay = Overlay::ScopeSwitch {
        destination: Scope::Library,
        host_to: None,
    };
    assert_eq!(
        reduce(&mut model, Action::DeleteDirectory),
        [Effect::SwitchToScope {
            destination: Scope::Library,
            host_to: None,
            save: false,
            discard: true,
        }]
    );
}

#[test]
fn library_acquisition_mode_cycles_move_copy_link_and_clear() {
    let mut row = Row::skill(
        "external/skills",
        "demo",
        "Demo Skill",
        false,
        true,
        MaterializationKind::Linked,
        "",
    );
    row.acquisition_source = Some(std::path::PathBuf::from("/external/demo"));

    row.check = Some(CheckState::Checked);
    row.initial_check = row.check;
    let mut model = Model::new(Workspace::Library, vec![row]);

    reduce(&mut model, Action::SwitchMode);
    assert_eq!(
        model.rows[0].acquisition_mode,
        Some(LibraryAcquisitionMode::Move)
    );
    assert_eq!(model.rows[0].action, "Move to Library");

    reduce(&mut model, Action::SwitchMode);
    assert_eq!(
        model.rows[0].acquisition_mode,
        Some(LibraryAcquisitionMode::Copy)
    );
    assert_eq!(model.rows[0].action, "Copy to Library");

    reduce(&mut model, Action::SwitchMode);
    assert_eq!(
        model.rows[0].acquisition_mode,
        Some(LibraryAcquisitionMode::Link)
    );
    assert_eq!(model.rows[0].action, "Link to Library");

    reduce(&mut model, Action::SwitchMode);
    assert_eq!(model.rows[0].acquisition_mode, None);
    assert_eq!(model.rows[0].action, "");
}

#[test]
fn repository_candidate_cycles_to_read_only_repo_without_an_enablement() {
    let directory = tempfile::tempdir().unwrap();
    let skill = directory.path().join("project-skill");
    std::fs::create_dir_all(&skill).unwrap();
    std::fs::write(
        skill.join("SKILL.md"),
        "---\nname: project-skill\ndescription: Project-owned instructions\n---\n",
    )
    .unwrap();
    let row = Row::repository_skill(
        "project-skill".to_owned(),
        "project-skill".to_owned(),
        "Project-owned instructions".to_owned(),
        "---\nname: project-skill\ndescription: Project-owned instructions\n---\n".to_owned(),
        &skill,
        false,
        false,
    );
    let mut model = Model::new(Workspace::Target, vec![row]);

    reduce(&mut model, Action::SwitchMode);

    assert_eq!(model.rows[0].check, Some(CheckState::Repository));
    assert_eq!(model.rows[0].action, "Track in repository");
    let config = repository_config_from_rows(
        &[SkillDirectoryConfig::agents_preset()],
        &[model.rows.clone()],
    )
    .unwrap();
    assert!(config.enablements().is_empty());

    reduce(&mut model, Action::Toggle);
    assert_eq!(model.rows[0].check, Some(CheckState::Repository));
    assert!(matches!(model.overlay, Overlay::Notice(_)));
    model.overlay = Overlay::None;
    reduce(&mut model, Action::SwitchMode);
    assert_eq!(model.rows[0].check, Some(CheckState::Repository));
    assert!(matches!(model.overlay, Overlay::Notice(_)));
}

#[test]
fn repository_skill_renders_with_r_marker_and_repo_mode() {
    let directory = tempfile::tempdir().unwrap();
    let skill = directory.path().join("project-skill");
    std::fs::create_dir_all(&skill).unwrap();
    std::fs::write(skill.join("SKILL.md"), "repository skill").unwrap();
    let row = Row::repository_skill(
        "project-skill".to_owned(),
        "project-skill".to_owned(),
        "Project-owned instructions".to_owned(),
        "repository skill".to_owned(),
        &skill,
        false,
        true,
    );
    let model = Model::new(Workspace::Target, vec![row]);
    let backend = ratatui::backend::TestBackend::new(90, 12);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();

    terminal.draw(|frame| render(frame, &model)).unwrap();

    let screen = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(screen.contains("[r]"));
    assert!(screen.contains("repo"));
}

#[test]
fn target_rows_discover_excepted_repository_skills() {
    let home = tempfile::tempdir().unwrap();
    let output = std::process::Command::new("git")
        .args(["init", "--quiet"])
        .arg(home.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let skill = home.path().join(".agents/skills/project-skill");
    std::fs::create_dir_all(&skill).unwrap();
    std::fs::write(
        skill.join("SKILL.md"),
        "---\nname: project-skill\ndescription: Project-owned instructions\n---\n",
    )
    .unwrap();
    let library_location = home.path().join("library");
    let library_skill = library_location.join("library-skill");
    std::fs::create_dir_all(&library_skill).unwrap();
    std::fs::write(
        library_skill.join("SKILL.md"),
        "---\nname: library-skill\ndescription: Library instructions\n---\n",
    )
    .unwrap();
    std::fs::write(
        home.path().join(".agents/.gitignore"),
        "# Generated by Skillator\n.gitignore\nskillator.yaml\nskills/*\n\n# Exception list for repository tracking\n!skills/project-skill/\n",
    )
    .unwrap();
    let config =
        RepositoryConfig::new(vec![SkillDirectoryConfig::agents_preset()], Vec::new()).unwrap();
    let library_config = LibraryConfig::new(vec![LibraryLocationConfig::new(
        library_location.display().to_string(),
        Vec::new(),
        false,
    )])
    .unwrap();
    let library = crate::library::scan_library(
        &library_config,
        &home.path().join("library.yaml"),
        home.path(),
        &BTreeMap::new(),
    );
    let target = Target::select(home.path()).unwrap();
    let observed = observe(&target, &config, &library);

    let rows = rows_for_directory(
        config.skill_directories().first().unwrap(),
        &config,
        &library,
        &library_config,
        &observed,
        &BTreeSet::new(),
        Some(&target),
    );

    let row = rows
        .iter()
        .find(|row| row.repository_name.as_deref() == Some("project-skill"))
        .expect("repository Skill row");
    assert_eq!(row.check, Some(CheckState::Repository));
    assert!(row.mode.is_none());
    let repository_group = rows
        .iter()
        .position(|row| row.kind == RowKind::Source && row.name == "Repository")
        .expect("Repository group");
    let library_group = rows
        .iter()
        .position(|row| row.kind == RowKind::Source && row.name != "Repository")
        .expect("Library group");
    assert!(repository_group < library_group);
}

#[test]
fn shifted_slash_is_literal_text_in_an_editor() {
    let mut model = Model::new(Workspace::Library, Vec::new());
    model.overlay = Overlay::LocationEditor {
        edit: false,
        input: String::new(),
    };

    assert_eq!(
        action_for_model_key(
            &model,
            KeyEvent::new(KeyCode::Char('/'), KeyModifiers::SHIFT)
        ),
        Some(Action::Input('/'))
    );
}
#[test]
fn chooser_validates_custom_paths_and_preserves_independent_scope_keys() {
    let existing = TargetTab {
        scope: TargetTabScope::User,
        directory: SkillDirectoryConfig::user_preset(),
        rows: Vec::new(),
    };
    let tabs = [existing];
    assert!(
        parse_chooser_directory(".agents/skills", &tabs, TargetTabScope::User)
            .unwrap_err()
            .contains("collision")
    );
    assert!(parse_chooser_directory("../escape/skills", &tabs, TargetTabScope::User).is_err());
    let same_path_in_repo =
        parse_chooser_directory(".agents/skills", &tabs, TargetTabScope::Repository).unwrap();
    assert_eq!(same_path_in_repo.key().as_str(), "agents");
    let custom =
        parse_chooser_directory("custom-agent/skills", &tabs, TargetTabScope::User).unwrap();
    assert_eq!(custom.key().as_str(), "custom-agent");
    assert_eq!(custom.label(), Some("custom-agent"));
    assert!(RepositoryConfig::new(vec![tabs[0].directory.clone(), custom], vec![]).is_ok());
}

#[cfg(unix)]
#[test]
fn chooser_does_not_stage_a_path_through_a_symlink() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(outside.path(), root.path().join("linked")).unwrap();
    assert!(
        validate_directory_containment(root.path(), "linked/skills")
            .unwrap_err()
            .contains("symbolic link")
    );
    assert!(validate_directory_containment(root.path(), ".agents/skills").is_ok());
}
#[test]
fn follower_sync_controls_never_save_or_exit_and_block_duplicate_delivery() {
    for fast in [false, true] {
        let mut model = Model::new(Workspace::Library, vec![Row::location("./library")]);
        model.host_labels.push("build".to_owned());
        model.host_index = 1;
        assert!(reduce(&mut model, Action::Save { fast }).is_empty());
        assert_eq!(
            model.overlay,
            Overlay::ConfirmFollowerSync {
                alias: "build".to_owned(),
                initial: false
            }
        );
        assert!(!model.exit_after_save);
        assert_eq!(
            reduce(&mut model, Action::ReturnToEditing),
            [Effect::FinishFollowerInit]
        );
        assert!(reduce(&mut model, Action::Save { fast }).is_empty());
        assert_eq!(
            reduce(&mut model, Action::Confirm),
            [Effect::StartFollowerSync("build".to_owned())]
        );
        model.host_syncing = true;
        assert!(reduce(&mut model, Action::Save { fast }).is_empty());
        reduce(&mut model, Action::Escape);
        assert_eq!(
            reduce(&mut model, Action::Escape),
            [Effect::CancelFollowerSync]
        );
        assert!(matches!(
            reduce(&mut model, Action::NextScope).as_slice(),
            [Effect::SwitchToScope {
                save: false,
                discard: false,
                ..
            }]
        ));
    }
    let mut local = Model::new(Workspace::Library, Vec::new());
    assert_eq!(
        reduce(&mut local, Action::Save { fast: false }),
        [Effect::PrepareSave { fast: false }]
    );
}

#[test]
fn follower_initialization_requires_saved_registration_and_preserves_order() {
    let home = tempfile::tempdir().unwrap();
    let mut host = HostUi::new(home.path(), &[]);
    let mut model = Model::new(Workspace::Library, Vec::new());
    for alias in ["zeta", "alpha"] {
        host.registry
            .as_mut()
            .unwrap()
            .stage(alias, "worker-07")
            .unwrap();
        host.staged_hosts.push(alias.to_owned());
    }
    assert!(!host.offer_initial_sync(&mut model));
    assert!(host.save_registry().unwrap());
    model.host_labels = host.labels();
    assert!(host.offer_initial_sync(&mut model));
    assert_eq!(
        model.overlay,
        Overlay::ConfirmFollowerSync {
            alias: "zeta".to_owned(),
            initial: true
        }
    );
    assert_eq!(
        reduce(&mut model, Action::ReturnToEditing),
        [Effect::FinishFollowerInit]
    );
    assert!(host.offer_initial_sync(&mut model));
    assert_eq!(
        model.overlay,
        Overlay::ConfirmFollowerSync {
            alias: "alpha".to_owned(),
            initial: true
        }
    );
    reduce(&mut model, Action::Escape);
    assert!(!host.offer_initial_sync(&mut model));
    assert_eq!(
        HostRegistry::load(home.path()).unwrap().followers().count(),
        2
    );

    host.registry
        .as_mut()
        .unwrap()
        .stage("unsaved", "worker-08")
        .unwrap();
    host.staged_hosts.push("unsaved".to_owned());
    std::fs::write(
        home.path().join(".skillator/config.yaml"),
        "version: 1\nhosts:\n  external:\n    destination: elsewhere\n",
    )
    .unwrap();
    assert!(host.save_registry().unwrap_err().contains("changed"));
    assert!(!host.offer_initial_sync(&mut model));
    host.discard_registry();
    assert!(host.staged_hosts.is_empty());
    assert!(!home.path().join(".skillator/library/replica").exists());
}

#[test]
fn follower_sync_results_wait_for_overlay_and_cannot_replace_other_host() {
    let home = tempfile::tempdir().unwrap();
    let mut host = HostUi::new(home.path(), &[]);
    let mut model = Model::new(Workspace::Library, vec![Row::location("retained replica")]);
    model.host_labels.push("build".to_owned());
    model.host_index = 1;
    model.host_syncing = true;
    model.overlay = Overlay::Filter;
    model.filter = "release".to_owned();
    host.tx
        .send(HostReply::Sync {
            request: host.request,
            alias: "build".to_owned(),
            result: Err("SSH denied; retained private export requires cleanup".to_owned()),
        })
        .unwrap();
    host.poll(&mut model);
    assert_eq!(model.overlay, Overlay::Filter);
    assert!(model.host_syncing);
    reduce(&mut model, Action::Confirm);
    assert!(host.poll(&mut model));
    assert!(
        matches!(&model.overlay, Overlay::FollowerSyncResult { body, .. }
        if body.contains("SSH denied") && body.contains("private export"))
    );
    assert_eq!(model.filter, "release");
    assert!(!model.host_syncing);
    reduce(&mut model, Action::Confirm);
    let stale_request = host.request;
    host.select_host(&mut model, 0, None);
    host.tx
        .send(HostReply::Sync {
            request: stale_request,
            alias: "build".to_owned(),
            result: Err("obsolete failure".to_owned()),
        })
        .unwrap();
    assert!(!host.poll(&mut model));
    assert_eq!(model.host_index, 0);
    assert_eq!(model.overlay, Overlay::None);
    assert!(model.rows.is_empty());
    model.host_index = 1;
    model.host_dirty = true;
    assert!(reduce(&mut model, Action::Save { fast: false }).is_empty());
    assert_eq!(
        model.overlay,
        Overlay::ScopeSwitch {
            destination: Scope::Library,
            host_to: Some(0)
        }
    );
}

#[test]
fn cancelled_follower_reply_cannot_replace_current_library() {
    let home = tempfile::tempdir().unwrap();
    let mut host = HostUi::new(home.path(), &[Row::location("./library")]);
    let mut model = Model::new(Workspace::Library, vec![Row::location("./library")]);
    let stale_request = host.request;
    host.cancel();
    host.tx
        .send(HostReply::Probe {
            request: stale_request,
            name: "build".to_owned(),
            result: Ok(hosts::ProbeResult {
                hostname: "worker-07.example.net".to_owned(),
                warning: None,
            }),
        })
        .unwrap();
    host.poll(&mut model);
    assert_eq!(model.rows()[0].name(), "./library");
    assert_eq!(host.labels(), ["Local"]);
    assert!(!host.registry.as_ref().unwrap().dirty());
}

#[test]
fn host_only_changes_guard_target_picker_without_clearing_staged_registration() {
    let mut model = Model::new(Workspace::Library, vec![Row::location("./library")]);
    model.host_dirty = true;
    assert!(reduce(&mut model, Action::ChangeTarget).is_empty());
    assert_eq!(model.overlay, Overlay::DiscardTarget);
    assert!(reduce(&mut model, Action::Confirm).is_empty());
    assert_eq!(model.overlay, Overlay::TargetPicker(String::new()));
    assert!(model.host_dirty);
    assert!(reduce(&mut model, Action::Escape).is_empty());
    assert_eq!(model.overlay, Overlay::None);
    assert!(model.host_dirty);
    assert!(reduce(&mut model, Action::ChangeTarget).is_empty());
    assert_eq!(model.overlay, Overlay::DiscardTarget);
}

#[test]
fn follower_probe_stages_without_switching_and_undo_discards_registration() {
    let home = tempfile::tempdir().unwrap();
    let mut host = HostUi::new(home.path(), &[Row::location("./library")]);
    let mut model = Model::new(Workspace::Library, vec![Row::location("./library")]);
    model.overlay = Overlay::FollowerProbe {
        name: "build".to_owned(),
    };
    host.tx
        .send(HostReply::Probe {
            request: host.request,
            name: "build".to_owned(),
            result: Ok(hosts::ProbeResult {
                hostname: "worker-07.example.net".to_owned(),
                warning: Some("fixture warning from SSH".to_owned()),
            }),
        })
        .unwrap();

    assert!(host.poll(&mut model));
    assert_eq!(model.host_index, 0);
    assert_eq!(host.labels(), ["Local", "build"]);
    assert!(model.host_dirty);
    assert!(matches!(&model.overlay, Overlay::Diagnostic { body, .. }
        if body.contains("fixture warning from SSH") && body.contains("worker-07.example.net")));
    assert!(reduce(&mut model, Action::Escape).is_empty());
    assert_eq!(reduce(&mut model, Action::Undo), vec![Effect::Undo]);
    host.registry.as_mut().unwrap().discard();
    assert_eq!(host.labels(), ["Local"]);
    assert!(!home.path().join(".skillator/config.yaml").exists());
}
#[test]
fn remote_inventory_rows_never_offer_mutation() {
    let inventory = ReplicaInventory {
        path: "/home/worker/.skillator/library/replica".to_owned(),
        skills: vec![hosts::ReplicaSkill {
            source_key: "local/library".to_owned(),
            skill_path: "release".to_owned(),
            description: "Release skill".to_owned(),
            document: "---\nname: release\n---\n".to_owned(),
            diagnostic: None,
            warnings: vec!["Name differs from skill directory".to_owned()],
        }],
        warning: None,
    };
    let mut model = Model::new(Workspace::Library, replica_rows("build", inventory));
    model.host_labels.push("build".to_owned());
    model.host_index = 1;
    assert!(model.rows.iter().any(|row| row.name == "local/library"));
    assert!(model.rows.iter().all(|row| !row_is_error(row)));
    assert_eq!(
        model
            .rows
            .iter()
            .filter(|row| row.kind == RowKind::Diagnostic)
            .count(),
        0
    );
    model.selected = model
        .rows
        .iter()
        .position(|row| row.name == "release")
        .unwrap();
    reduce(&mut model, Action::Confirm);
    assert!(matches!(
        model.overlay,
        Overlay::Details { ref warnings, ref errors, .. }
            if warnings.len() == 1 && errors.is_empty()
    ));
    reduce(&mut model, Action::Escape);
    assert!(
        model
            .rows
            .iter()
            .any(|row| row.name == "release" && row.check.is_none())
    );
    model.filter = "pending".to_owned();
    assert!(
        model.visible_indices().is_empty(),
        "replica skills are not pending local edits"
    );
    model.filter.clear();
    assert!(reduce(&mut model, Action::Toggle).is_empty());
    assert!(matches!(model.overlay, Overlay::Notice(_)));
    assert!(!model.dirty());
}

fn refresh_fixture() -> (tempfile::TempDir, AppPaths, LibraryConfig) {
    let home = tempfile::tempdir().unwrap();
    let paths = AppPaths::new(home.path().to_owned());
    let location = home.path().join("library");
    write_refresh_skill(&location, "alpha");
    let config = LibraryConfig::new(vec![LibraryLocationConfig::new(
        location.to_string_lossy().into_owned(),
        Vec::new(),
        false,
    )])
    .unwrap();
    (home, paths, config)
}

fn write_refresh_skill(location: &Path, name: &str) {
    let skill = location.join(name);
    std::fs::create_dir_all(&skill).unwrap();
    std::fs::write(
        skill.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: {name} skill\n---\n"),
    )
    .unwrap();
}

fn finish_library_scans(paths: &AppPaths, refresh: &mut LibraryRefresh) {
    while let Some(flight) = &refresh.flight {
        let discovery = flight
            .receiver
            .recv_timeout(Duration::from_secs(10))
            .unwrap();
        refresh.complete(paths, discovery);
    }
}

fn refresh_model(paths: &AppPaths, config: &LibraryConfig) -> Model {
    Model::new(
        Workspace::Library,
        library_rows(config, &LibraryWorkflow::snapshot(paths, config)),
    )
}

#[test]
fn superseded_library_discovery_never_restores_obsolete_locations() {
    let (home, paths, config) = refresh_fixture();
    let mut model = refresh_model(&paths, &config);
    let mut hosts = HostUi::new(paths.home(), &model.rows);
    let mut refresh = LibraryRefresh::default();
    refresh.request(&paths, &config);
    for name in ["beta", "gamma"] {
        let location = home.path().join(name);
        write_refresh_skill(&location, name);
        let replacement = LibraryConfig::new(vec![LibraryLocationConfig::new(
            location.to_string_lossy().into_owned(),
            Vec::new(),
            false,
        )])
        .unwrap();
        refresh.request(&paths, &replacement);
    }
    let obsolete = refresh
        .flight
        .as_ref()
        .unwrap()
        .receiver
        .recv_timeout(Duration::from_secs(10))
        .unwrap();
    refresh.complete(&paths, obsolete);
    assert!(!refresh.apply(&mut model, &mut hosts));
    assert!(
        model
            .rows
            .iter()
            .any(|row| row.kind == RowKind::Skill && row.name == "alpha")
    );
    finish_library_scans(&paths, &mut refresh);
    assert!(refresh.apply(&mut model, &mut hosts));
    assert!(
        model
            .rows
            .iter()
            .any(|row| row.kind == RowKind::Skill && row.name == "gamma")
    );
    assert!(
        !model
            .rows
            .iter()
            .any(|row| row.kind == RowKind::Skill && matches!(row.name.as_str(), "alpha" | "beta"))
    );
}

#[test]
fn library_discovery_cannot_overwrite_staged_checks_or_acquisition_modes() {
    let (home, paths, base_config) = refresh_fixture();
    let destination = home.path().join("destination");
    std::fs::create_dir(&destination).unwrap();
    let config = LibraryConfig::new(vec![
        LibraryLocationConfig::new(
            destination.to_string_lossy().into_owned(),
            Vec::new(),
            false,
        ),
        base_config.locations()[0].clone(),
    ])
    .unwrap();
    let mut model = refresh_model(&paths, &config);
    model.selected = model
        .rows
        .iter()
        .position(|row| row.kind == RowKind::Skill)
        .unwrap();
    reduce(&mut model, Action::Toggle);
    reduce(&mut model, Action::SwitchMode);
    assert!(model.dirty);
    let check = model.selected_row().unwrap().check;
    let mode = model.selected_row().unwrap().acquisition_mode;
    assert_eq!(check, Some(CheckState::Unchecked));
    assert_eq!(mode, Some(LibraryAcquisitionMode::Move));
    let mut hosts = HostUi::new(paths.home(), &model.rows);
    write_refresh_skill(&home.path().join("library"), "beta");
    let mut refresh = LibraryRefresh::default();
    refresh.request(&paths, &config);
    finish_library_scans(&paths, &mut refresh);
    assert!(!refresh.apply(&mut model, &mut hosts));
    assert_eq!(model.selected_row().unwrap().name, "alpha");
    assert_eq!(model.selected_row().unwrap().check, check);
    assert_eq!(model.selected_row().unwrap().acquisition_mode, mode);
    assert!(!model.rows.iter().any(|row| row.name == "beta"));
    assert!(model.dirty);
}

#[test]
fn library_refresh_waits_for_editor_and_preserves_browsing_identity() {
    let (home, paths, config) = refresh_fixture();
    let mut model = refresh_model(&paths, &config);
    model.selected = model
        .rows
        .iter()
        .position(|row| row.kind == RowKind::Skill)
        .unwrap();
    let group = model.selected_row().unwrap().group.clone().unwrap();
    model.collapsed.insert(group.clone());
    model.filter = "alpha".to_owned();
    reduce(&mut model, Action::AddDirectory);
    for character in "custom".chars() {
        reduce(&mut model, Action::Input(character));
    }
    let mut hosts = HostUi::new(paths.home(), &model.rows);
    write_refresh_skill(&home.path().join("library"), "aardvark");
    let mut refresh = LibraryRefresh::default();
    refresh.request(&paths, &config);
    finish_library_scans(&paths, &mut refresh);
    assert!(!refresh.apply(&mut model, &mut hosts));
    assert!(matches!(&model.overlay, Overlay::LocationEditor { input, .. } if input == "custom"));
    assert!(!model.rows.iter().any(|row| row.name == "aardvark"));
    reduce(&mut model, Action::Escape);
    assert!(refresh.apply(&mut model, &mut hosts));
    assert_eq!(model.selected_row().unwrap().name, "alpha");
    assert_eq!(model.filter, "alpha");
    assert!(model.collapsed.contains(&group));
    assert!(
        model
            .rows
            .iter()
            .any(|row| row.kind == RowKind::Skill && row.name == "aardvark")
    );
}

fn target_refresh_fixture() -> (tempfile::TempDir, AppPaths, TargetUi, Model) {
    let (home, paths, config) = refresh_fixture();
    let session = LibraryWorkflow::load(&paths).unwrap();
    LibraryWorkflow::save(&paths, &session, &config, true).unwrap();
    std::fs::create_dir_all(paths.user_config().parent().unwrap()).unwrap();
    std::fs::write(
        paths.user_config(),
        "version: 1\nbravo:\n  path: \".bravo/skills\"\n  skills: {}\ncharlie:\n  path: \".charlie/skills\"\n  skills: {}\n",
    )
    .unwrap();
    let directory = home.path().join("repo");
    std::fs::create_dir(&directory).unwrap();
    let git = std::process::Command::new("git")
        .args(["init", "--quiet"])
        .arg(&directory)
        .output()
        .unwrap();
    assert!(git.status.success());
    let directory = directory.canonicalize().unwrap();
    let data = load_target_data(&paths, &directory, None).unwrap();
    let mut model = initial_target_model(&data.state.tabs);
    let ui = TargetUi {
        directory,
        loaded: Some(data),
        dirty_scopes: BTreeSet::new(),
        refresh: TargetRefresh::default(),
    };
    ui.activate(&mut model, Scope::User, &paths);
    (home, paths, ui, model)
}

fn finish_target_scans(paths: &AppPaths, refresh: &mut TargetRefresh) {
    while let Some(flight) = &refresh.flight {
        let reply = flight
            .receiver
            .recv_timeout(Duration::from_secs(10))
            .unwrap();
        refresh.complete(paths, reply);
    }
}

fn request_target_refresh(paths: &AppPaths, ui: &mut TargetUi) {
    ui.refresh.request(
        paths,
        TargetRequest {
            directory: ui.directory.clone(),
            library_snapshot: None,
        },
    );
    finish_target_scans(paths, &mut ui.refresh);
}

#[test]
fn target_refresh_cannot_overwrite_hidden_scope_edits() {
    let (home, paths, mut ui, mut model) = target_refresh_fixture();
    model.selected = model
        .rows
        .iter()
        .position(|row| row.skill_path.as_deref() == Some("alpha"))
        .unwrap();
    reduce(&mut model, Action::Toggle);
    reduce(&mut model, Action::SwitchMode);
    assert_eq!(
        model.selected_row().unwrap().mode,
        Some(MaterializationKind::Copied)
    );
    assert_eq!(
        model.selected_row().unwrap().check,
        Some(CheckState::Checked)
    );
    assert!(model.dirty);
    store_active_target_tab(&model, &mut ui.loaded.as_mut().unwrap().state.tabs);
    ui.dirty_scopes.insert(TargetTabScope::User);
    ui.activate(&mut model, Scope::Repo, &paths);
    assert!(!model.dirty);
    write_refresh_skill(&home.path().join("library"), "beta");
    request_target_refresh(&paths, &mut ui);
    assert!(!ui.apply(&mut model, &paths));
    let user = ui
        .loaded
        .as_ref()
        .unwrap()
        .state
        .tabs
        .iter()
        .find(|tab| tab.scope == TargetTabScope::User)
        .unwrap();
    let alpha = user
        .rows
        .iter()
        .find(|row| row.skill_path.as_deref() == Some("alpha"))
        .unwrap();
    assert_eq!(alpha.check, Some(CheckState::Checked));
    assert_eq!(alpha.mode, Some(MaterializationKind::Copied));
    assert!(
        !model
            .rows
            .iter()
            .any(|row| row.skill_path.as_deref() == Some("beta"))
    );
}

#[test]
fn target_refresh_preserves_directory_and_skill_identity_after_insertions() {
    let (home, paths, mut ui, mut model) = target_refresh_fixture();
    for key in ["bravo", "charlie"] {
        let tabs = &ui.loaded.as_ref().unwrap().state.tabs;
        let index = tabs
            .iter()
            .position(|tab| {
                tab.scope == TargetTabScope::User && tab.directory.key().as_str() == key
            })
            .unwrap();
        activate_target_tab(&mut model, tabs, &ui.dirty_scopes, index);
        model.selected = model
            .rows
            .iter()
            .position(|row| row.skill_path.as_deref() == Some("alpha"))
            .unwrap();
        model.filter = "alpha".to_owned();
        model
            .collapsed
            .insert(model.selected_row().unwrap().group.clone().unwrap());
        stash_target_browse(&mut model, tabs, index);
    }
    reduce(&mut model, Action::AddDirectory);
    for character in "custom".chars() {
        reduce(&mut model, Action::Input(character));
    }
    write_refresh_skill(&home.path().join("library"), "aardvark");
    write_refresh_skill(&home.path().join("library/aaa"), "alpha");
    std::fs::write(
        paths.user_config(),
        "version: 1\naaa:\n  path: \".aaa/skills\"\n  skills: {}\nbravo:\n  path: \".bravo/skills\"\n  skills: {}\ncharlie:\n  path: \".charlie/skills\"\n  skills: {}\n",
    ).unwrap();
    request_target_refresh(&paths, &mut ui);
    assert!(!ui.apply(&mut model, &paths));
    assert!(matches!(&model.overlay, Overlay::DirectoryChooser { input, .. } if input == "custom"));
    assert_eq!(
        model.selected_row().unwrap().skill_path.as_deref(),
        Some("alpha")
    );
    reduce(&mut model, Action::Escape);
    assert!(ui.apply(&mut model, &paths));
    let tabs = &ui.loaded.as_ref().unwrap().state.tabs;
    assert_eq!(
        tabs[model.directory_index].directory.key().as_str(),
        "charlie"
    );
    assert_eq!(
        model.selected_row().unwrap().skill_path.as_deref(),
        Some("alpha")
    );
    assert_eq!(model.filter, "alpha");
    assert!(
        model
            .collapsed
            .contains(model.selected_row().unwrap().group.as_ref().unwrap())
    );
    let bravo = tabs
        .iter()
        .position(|tab| tab.directory.key().as_str() == "bravo")
        .unwrap();
    activate_target_tab(&mut model, tabs, &ui.dirty_scopes, bravo);
    assert_eq!(
        model.selected_row().unwrap().skill_path.as_deref(),
        Some("alpha")
    );
    assert_eq!(model.filter, "alpha");
    assert!(
        model
            .collapsed
            .contains(model.selected_row().unwrap().group.as_ref().unwrap())
    );
}

#[test]
fn target_refresh_rejects_invalidated_and_replaced_target_results() {
    let (home, paths, mut ui, mut model) = target_refresh_fixture();
    ui.refresh.request(
        &paths,
        TargetRequest {
            directory: ui.directory.clone(),
            library_snapshot: None,
        },
    );
    ui.refresh.invalidate();
    finish_target_scans(&paths, &mut ui.refresh);
    assert!(!ui.apply(&mut model, &paths));
    let other = home.path().join("other");
    std::fs::create_dir(&other).unwrap();
    let git = std::process::Command::new("git")
        .args(["init", "--quiet"])
        .arg(&other)
        .output()
        .unwrap();
    assert!(git.status.success());
    std::fs::create_dir(other.join(".agents")).unwrap();
    std::fs::write(
        other.join(".agents/skillator.yaml"),
        "version: 1\nother:\n  path: \".other/skills\"\n  skills: {}\n",
    )
    .unwrap();
    ui.refresh.request(
        &paths,
        TargetRequest {
            directory: ui.directory.clone(),
            library_snapshot: None,
        },
    );
    ui.refresh.request(
        &paths,
        TargetRequest {
            directory: other.clone(),
            library_snapshot: None,
        },
    );
    let obsolete = ui
        .refresh
        .flight
        .as_ref()
        .unwrap()
        .receiver
        .recv_timeout(Duration::from_secs(10))
        .unwrap();
    ui.refresh.complete(&paths, obsolete);
    assert!(!ui.apply(&mut model, &paths));
    assert_eq!(
        ui.loaded.as_ref().unwrap().state.repository.target.root(),
        ui.directory
    );
    finish_target_scans(&paths, &mut ui.refresh);
    assert!(ui.apply(&mut model, &paths));
    assert_eq!(ui.directory, other.canonicalize().unwrap());
    assert!(
        ui.loaded
            .as_ref()
            .unwrap()
            .state
            .tabs
            .iter()
            .any(|tab| tab.scope == TargetTabScope::Repository
                && tab.directory.key().as_str() == "other")
    );
}
