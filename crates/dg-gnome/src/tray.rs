//! The tray icon on Linux (ARCHITECTURE.md §16.3, L2): a StatusNotifierItem, which
//! Ubuntu's top bar shows through its AppIndicator extension. The menu is the one from
//! Windows: the groups (the one on screen ticked), recover, settings, start at login,
//! exit. Choices are handed to the agent's event loop.

use std::sync::Arc;

use ksni::blocking::TrayMethods;
use ksni::menu::{CheckmarkItem, StandardItem};
use ksni::MenuItem;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrayCommand {
    Apply(Uuid),
    Recover,
    Settings,
    ToggleStartup,
    Exit,
}

/// Menu texts in the user's language.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TrayLabels {
    pub tooltip: String,
    pub no_groups: String,
    pub recover: String,
    pub settings: String,
    pub startup: String,
    pub exit: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TrayModel {
    pub groups: Vec<(Uuid, String)>,
    pub current: Option<Uuid>,
    pub startup: bool,
    pub labels: TrayLabels,
}

type Sender = Arc<dyn Fn(TrayCommand) + Send + Sync>;

struct DgTray {
    model: TrayModel,
    send: Sender,
}

fn item(label: &str, command: TrayCommand) -> MenuItem<DgTray> {
    StandardItem {
        label: label.to_string(),
        activate: Box::new(move |t: &mut DgTray| (t.send)(command.clone())),
        ..Default::default()
    }
    .into()
}

impl ksni::Tray for DgTray {
    // A left click opens the menu, as on Windows.
    const MENU_ON_ACTIVATE: bool = true;

    fn id(&self) -> String {
        "displaygroups".into()
    }

    fn title(&self) -> String {
        self.model.labels.tooltip.clone()
    }

    fn icon_name(&self) -> String {
        "video-display-symbolic".into()
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        let m = &self.model;
        let mut items: Vec<MenuItem<Self>> = Vec::new();
        if m.groups.is_empty() {
            items.push(
                StandardItem {
                    label: m.labels.no_groups.clone(),
                    enabled: false,
                    ..Default::default()
                }
                .into(),
            );
        }
        for (id, name) in &m.groups {
            let id = *id;
            items.push(
                CheckmarkItem {
                    label: name.clone(),
                    checked: m.current == Some(id),
                    activate: Box::new(move |t: &mut DgTray| (t.send)(TrayCommand::Apply(id))),
                    ..Default::default()
                }
                .into(),
            );
        }
        items.push(MenuItem::Separator);
        items.push(item(&m.labels.recover, TrayCommand::Recover));
        items.push(MenuItem::Separator);
        items.push(item(&m.labels.settings, TrayCommand::Settings));
        items.push(
            CheckmarkItem {
                label: m.labels.startup.clone(),
                checked: m.startup,
                activate: Box::new(|t: &mut DgTray| (t.send)(TrayCommand::ToggleStartup)),
                ..Default::default()
            }
            .into(),
        );
        items.push(MenuItem::Separator);
        items.push(item(&m.labels.exit, TrayCommand::Exit));
        items
    }

    fn watcher_offline(&self, reason: ksni::OfflineReason) -> bool {
        // Keep the item: it shows up as soon as the top bar's extension is back.
        tracing::warn!(?reason, "no tray host right now");
        true
    }
}

pub struct TrayHandle(ksni::blocking::Handle<DgTray>);

impl TrayHandle {
    /// Replaces the menu's contents (after a switch or a config change).
    pub fn set(&self, model: TrayModel) {
        self.0.update(|t| {
            if t.model != model {
                t.model = model;
            }
        });
    }
}

/// Adds the tray icon; `send` receives the menu choices (on the tray's own thread).
pub fn spawn(
    model: TrayModel,
    send: impl Fn(TrayCommand) + Send + Sync + 'static,
) -> Result<TrayHandle, String> {
    DgTray {
        model,
        send: Arc::new(send),
    }
    // The agent may start before the top bar at login.
    .assume_sni_available(true)
    .spawn()
    .map(TrayHandle)
    .map_err(|e| e.to_string())
}
