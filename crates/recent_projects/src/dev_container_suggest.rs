use crate::RemoteServerProjects;
use db::kvp::KEY_VALUE_STORE;
use dev_container::{DevContainerContext, find_configs_in_snapshot, find_devcontainer_configs};
use gpui::{SharedString, Window};
use project::{Project, WorktreeId};
use std::sync::LazyLock;
use ui::prelude::*;
use util::rel_path::RelPath;
use workspace::Workspace;
use workspace::notifications::NotificationId;
use workspace::notifications::simple_message_notification::MessageNotification;
use worktree::UpdatedEntriesSet;

const DEV_CONTAINER_SUGGEST_KEY: &str = "dev_container_suggest_dismissed";

fn devcontainer_dir_path() -> &'static RelPath {
    static PATH: LazyLock<&'static RelPath> =
        LazyLock::new(|| RelPath::unix(".devcontainer").expect("valid path"));
    *PATH
}

fn devcontainer_json_path() -> &'static RelPath {
    static PATH: LazyLock<&'static RelPath> =
        LazyLock::new(|| RelPath::unix(".devcontainer.json").expect("valid path"));
    *PATH
}

fn project_devcontainer_key(project_path: &str) -> String {
    format!("{}_{}", DEV_CONTAINER_SUGGEST_KEY, project_path)
}

pub fn suggest_on_worktree_updated(
    workspace: &mut Workspace,
    worktree_id: WorktreeId,
    updated_entries: &UpdatedEntriesSet,
    project: &gpui::Entity<Project>,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let cli_auto_open = workspace.open_in_dev_container();

    let devcontainer_updated = updated_entries.iter().any(|(path, _, _)| {
        path.as_ref() == devcontainer_dir_path() || path.as_ref() == devcontainer_json_path()
    });

    if !devcontainer_updated && !cli_auto_open {
        return;
    }

    let Some(worktree) = project.read(cx).worktree_for_id(worktree_id, cx) else {
        return;
    };

    let worktree = worktree.read(cx);

    if !worktree.is_local() {
        return;
    }

    let has_configs = !find_configs_in_snapshot(worktree).is_empty();

    if cli_auto_open {
        if has_configs {
            workspace.set_open_in_dev_container(false);
            cx.on_next_frame(window, move |workspace, window, cx| {
                if !workspace.project().read(cx).is_local() {
                    return;
                }

                let fs = workspace.project().read(cx).fs().clone();
                let configs = find_devcontainer_configs(workspace, cx);
                let app_state = workspace.app_state().clone();
                let dev_container_context = DevContainerContext::from_workspace(workspace, cx);
                let handle = cx.entity().downgrade();
                workspace.toggle_modal(window, cx, |window, cx| {
                    RemoteServerProjects::new_dev_container(
                        fs,
                        configs,
                        app_state,
                        dev_container_context,
                        window,
                        handle,
                        cx,
                    )
                });
            });
            return;
        }

        let scan_complete = worktree.completed_scan_id() >= worktree.scan_id();
        if scan_complete {
            workspace.set_open_in_dev_container(false);
            log::warn!(
                "--dev-container: no devcontainer configuration found in project"
            );
        }
        return;
    }

    if !has_configs {
        return;
    }

    let abs_path = worktree.abs_path();
    let project_path = abs_path.to_string_lossy().to_string();
    let key_for_dismiss = project_devcontainer_key(&project_path);

    let already_dismissed = KEY_VALUE_STORE
        .read_kvp(&key_for_dismiss)
        .ok()
        .flatten()
        .is_some();

    if already_dismissed {
        return;
    }

    cx.on_next_frame(window, move |workspace, _window, cx| {
        struct DevContainerSuggestionNotification;

        let notification_id = NotificationId::composite::<DevContainerSuggestionNotification>(
            SharedString::from(project_path.clone()),
        );

        workspace.show_notification(notification_id, cx, |cx| {
            cx.new(move |cx| {
                MessageNotification::new(
                    "This project contains a Dev Container configuration file. Would you like to re-open it in a container?",
                    cx,
                )
                .primary_message("Yes, Open in Container")
                .primary_icon(IconName::Check)
                .primary_icon_color(Color::Success)
                .primary_on_click({
                    move |window, cx| {
                        window.dispatch_action(Box::new(zed_actions::OpenDevContainer), cx);
                    }
                })
                .secondary_message("Don't Show Again")
                .secondary_icon(IconName::Close)
                .secondary_icon_color(Color::Error)
                .secondary_on_click({
                    move |_window, cx| {
                        let key = key_for_dismiss.clone();
                        db::write_and_log(cx, move || {
                            KEY_VALUE_STORE.write_kvp(key, "dismissed".to_string())
                        });
                    }
                })
            })
        });
    });
}
