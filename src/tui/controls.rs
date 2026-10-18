use super::model::{Action, Model, Overlay, Scope, Workspace};

pub(super) struct Control {
    pub key: &'static str,
    pub label: &'static str,
    pub action: Action,
}

const TARGET_CONTROLS: &[Control] = &[
    Control {
        key: "s",
        label: "save",
        action: Action::Save { fast: false },
    },
    Control {
        key: "Ctrl+S",
        label: "save & exit",
        action: Action::Save { fast: true },
    },
    Control {
        key: "u",
        label: "undo",
        action: Action::Undo,
    },
    Control {
        key: "Space",
        label: "toggle",
        action: Action::Toggle,
    },
    Control {
        key: "m",
        label: "mode",
        action: Action::SwitchMode,
    },
    Control {
        key: "/",
        label: "filter",
        action: Action::StartFilter,
    },
    Control {
        key: "Ctrl+T",
        label: "new tab",
        action: Action::NewTargetTab,
    },
    Control {
        key: "Enter",
        label: "details",
        action: Action::Confirm,
    },
    Control {
        key: "?",
        label: "help",
        action: Action::Help,
    },
];

const LOCAL_LIBRARY_CONTROLS: &[Control] = &[
    Control {
        key: "s",
        label: "save",
        action: Action::Save { fast: false },
    },
    Control {
        key: "Ctrl+S",
        label: "save & exit",
        action: Action::Save { fast: true },
    },
    Control {
        key: "u",
        label: "undo",
        action: Action::Undo,
    },
    Control {
        key: "Space",
        label: "show/hide",
        action: Action::Toggle,
    },
    Control {
        key: "m",
        label: "mode",
        action: Action::SwitchMode,
    },
    Control {
        key: "/",
        label: "filter",
        action: Action::StartFilter,
    },
    Control {
        key: "a",
        label: "add",
        action: Action::AddDirectory,
    },
    Control {
        key: "e",
        label: "edit",
        action: Action::EditDirectory,
    },
    Control {
        key: "d",
        label: "remove",
        action: Action::DeleteDirectory,
    },
    Control {
        key: "Ctrl+T",
        label: "follower",
        action: Action::NewTargetTab,
    },
    Control {
        key: "r",
        label: "refresh",
        action: Action::RefreshLibrary,
    },
    Control {
        key: "Enter",
        label: "details",
        action: Action::Confirm,
    },
    Control {
        key: "?",
        label: "help",
        action: Action::Help,
    },
];

const REMOTE_LIBRARY_CONTROLS: &[Control] = &[
    Control {
        key: "s",
        label: "sync",
        action: Action::Save { fast: false },
    },
    Control {
        key: "Ctrl+S",
        label: "sync",
        action: Action::Save { fast: true },
    },
    Control {
        key: "/",
        label: "filter",
        action: Action::StartFilter,
    },
    Control {
        key: "Esc",
        label: "cancel sync",
        action: Action::Escape,
    },
    Control {
        key: "Enter",
        label: "details",
        action: Action::Confirm,
    },
    Control {
        key: "?",
        label: "help",
        action: Action::Help,
    },
];

const WELCOME_CONTROLS: &[Control] = &[
    Control {
        key: "Enter",
        label: "continue",
        action: Action::Confirm,
    },
    Control {
        key: "Esc",
        label: "exit",
        action: Action::Escape,
    },
];
const CLOSE_CONTROLS: &[Control] = &[Control {
    key: "Esc",
    label: "close",
    action: Action::Escape,
}];
const APPLY_CANCEL_CONTROLS: &[Control] = &[
    Control {
        key: "Enter",
        label: "apply",
        action: Action::Confirm,
    },
    Control {
        key: "Esc",
        label: "cancel",
        action: Action::Escape,
    },
];
const APPLY_CANCEL_COMPLETE_CONTROLS: &[Control] = &[
    Control {
        key: "Enter",
        label: "apply",
        action: Action::Confirm,
    },
    Control {
        key: "Tab",
        label: "complete",
        action: Action::CompletePath,
    },
    Control {
        key: "Esc",
        label: "cancel",
        action: Action::Escape,
    },
];
const PROCEED_RETURN_CONTROLS: &[Control] = &[
    Control {
        key: "Enter",
        label: "proceed",
        action: Action::Confirm,
    },
    Control {
        key: "Esc",
        label: "return",
        action: Action::Escape,
    },
];
const DISCARD_RETURN_CONTROLS: &[Control] = &[
    Control {
        key: "Enter",
        label: "discard",
        action: Action::Confirm,
    },
    Control {
        key: "Esc",
        label: "return",
        action: Action::Escape,
    },
];
const SCOPE_SWITCH_CONTROLS: &[Control] = &[
    Control {
        key: "Enter",
        label: "save + switch",
        action: Action::Confirm,
    },
    Control {
        key: "d",
        label: "discard + switch",
        action: Action::DeleteDirectory,
    },
    Control {
        key: "Esc",
        label: "return",
        action: Action::Escape,
    },
];
const FOLLOWER_SYNC_CONTROLS: &[Control] = &[
    Control {
        key: "Enter",
        label: "sync",
        action: Action::Confirm,
    },
    Control {
        key: "Esc",
        label: "skip",
        action: Action::Escape,
    },
];
const DELETE_CONTROLS: &[Control] = &[
    Control {
        key: "Enter",
        label: "delete",
        action: Action::Confirm,
    },
    Control {
        key: "Esc",
        label: "return",
        action: Action::Escape,
    },
];
const RETRY_CONTROLS: &[Control] = &[
    Control {
        key: "Enter",
        label: "retry",
        action: Action::Confirm,
    },
    Control {
        key: "Esc",
        label: "return",
        action: Action::Escape,
    },
];
const RESULT_CONTROLS: &[Control] = &[Control {
    key: "Enter",
    label: "exit",
    action: Action::Confirm,
}];
const FILTER_CONTROLS: &[Control] = &[
    Control {
        key: "Enter",
        label: "apply",
        action: Action::Confirm,
    },
    Control {
        key: "Esc",
        label: "clear",
        action: Action::Escape,
    },
];
const CHOOSER_CONTROLS: &[Control] = &[
    Control {
        key: "Enter",
        label: "stage",
        action: Action::Confirm,
    },
    Control {
        key: "Esc",
        label: "cancel",
        action: Action::Escape,
    },
];
const PROBE_CONTROLS: &[Control] = &[Control {
    key: "Esc",
    label: "cancel",
    action: Action::Escape,
}];

pub(super) fn workspace_controls(model: &Model) -> &'static [Control] {
    match model.workspace {
        Workspace::Target => TARGET_CONTROLS,
        Workspace::Library if model.scope == Scope::Library && model.host_index != 0 => {
            REMOTE_LIBRARY_CONTROLS
        }
        Workspace::Library => LOCAL_LIBRARY_CONTROLS,
    }
}

pub(super) fn overlay_controls(overlay: &Overlay) -> &'static [Control] {
    match overlay {
        Overlay::None | Overlay::Notice(_) => &[],
        Overlay::Welcome => WELCOME_CONTROLS,
        Overlay::Help | Overlay::Details { .. } | Overlay::Diagnostic { .. } => CLOSE_CONTROLS,
        Overlay::Filter => FILTER_CONTROLS,
        Overlay::ConfirmSave | Overlay::ConfirmSaveWarning(_) | Overlay::GuardedConfirmation(_) => {
            PROCEED_RETURN_CONTROLS
        }
        Overlay::DiscardTarget => DISCARD_RETURN_CONTROLS,
        Overlay::DirectoryEditor { .. }
        | Overlay::FollowerEditor(_)
        | Overlay::SourceKeyEditor(_) => APPLY_CANCEL_CONTROLS,
        Overlay::DirectoryChooser { .. } => CHOOSER_CONTROLS,
        Overlay::FollowerProbe { .. } => PROBE_CONTROLS,
        Overlay::ConfirmFollowerSync { .. } => FOLLOWER_SYNC_CONTROLS,
        Overlay::FollowerSyncResult { .. } => CLOSE_CONTROLS,
        Overlay::ScopeSwitch { .. } => SCOPE_SWITCH_CONTROLS,
        Overlay::LocationEditor { .. } | Overlay::TargetPicker(_) => APPLY_CANCEL_COMPLETE_CONTROLS,
        Overlay::ConfirmDelete => DELETE_CONTROLS,
        Overlay::Busy => RETRY_CONTROLS,
        Overlay::Result(_) => RESULT_CONTROLS,
    }
}
