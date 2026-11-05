//! The main window: usage overview, manual/automatic cleaning, tray and about.

use crate::config::Settings;
use crate::system::{CleanOptions, CleanReport, MemoryStats};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq)]
enum Page {
    Main,
    Reduction,
    Tray,
    About,
}

struct Shared {
    stats: Mutex<MemoryStats>,
    running: AtomicBool,
    last_clean: Mutex<Option<Instant>>,
    last_report: Mutex<Option<CleanReport>>,
    settings: Mutex<Settings>,
}

pub struct App {
    shared: Arc<Shared>,
    settings: Settings,
    page: Page,
    elevated: bool,
    tray: Option<crate::tray::Tray>,
    screenshot: Option<crate::shotctl::ShotCtl>,
    exit_requested: bool,
    ui_ctx: egui::Context,
}

impl App {
    pub fn new(cc: &eframe::CreationContext, screenshot_path: Option<String>) -> App {
        let settings = Settings::load();
        let shared = Arc::new(Shared {
            stats: Mutex::new(crate::system::poll_stats()),
            running: AtomicBool::new(false),
            last_clean: Mutex::new(None),
            last_report: Mutex::new(None),
            settings: Mutex::new(settings.clone()),
        });

        let ui_ctx = cc.egui_ctx.clone();
        // The app is dark-styled; don't follow the system light/dark setting.
        ui_ctx.set_theme(egui::Theme::Dark);

        let screenshot = screenshot_path.map(crate::shotctl::ShotCtl::new);

        // Background: refresh stats once a second and run automatic reduction.
        std::thread::spawn({
            let shared = Arc::clone(&shared);
            let ctx = ui_ctx.clone();
            move || poll_loop(ctx, shared)
        });

        let elevated = crate::system::is_elevated();

        #[cfg(windows)]
        let tray = {
            if settings.show_tray && screenshot.is_none() {
                let stats = *shared.stats.lock().unwrap();
                let color = crate::tray::color_for(stats.load_percent);
                let tooltip = tray_tooltip(stats);
                crate::tray::Tray::new(stats.load_percent, color, &tooltip, settings.autoreduce)
                    .ok()
            } else {
                None
            }
        };
        #[cfg(not(windows))]
        let tray = {
            if settings.show_tray && screenshot.is_none() {
                let stats = *shared.stats.lock().unwrap();
                crate::tray::Tray::new(
                    stats.load_percent,
                    crate::tray::color_for(stats.load_percent),
                    &tray_tooltip(stats),
                    settings.autoreduce,
                )
                .ok()
            } else {
                None
            }
        };

        App {
            shared,
            settings,
            page: Page::Main,
            elevated,
            tray,
            screenshot,
            exit_requested: false,
            ui_ctx,
        }
    }

    fn stats(&self) -> MemoryStats {
        *self.shared.stats.lock().unwrap()
    }

    fn persist(&mut self) {
        *self.shared.settings.lock().unwrap() = self.settings.clone();
        self.settings.save();
        if let Some(tray) = &self.tray {
            tray.set_checked_auto(self.settings.autoreduce);
        }
    }

    fn start_clean(&mut self) {
        if self.shared.running.swap(true, Ordering::SeqCst) {
            return;
        }
        let options = CleanOptions {
            working_sets: self.settings.clean_working_sets,
            system_working_sets: self.settings.clean_system_working_sets,
            standby: self.settings.clean_standby,
        };
        let shared = Arc::clone(&self.shared);
        let ctx = self.ui_ctx.clone();
        std::thread::spawn(move || {
            let mut report = CleanReport::default();
            crate::system::clean(&options, &mut report);
            *shared.last_report.lock().unwrap() = Some(report);
            *shared.last_clean.lock().unwrap() = Some(Instant::now());
            shared.running.store(false, Ordering::SeqCst);
            ctx.request_repaint();
        });
    }

    fn handle_tray(&mut self, ctx: &egui::Context) {
        let Some(_) = &self.tray else { return };
        if let Some(action) = self.tray.as_ref().unwrap().poll() {
            match action {
                crate::tray::TrayAction::Clean => self.start_clean(),
                crate::tray::TrayAction::Show => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                }
                crate::tray::TrayAction::ToggleAuto => {
                    self.settings.autoreduce = !self.settings.autoreduce;
                    self.persist();
                }
                crate::tray::TrayAction::Exit => {
                    self.exit_requested = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        }
    }

    fn handle_close_to_tray(&mut self, ctx: &egui::Context) {
        if self.settings.close_to_tray
            && !self.exit_requested
            && ctx.input(|i| i.viewport().close_requested())
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        }
    }

    fn handle_screenshot(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        let Some(shot) = &mut self.screenshot else { return };
        shot.frames += 1;
        if shot.frames < crate::shotctl::FRAMES {
            ctx.request_repaint_after(Duration::from_millis(16));
            return;
        }
        if !shot.saved {
            if let Some((img, dims)) = crate::shot::capture(ctx, frame) {
                // Accept once the picture clearly contains painted UI (not a
                // blank framebuffer) and the window has settled at the same
                // size for a few frames in a row.
                if crate::shot::looks_painted(&img) {
                    if shot.last_dims == Some(dims) {
                        shot.ok_streak += 1;
                    } else {
                        shot.ok_streak = 1;
                    }
                    shot.last_dims = Some(dims);
                    if shot.ok_streak >= 3 {
                        match crate::shot::save(&img, &shot.path) {
                            Ok(()) => shot.saved = true,
                            Err(e) => eprintln!("screenshot failed: {e}"),
                        }
                    }
                } else {
                    shot.ok_streak = 0;
                }
            }
        }
        if shot.saved || shot.frames > 20 * crate::shotctl::FRAMES {
            std::process::exit(0);
        }
        ctx.request_repaint_after(Duration::from_millis(16));
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        self.handle_screenshot(ctx, frame);
        self.handle_tray(ctx);
        self.handle_close_to_tray(ctx);
        draw_ui(self, ctx);
    }
}

/// Background thread: poll stats every second, trigger autoreduction.
fn poll_loop(ctx: egui::Context, shared: Arc<Shared>) {
    let boot = Instant::now();
    let mut last_trigger: Option<Instant> = None;
    loop {
        std::thread::sleep(Duration::from_secs(1));
        let stats = crate::system::poll_stats();
        *shared.stats.lock().unwrap() = stats;

        let settings = shared.settings.lock().unwrap().clone();
        if shared.running.load(Ordering::SeqCst) {
            ctx.request_repaint();
            continue;
        }

        let threshold_hit = settings.autoreduce
            && settings.threshold_hi as u64 <= stats.load_percent as u64
            && last_trigger
                .map(|t| t.elapsed() > Duration::from_secs(10))
                .unwrap_or(true);

        let periodic_hit = settings.interval_min > 0
            && last_trigger
                .map(|t| t.elapsed() >= Duration::from_secs(settings.interval_min as u64 * 60))
                .unwrap_or_else(|| boot.elapsed() >= Duration::from_secs(settings.interval_min as u64 * 60));

        if threshold_hit || periodic_hit {
            last_trigger = Some(Instant::now());
            if !shared.running.swap(true, Ordering::SeqCst) {
                let shared2 = Arc::clone(&shared);
                let ctx2 = ctx.clone();
                let options = CleanOptions {
                    working_sets: settings.clean_working_sets,
                    system_working_sets: settings.clean_system_working_sets,
                    standby: settings.clean_standby,
                };
                std::thread::spawn(move || {
                    let mut report = CleanReport::default();
                    crate::system::clean(&options, &mut report);
                    *shared2.last_report.lock().unwrap() = Some(report);
                    *shared2.last_clean.lock().unwrap() = Some(Instant::now());
                    shared2.running.store(false, Ordering::SeqCst);
                    ctx2.request_repaint();
                });
            }
        }

        ctx.request_repaint();
    }
}

fn tray_tooltip(stats: MemoryStats) -> String {
    format!(
        "Mem Reduct — used {}% ({:.1} / {:.1} GB)",
        stats.load_percent,
        gib(stats.used_phys()),
        gib(stats.total_phys),
    )
}

fn gib(bytes: u64) -> f64 {
    bytes as f64 / (1024.0 * 1024.0 * 1024.0)
}

fn fmt_gib(bytes: u64) -> String {
    format!("{:.1} GB", gib(bytes))
}

// ---------------------------------------------------------------- drawing

fn draw_ui(app: &mut App, ctx: &egui::Context) {
    egui::SidePanel::left("nav_panel")
        .exact_width(124.0)
        .resizable(false)
        .show(ctx, |ui| draw_nav(ui, app));

    egui::CentralPanel::default()
        .frame(
            egui::Frame::NONE
                .inner_margin(egui::Margin::same(12))
                .fill(egui::Color32::from_rgb(0x1E, 0x21, 0x26)),
        )
        .show(ctx, |ui| match app.page {
            Page::Main => draw_main(ui, app),
            Page::Reduction => draw_reduction(ui, app),
            Page::Tray => draw_tray_page(ui, app),
            Page::About => draw_about(ui, app),
        });

    // Keep the tray ring and tooltip in step with the newest numbers.
    if let Some(tray) = &app.tray {
        let stats = app.stats();
        tray.update_look(
            stats.load_percent,
            crate::tray::color_for(stats.load_percent),
            &tray_tooltip(stats),
        );
    }
}

fn draw_nav(ui: &mut egui::Ui, app: &mut App) {
    ui.add_space(10.0);
    ui.label(
        egui::RichText::new("Mem Reduct")
            .size(18.0)
            .color(egui::Color32::from_rgb(0xAF, 0xD8, 0xAD)),
    );
    ui.label(
        egui::RichText::new("memory reduction")
            .size(10.0)
            .color(egui::Color32::from_gray(110)),
    );
    ui.add_space(18.0);

    for (page, label) in [
        (Page::Main, "Main"),
        (Page::Reduction, "Reduction"),
        (Page::Tray, "Tray"),
        (Page::About, "About"),
    ] {
        ui.add_space(1.5);
        ui.selectable_value(&mut app.page, page, label);
    }
}

fn usage_color(percent: u32) -> egui::Color32 {
    if percent >= 85 {
        egui::Color32::from_rgb(0xE5, 0x4F, 0x4F)
    } else if percent >= 65 {
        egui::Color32::from_rgb(0xE8, 0xB4, 0x4A)
    } else {
        egui::Color32::from_rgb(0x55, 0xC3, 0x65)
    }
}

fn usage_bar(ui: &mut egui::Ui, frac: f32, color: egui::Color32, overlay: String) {
    let height = 22.0;
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), height),
        egui::Sense::hover(),
    );
    let painter = ui.painter_at(rect);
    let rounding = egui::CornerRadius::same(5);
    painter.rect_filled(rect.expand2(egui::vec2(0.0, 2.0)), rounding, egui::Color32::from_gray(38));
    let mut fill = rect;
    let fill_w = rect.width() * frac.clamp(0.0, 1.0);
    fill.max.x = fill.min.x + fill_w.max(0.0);
    if fill_w > 4.0 {
        painter.rect_filled(fill, rounding, color);
    }
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        overlay,
        egui::FontId::proportional(12.0),
        egui::Color32::WHITE,
    );
}

fn draw_main(ui: &mut egui::Ui, app: &mut App) {
    let stats = app.stats();

    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Memory usage").heading());
        if app.shared.running.load(Ordering::SeqCst) {
            ui.add_space(10.0);
            ui.add(egui::Spinner::new().size(16.0));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if app.elevated {
                ui.label(
                    egui::RichText::new("administrator")
                        .small()
                        .color(egui::Color32::from_rgb(0x8B, 0xC8, 0xE0)),
                );
            }
        });
    });
    ui.add_space(8.0);

    let color = usage_color(stats.load_percent);
    ui.label(
        egui::RichText::new(format!("{}%", stats.load_percent))
            .size(40.0)
            .strong()
            .color(color),
    );
    usage_bar(
        ui,
        stats.load_percent as f32 / 100.0,
        color,
        format!("{} / {}", fmt_gib(stats.used_phys()), fmt_gib(stats.total_phys)),
    );

    ui.add_space(8.0);
    ui.separator();
    ui.add_space(5.0);

    egui::Grid::new("stats_grid").spacing([12.0, 5.0]).show(ui, |ui| {
        ui.label("Physical memory");
        ui.label(format!(
            "{} used, {} available",
            fmt_gib(stats.used_phys()),
            fmt_gib(stats.avail_phys),
        ));
        ui.end_row();

        ui.label("Virtual memory");
        ui.label(format!(
            "{} used, {} available",
            fmt_gib(stats.used_commit()),
            fmt_gib(stats.avail_commit),
        ));
        ui.end_row();
    });

    ui.add_space(10.0);

    let cleaning = app.shared.running.load(Ordering::SeqCst);
    ui.add_enabled_ui(!cleaning, |ui| {
        let button = egui::Button::new(
            egui::RichText::new(if cleaning { "Cleaning…" } else { "Clean memory now" })
                .size(14.0)
                .strong(),
        )
        .min_size(egui::vec2(ui.available_width(), 32.0));
        if ui.add(button).clicked() {
            app.start_clean();
        }
    });

    ui.add_space(8.0);

    let last = *app.shared.last_clean.lock().unwrap();
    match last {
        Some(t) => {
            let ago = human_ago(t);
            ui.label(format!("Last cleaning: {ago}"));
        }
        None => {
            ui.label("No cleaning performed yet.");
        }
    }

    if app.settings.autoreduce {
        ui.label(format!(
            "Autoreduction: on — cleans when usage reaches {}%",
            app.settings.threshold_hi
        ));
    } else {
        ui.label("Autoreduction: off");
    }

    if let Some(report) = &*app.shared.last_report.lock().unwrap() {
        ui.add_space(10.0);
        egui::CollapsingHeader::new("Details of the last cleaning")
            .show(ui, |ui| {
                if report.processes_checked > 0 {
                    ui.label(format!(
                        "working sets: {} / {} processes",
                        report.processes_reduced, report.processes_checked
                    ));
                }
                for line in &report.system_ok {
                    ui.label(format!("✔ {line}"));
                }
                for line in &report.system_fail {
                    ui.label(format!("✖ {line}"));
                }
            });
    }
}

fn human_ago(t: Instant) -> String {
    let d = t.elapsed();
    if d < Duration::from_secs(60) {
        format!("{} second(s) ago", d.as_secs())
    } else if d < Duration::from_secs(3600) {
        format!("{} minute(s) ago", d.as_secs() / 60)
    } else {
        format!("{} hour(s) ago", d.as_secs() / 3600)
    }
}

fn draw_reduction(ui: &mut egui::Ui, app: &mut App) {
    let mut s = app.settings.clone();
    let mut changed = false;

    ui.heading("Autoreduction");
    ui.add_space(8.0);

    changed |= ui
        .checkbox(&mut s.autoreduce, "Reduce memory automatically")
        .changed();

    ui.add_space(6.0);
    ui.label("Clean when usage reaches:");
    changed |= ui
        .add(
            egui::Slider::new(&mut s.threshold_hi, 50..=95)
                .suffix("%")
                .smart_aim(false),
        )
        .changed();

    ui.add_space(10.0);
    let mut interval_enabled = s.interval_min > 0;
    let mut minutes = if interval_enabled { s.interval_min } else { 30 };
    let mut interval_changed = ui.checkbox(&mut interval_enabled, "Clean regularly every").changed();
    interval_changed |= ui
        .add(
            egui::DragValue::new(&mut minutes)
                .range(1..=240)
                .suffix(" min"),
        )
        .changed();
    let new_interval = if interval_enabled {
        minutes
    } else {
        0
    };
    if new_interval != s.interval_min {
        s.interval_min = new_interval;
        interval_changed = true;
    }
    changed |= interval_changed;

    ui.add_space(10.0);
    ui.heading("Memory layers to clean");
    ui.add_space(6.0);

    #[cfg(windows)]
    {
        changed |= ui
            .checkbox(
                &mut s.clean_working_sets,
                "Working sets of each process",
            )
            .changed();
        changed |= ui
            .add_enabled(
                app.elevated,
                egui::Checkbox::new(&mut s.clean_system_working_sets, "All working sets at once"),
            )
            .changed();
        changed |= ui
            .add_enabled(
                app.elevated,
                egui::Checkbox::new(&mut s.clean_standby, "Standby list"),
            )
            .changed();
        if !app.elevated {
            ui.label(
                egui::RichText::new(
                    "Run as administrator to clean all working sets at once or purge the standby list.",
                )
                .small()
                .weak(),
            );
        }
    }
    #[cfg(not(windows))]
    {
        changed |= ui
            .checkbox(
                &mut s.clean_working_sets,
                "Drop page cache, dentries and inodes",
            )
            .changed();
        changed |= ui
            .checkbox(&mut s.clean_standby, "Also reclaim low-priority pages")
            .changed();
        ui.label(
            egui::RichText::new(
                "Cache dropping needs root rights. Working sets and standby purge are Windows-only layers.",
            )
            .small()
            .weak(),
        );
    }

    if changed {
        app.settings = s;
        app.persist();
    }
}

fn draw_tray_page(ui: &mut egui::Ui, app: &mut App) {
    ui.heading("Tray icon");
    ui.add_space(8.0);

    #[cfg(windows)]
    {
        let mut changed = false;
        if ui
            .checkbox(&mut app.settings.show_tray, "Show tray icon (colour ring with usage)")
            .changed()
        {
            changed = true;
        }
        if ui
            .checkbox(&mut app.settings.close_to_tray, "Closing the window keeps the app in the tray")
            .changed()
        {
            changed = true;
        }
        if changed {
            app.persist();
        }
        ui.add_space(4.0);
        ui.label(
            egui::RichText::new("Show tray / tray creation applies on next start. The icon ring turns amber above 50% and red above 80%.")
                .small()
                .weak(),
        );
        ui.add_space(10.0);
        ui.heading("Menu");
        ui.label("Clean memory now — performs a cleaning.");
        ui.label("Show window — brings the window to the front.");
        ui.label("Autoreduction — toggles automatic cleaning.");
        ui.label("Exit — quits the app completely.");
    }
    #[cfg(not(windows))]
    {
        ui.label(
            egui::RichText::new("The tray icon lives in the KDE status bar (system tray) and shows the current memory percentage right on its icon. Right-click it for the menu.")
                .small()
                .weak(),
        );
        ui.add_space(6.0);
        let changed = ui
            .checkbox(&mut app.settings.close_to_tray, "Closing the window keeps the app in the tray")
            .changed();
        if changed {
            app.persist();
        }
        ui.add_space(10.0);
        ui.heading("Menu");
        ui.label("Clean memory now — performs a cleaning.");
        ui.label("Show window — brings the window to the front.");
        ui.label("Autoreduction — toggles automatic cleaning.");
        ui.label("Exit — quits the app completely.");
        ui.add_space(4.0);
        ui.label(
            egui::RichText::new("Enabling/disabling the tray icon applies on next start.")
                .small()
                .weak(),
        );
    }
}

fn draw_about(ui: &mut egui::Ui, app: &mut App) {
    ui.heading("About");
    ui.add_space(8.0);
    ui.label("Mem Reduct — Rust edition");
    ui.label(egui::RichText::new(concat!("Version ", env!("CARGO_PKG_VERSION"))).weak());
    ui.add_space(10.0);
    ui.label(
        "A small utility that watches RAM usage and reduces memory pressure on demand: \
it trims process working sets and other memory layers, by hand or automatically.",
    );
    ui.add_space(6.0);
    ui.label("This is an independent, rebuilt version, created from the app's public behaviour.");
    ui.add_space(12.0);
    ui.label(format!("Settings file: {}", crate::config::config_file()));
    let _ = app;
}
