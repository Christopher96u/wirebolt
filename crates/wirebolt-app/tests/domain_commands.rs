use std::collections::HashSet;
use wirebolt_app::domain::commands::*;

#[test]
fn command_catalog_has_unique_stable_ids_and_typed_shortcuts() {
    let context = CommandContext::default();
    let ids: HashSet<_> = WorkspaceCommand::ALL.iter().map(|c| c.id()).collect();
    assert_eq!(ids.len(), 31);
    let palette = WorkspaceCommand::CommandPalette.descriptor(&context);
    assert_eq!(palette.title, "Command Palette…");
    let shortcut = palette.shortcut.unwrap();
    assert_eq!(shortcut.key, ShortcutKey::Character { value: "k".into() });
    assert_eq!(shortcut.modifiers, [ShortcutModifier::Command]);
    assert_eq!(shortcut.label, "⌘K");
    assert_eq!(
        WorkspaceCommand::Back
            .descriptor(&context)
            .shortcut
            .unwrap()
            .key,
        ShortcutKey::LeftArrow
    );
    assert_eq!(
        WorkspaceCommand::Forward
            .descriptor(&context)
            .shortcut
            .unwrap()
            .label,
        "⌃⌘→"
    );
    assert_eq!(
        WorkspaceCommand::RequestBody
            .descriptor(&context)
            .shortcut
            .unwrap()
            .label,
        "⌥⌘3"
    );
    assert_eq!(
        WorkspaceCommand::SplitRight
            .descriptor(&context)
            .shortcut
            .unwrap()
            .label,
        "⇧⌘D"
    );
    for command in WorkspaceCommand::ALL {
        if let Some(shortcut) = command.descriptor(&context).shortcut {
            assert_eq!(
                shortcut.modifiers.len(),
                shortcut.modifiers.iter().collect::<HashSet<_>>().len()
            );
        }
    }
}

#[test]
fn command_eligibility_and_dynamic_labels_share_one_context() {
    let mut context = CommandContext::default();
    for command in [
        WorkspaceCommand::CommandPalette,
        WorkspaceCommand::Save,
        WorkspaceCommand::Back,
        WorkspaceCommand::Forward,
        WorkspaceCommand::NextTab,
        WorkspaceCommand::PreviousTab,
        WorkspaceCommand::ResponseBody,
    ] {
        assert!(command.descriptor(&context).unavailable_reason.is_some());
    }
    context.has_session = true;
    context.has_name = true;
    context.has_collections = true;
    context.tab_count = 2;
    context.can_open_palette = true;
    for command in [
        WorkspaceCommand::Save,
        WorkspaceCommand::NextTab,
        WorkspaceCommand::CommandPalette,
    ] {
        assert!(command.descriptor(&context).unavailable_reason.is_none());
    }
    context.sidebar_hidden = true;
    assert_eq!(
        WorkspaceCommand::ToggleSidebar.descriptor(&context).title,
        "Show Sidebar"
    );
    context.web_socket = true;
    assert_eq!(
        WorkspaceCommand::RequestBody.descriptor(&context).title,
        "Message"
    );
    for command in [
        WorkspaceCommand::ResponseHeaders,
        WorkspaceCommand::ResponseBody,
        WorkspaceCommand::ResponseCookies,
        WorkspaceCommand::ResponseRaw,
        WorkspaceCommand::ResponseRequest,
    ] {
        assert_eq!(
            command.descriptor(&context).unavailable_reason.as_deref(),
            Some("Open an HTTP request first.")
        );
    }
    for command in [WorkspaceCommand::Settings, WorkspaceCommand::Help] {
        assert!(
            command
                .descriptor(&CommandContext::default())
                .unavailable_reason
                .is_none()
        );
    }
}
