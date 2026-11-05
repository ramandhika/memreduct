#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod config;
mod shot;
mod shotctl;
mod system;
#[cfg(windows)]
mod tray;
#[cfg(not(windows))]
mod tray;

use eframe::egui;

fn parse_flags() -> Option<String> {
    let mut args = std::env::args().skip(1);
    let mut screenshot = None;
    while let Some(arg) = args.next() {
        if let Some(path) = arg.strip_prefix("--screenshot=") {
            screenshot = Some(path.to_owned());
        } else if arg == "--screenshot" {
            screenshot = args.next();
        }
    }
    screenshot
}

/// `--clean`: run one cleaning pass and print a report, then exit.
fn run_clean_once() {
    let settings = config::Settings::load();
    let options = system::CleanOptions {
        working_sets: settings.clean_working_sets,
        system_working_sets: settings.clean_system_working_sets,
        standby: settings.clean_standby,
    };
    let mut report = system::CleanReport::default();
    system::clean(&options, &mut report);
    if report.processes_checked > 0 {
        println!(
            "working sets: {} of {} processes reduced",
            report.processes_reduced, report.processes_checked
        );
    }
    for ok in &report.system_ok {
        println!("ok: {ok}");
    }
    for fail in &report.system_fail {
        println!("failed: {fail}");
    }
}

fn main() -> Result<(), eframe::Error> {
    if std::env::args().any(|a| a == "--clean") {
        run_clean_once();
        return Ok(());
    }

    let screenshot_path = parse_flags();

    let viewport = egui::ViewportBuilder::default()
        .with_inner_size([420.0, 460.0])
        .with_min_inner_size([340.0, 360.0])
        .with_icon(egui::IconData {
            width: 32,
            height: 32,
            rgba: app_icon_rgba(),
        });

    let native_options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    eframe::run_native(
        "Mem Reduct",
        native_options,
        Box::new(|cc| Ok(Box::new(app::App::new(cc, screenshot_path)))),
    )
}

/// A neutral window icon: a rounded square with a filled ring.
fn app_icon_rgba() -> Vec<u8> {
    let w = 32usize;
    let h = 32usize;
    let mut rgba = vec![0u8; w * h * 4];
    let cx = (w - 1) as f32 / 2.0;
    let cy = (h - 1) as f32 / 2.0;
    let r_outer = 13.0;
    let r_inner = 8.5;
    for y in 0..h {
        for x in 0..w {
            let dx = x as f32 - cx;
            let dy = y as f32 - cy;
            let dist = (dx * dx + dy * dy).sqrt();
            let inside_rounded = x >= 2 && x < w - 2 && y >= 2 && y < h - 2;
            let color = if dist <= r_outer && dist >= r_inner {
                [0x55, 0xC3, 0x65]
            } else if inside_rounded {
                [0x2A, 0x2E, 0x35]
            } else {
                [0, 0, 0]
            };
            let i = (y * w + x) * 4;
            let aa = if dist <= r_outer + 1.0 && dist >= r_inner - 1.0 && color != [0, 0, 0] {
                255u8
            } else if color != [0, 0, 0] {
                255u8
            } else {
                0u8
            };
            rgba[i] = color[0];
            rgba[i + 1] = color[1];
            rgba[i + 2] = color[2];
            rgba[i + 3] = aa;
        }
    }
    rgba
}
