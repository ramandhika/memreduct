//! System tray with a colour-coded icon, tooltip and context menu (Windows).
//!
//! The tray is created on the main thread and menu events are polled from the
//! UI update loop through the global `MenuEvent` channel.

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TrayAction {
    Clean,
    Show,
    Exit,
}

pub struct Tray {
    _tray: tray_icon::TrayIcon,
    _menu_auto: tray_icon::menu::CheckMenuItem,
    _menu_clean: tray_icon::menu::MenuItem,
    _menu_show: tray_icon::menu::MenuItem,
    _menu_exit: tray_icon::menu::MenuItem,
    last_icon_color: [u8; 3],
}

impl Tray {
    pub fn new(color: [u8; 3], tooltip: &str, autoreduce: bool) -> Result<Tray, String> {
        let menu_clean =
            tray_icon::menu::MenuItem::with_id("clean", "Clean memory now", true, None);
        let menu_show = tray_icon::menu::MenuItem::with_id("show", "Show window", true, None);
        let menu_auto = tray_icon::menu::CheckMenuItem::with_id(
            "auto",
            "Autoreduction",
            true,
            autoreduce,
            None,
        );
        let menu_exit = tray_icon::menu::MenuItem::with_id("exit", "Exit", true, None);

        let menu = tray_icon::menu::Menu::new();
        menu.append_items(&[
            &menu_clean,
            &menu_show,
            &menu_auto,
            &tray_icon::menu::PredefinedMenuItem::separator(),
            &menu_exit,
        ])
        .map_err(|e| format!("menu: {e}"))?;

        let icon = icon_from_color(color).map_err(|e| format!("icon: {e}"))?;

        let tray = tray_icon::TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip(tooltip)
            .with_icon(icon)
            .build()
            .map_err(|e| format!("tray: {e}"))?;

        Ok(Tray {
            _tray: tray,
            _menu_auto: menu_auto,
            _menu_clean: menu_clean,
            _menu_show: menu_show,
            _menu_exit: menu_exit,
            last_icon_color: color,
        })
    }

    /// Poll menu events; menu ids are forwarded through the channel we gave the
    /// global menu-event listener, so ids map straight onto actions.
    pub fn poll(&self) -> Option<TrayAction> {
        // We didn't register a custom listener: menu events arrive on the
        // global channel with ids we set above.
        let receiver = tray_icon::menu::MenuEvent::receiver();
        for event in receiver.try_iter() {
            let action = match event.id().as_ref() {
                "clean" => Some(TrayAction::Clean),
                "show" => Some(TrayAction::Show),
                "exit" => Some(TrayAction::Exit),
                _ => None,
            };
            if let Some(action) = action {
                return Some(action);
            }
        }
        None
    }

    pub fn update_look(&self, color: [u8; 3], tooltip: &str) {
        if color != self.last_icon_color {
            if let Ok(icon) = icon_from_color(color) {
                let _ = self._tray.set_icon(Some(icon));
            }
        }
        let _ = self._tray.set_tooltip(Some(tooltip));
    }

    pub fn set_checked_auto(&self, on: bool) {
        let _ = self._menu_auto.set_checked(on);
    }
}

pub fn color_for(usage: u32) -> [u8; 3] {
    if usage >= 80 {
        [0xE5, 0x53, 0x53] // red: high
    } else if usage >= 50 {
        [0xF2, 0xB1, 0x4D] // amber: moderate
    } else {
        [0x60, 0xC9, 0x6C] // green: comfortable
    }
}

/// A simple filled ring, neutral by design (no logos).
fn icon_from_color(color: [u8; 3]) -> Result<tray_icon::Icon, String> {
    let size: u32 = 32;
    let cx = size as f32 / 2.0;
    let cy = size as f32 / 2.0;
    let r_outer = size as f32 / 2.0 - 2.0;
    let r_inner = 8.0;
    let mut rgba = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let dx = x as f32 + 0.5 - cx;
            let dy = y as f32 + 0.5 - cy;
            let dist = (dx * dx + dy * dy).sqrt();
            let hit = dist <= r_outer && dist >= r_inner;
            if hit {
                rgba.extend_from_slice(&[color[0], color[1], color[2], 255]);
            } else {
                rgba.extend_from_slice(&[0, 0, 0, 0]);
            }
        }
    }
    tray_icon::Icon::from_rgba(rgba, size, size).map_err(|e| e.to_string())
}
