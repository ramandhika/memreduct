//! System tray (Windows: tray-icon; Linux: KDE/freedesktop StatusNotifierItem).
//!
//! Windows: the tray is created on the main thread and menu events are polled
//! from the UI update loop through the global `MenuEvent` channel.
//!
//! Linux: the tray registers itself on the session bus as a StatusNotifierItem,
//! which the KDE status bar (system tray) displays. The icon is drawn with the
//! current memory percentage right on it, and refreshes as usage changes. Menu
//! actions arrive on a channel that the UI loop polls.

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TrayAction {
    Clean,
    Show,
    Exit,
    /// Toggle automatic cleaning; sent from the KDE tray menu.
    #[allow(dead_code)] // constructed by the Linux/KDE tray only
    ToggleAuto,
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

pub use imp::Tray;

#[cfg(windows)]
mod imp {
    use super::TrayAction;

    pub struct Tray {
        _tray: tray_icon::TrayIcon,
        _menu_auto: tray_icon::menu::CheckMenuItem,
        _menu_clean: tray_icon::menu::MenuItem,
        _menu_show: tray_icon::menu::MenuItem,
        _menu_exit: tray_icon::menu::MenuItem,
        last_icon_color: [u8; 3],
    }

    impl Tray {
        pub fn new(
            percent: u32,
            color: [u8; 3],
            tooltip: &str,
            autoreduce: bool,
        ) -> Result<Tray, String> {
            let _ = percent;
            let menu_clean =
                tray_icon::menu::MenuItem::with_id("clean", "Clean memory now", true, None);
            let menu_show =
                tray_icon::menu::MenuItem::with_id("show", "Show window", true, None);
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

        /// Poll menu events; menu ids are forwarded through the channel we gave
        /// the global menu-event listener, so ids map straight onto actions.
        pub fn poll(&self) -> Option<TrayAction> {
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

        pub fn update_look(&self, percent: u32, color: [u8; 3], tooltip: &str) {
            let _ = percent;
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
}

#[cfg(not(windows))]
mod imp {
    use super::{color_for, TrayAction};
    use ksni::blocking::TrayMethods as _;
    use std::sync::{mpsc, Arc, Mutex};

    pub struct Tray {
        rx: Mutex<mpsc::Receiver<TrayAction>>,
        handle: Arc<Mutex<Option<ksni::blocking::Handle<KdeTray>>>>,
        last_sent: Mutex<([u8; 3], u32)>,
    }

    impl Tray {
        pub fn new(
            percent: u32,
            color: [u8; 3],
            tooltip: &str,
            autoreduce: bool,
        ) -> Result<Tray, String> {
            let (tx, rx) = mpsc::channel();
            let handle_slot: Arc<Mutex<Option<ksni::blocking::Handle<KdeTray>>>> =
                Arc::new(Mutex::new(None));

            let slot = Arc::clone(&handle_slot);
            let info = tooltip.to_owned();
            std::thread::Builder::new()
                .name("kde-tray".into())
                .spawn(move || {
                    let tray = KdeTray {
                        percent,
                        color,
                        autoreduce,
                        info,
                        tx,
                    };
                    match tray.spawn() {
                        Ok(handle) => {
                            *slot.lock().unwrap() = Some(handle);
                        }
                        // No SNI host (no desktop session, GNOME without an
                        // extension, …): the app keeps working without a tray.
                        Err(_why) => {}
                    }
                })
                .map_err(|e| format!("tray thread: {e}"))?;

            Ok(Tray {
                rx: Mutex::new(rx),
                handle: handle_slot,
                last_sent: Mutex::new(([0, 0, 0], u32::MAX)),
            })
        }

        pub fn poll(&self) -> Option<TrayAction> {
            if let Ok(rx) = self.rx.lock() {
                rx.try_iter().next()
            } else {
                None
            }
        }

        pub fn update_look(&self, percent: u32, color: [u8; 3], tooltip: &str) {
            let mut last = match self.last_sent.lock() {
                Ok(last) => last,
                Err(_) => return,
            };
            if *last == (color, percent) {
                return; // nothing new for the status bar to draw
            }
            *last = (color, percent);
            drop(last);

            if let Ok(slot) = self.handle.lock() {
                if let Some(handle) = &*slot {
                    let info = tooltip.to_owned();
                    handle.update(move |tray: &mut KdeTray| {
                        tray.percent = percent;
                        tray.color = color;
                        tray.info = info;
                    });
                }
            }
        }

        pub fn set_checked_auto(&self, on: bool) {
            if let Ok(slot) = self.handle.lock() {
                if let Some(handle) = &*slot {
                    handle.update(|tray: &mut KdeTray| tray.autoreduce = on);
                }
            }
        }
    }

    struct KdeTray {
        percent: u32,
        color: [u8; 3],
        autoreduce: bool,
        /// Tooltip text: usage in GB plus a note on autoreduction.
        info: String,
        tx: mpsc::Sender<TrayAction>,
    }

    impl ksni::Tray for KdeTray {
        fn id(&self) -> String {
            "memreduct".into()
        }

        fn category(&self) -> ksni::Category {
            ksni::Category::SystemServices
        }

        fn status(&self) -> ksni::Status {
            ksni::Status::Active
        }

        fn title(&self) -> String {
            format!("Mem Reduct — {}% memory used", self.percent)
        }

        fn icon_pixmap(&self) -> Vec<ksni::Icon> {
            vec![icon_with_percent(self.percent)]
        }

        fn tool_tip(&self) -> ksni::ToolTip {
            ksni::ToolTip {
                icon_name: String::new(),
                icon_pixmap: vec![icon_with_percent(self.percent)],
                title: format!("Mem Reduct — {}%", self.percent),
                description: self.info.clone(),
            }
        }

        /// Left click on the status bar entry shows the window.
        fn activate(&mut self, _x: i32, _y: i32) {
            let _ = self.tx.send(TrayAction::Show);
        }

        fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
            use ksni::menu::*;
            vec![
                StandardItem {
                    label: "Clean memory now".into(),
                    activate: Box::new(|this: &mut Self| {
                        let _ = this.tx.send(TrayAction::Clean);
                    }),
                    ..Default::default()
                }
                .into(),
                StandardItem {
                    label: "Show window".into(),
                    activate: Box::new(|this: &mut Self| {
                        let _ = this.tx.send(TrayAction::Show);
                    }),
                    ..Default::default()
                }
                .into(),
                CheckmarkItem {
                    label: "Autoreduction".into(),
                    checked: self.autoreduce,
                    activate: Box::new(|this: &mut Self| {
                        let _ = this.tx.send(TrayAction::ToggleAuto);
                    }),
                    ..Default::default()
                }
                .into(),
                MenuItem::Separator,
                StandardItem {
                    label: "Exit".into(),
                    activate: Box::new(|this: &mut Self| {
                        let _ = this.tx.send(TrayAction::Exit);
                    }),
                    ..Default::default()
                }
                .into(),
            ]
        }
    }

    /// 48x48 icon: the usage percentage written straight onto the icon, in the
    /// state colour on a dark halo, inside a colour-coded ring. Thus the
    /// percentage is visible directly on the KDE status bar, without any fonts.
    fn icon_with_percent(percent: u32) -> ksni::Icon {
        const SIZE: i32 = 48;
        const CX: f32 = (SIZE - 1) as f32 / 2.0;
        const R_OUTER: f32 = 22.5;
        const R_INNER: f32 = 19.0;
        let p = percent.clamp(0, 100);

        const ZERO: [&str; 5] = ["111", "101", "101", "101", "111"];
        const ONE: [&str; 5] = ["010", "110", "010", "010", "010"];
        const TWO: [&str; 5] = ["111", "001", "111", "100", "111"];
        const THREE: [&str; 5] = ["111", "001", "111", "001", "111"];
        const FOUR: [&str; 5] = ["101", "101", "111", "001", "001"];
        const FIVE: [&str; 5] = ["111", "100", "111", "001", "111"];
        const SIX: [&str; 5] = ["111", "100", "111", "101", "111"];
        const SEVEN: [&str; 5] = ["111", "001", "001", "001", "001"];
        const EIGHT: [&str; 5] = ["111", "101", "111", "101", "111"];
        const NINE: [&str; 5] = ["111", "101", "111", "001", "111"];
        const PERCENT: [&str; 5] = ["101", "001", "010", "100", "101"];

        const DIGITS: [&[&str; 5]; 10] = [
            &ZERO, &ONE, &TWO, &THREE, &FOUR, &FIVE, &SIX, &SEVEN, &EIGHT, &NINE,
        ];

        let text = format!("{p}%");
        let scale: i32 = if text.chars().count() <= 2 { 3 } else { 2 };
        let glyph = |c: char| match c {
            '%' => &PERCENT,
            d if d.is_ascii_digit() => &DIGITS[d as usize - b'0' as usize],
            _ => &PERCENT,
        };
        let text_w: i32 = {
            let mut w = 0;
            for c in text.chars() {
                w += glyph(c)[0].len() as i32;
            }
            w += text.chars().count() as i32 - 1; // 1px gaps (unscaled)
            w * scale
        };
        let text_h = 5 * scale;
        let start_x = ((SIZE - text_w) / 2).max(0);
        let start_y = ((SIZE - text_h) / 2).max(0);

        let mut buf = vec![0u8; (SIZE * SIZE * 3) as usize]; // RGB, data[0]=0 marks transparent

        let color = color_for(p);
        let mut put = |x: i32, y: i32, rgb: [u8; 3]| {
            if x >= 0 && x < SIZE && y >= 0 && y < SIZE {
                let i = ((y * SIZE + x) * 3) as usize;
                buf[i] = 1;
                buf[i + 1] = rgb[0];
                buf[i + 2] = rgb[1];
                buf[i + 3] = rgb[2];
            }
        };

        // Ring, state-coloured so the panel shows load at a glance.
        for y in 0..SIZE {
            for x in 0..SIZE {
                let dx = x as f32 - CX;
                let dy = y as f32 - CX;
                let dist = (dx * dx + dy * dy).sqrt();
                if dist <= R_OUTER && dist >= R_INNER {
                    put(x, y, color);
                }
            }
        }

        // Halo pass first (dark outline), then the glyphs themselves, so the
        // digits stay readable on light and dark panels alike.
        for halo in [true, false] {
            let mut cursor = start_x;
            for c in text.chars() {
                let rows = glyph(c);
                let w = rows[0].len() as i32;
                for (py, row) in rows.iter().enumerate() {
                    for (px, ch) in row.chars().enumerate() {
                        if ch != '1' {
                            continue;
                        }
                        let x0 = cursor + px as i32 * scale;
                        let y0 = start_y + py as i32 * scale;
                        if halo {
                            // 1px dark outline around the whole scaled cell.
                            for hy in -1..=scale {
                                for hx in -1..=scale {
                                    if (0..scale).contains(&hx) && (0..scale).contains(&hy) {
                                        continue;
                                    }
                                    put(x0 + hx, y0 + hy, [0, 0, 0]);
                                }
                            }
                        } else {
                            for hy in 0..scale {
                                for hx in 0..scale {
                                    put(x0 + hx, y0 + hy, [255, 255, 255]);
                                }
                            }
                        }
                    }
                }
                cursor += (w + 1) * scale;
            }
        }

        // SNI pixmap data is ARGB32, big-endian byte order: A, R, G, B.
        let mut data = Vec::with_capacity((SIZE * SIZE * 4) as usize);
        for i in (0..buf.len()).step_by(3) {
            let a = buf[i];
            data.push(if a != 0 { 255u8 } else { 0u8 });
            data.extend_from_slice(&buf[i + 1..i + 3]);
        }

        ksni::Icon {
            width: SIZE,
            height: SIZE,
            data,
        }
    }
}
