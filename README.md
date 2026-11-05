# Mem Reduct — Rust edition

A small desktop utility that watches RAM usage and reduces memory pressure on
demand. It shows live memory usage in a window and a tray icon, cleans memory
by hand or automatically, and remembers its settings. Windows and Linux.

This is an independent rebuild of the well-known Mem Reduct program, written
from scratch in Rust (eframe/egui). It does not contain any code, images or
assets from the original.

## Features

* Live usage display: physical memory and virtual memory, with colour-coded
  bar (green below 65 %, amber below 85 %, red above).
* **Clean memory now**: reduces the working set of every accessible process
  (Windows) and/or trims the page cache (Linux).
* Optional extra layers on Windows: empty all working sets at once, purge the
  standby list. These need administrator rights.
* Autoreduction: clean automatically when usage passes a threshold (50–95 %).
* Periodic cleaning: clean every N minutes.
* System tray icon with a colour ring and tooltip; menu: clean, show window,
  toggle autoreduction, exit. (Windows; not yet on Linux.)
* Closing to tray instead of quitting.
* Settings are saved in the user's config directory:
  `%APPDATA%\memreduct-rs\settings.json` (Windows) or
  `$XDG_CONFIG_HOME/memreduct-rs/settings.json` (Linux).

## Build

```
cargo build --release
```

## Run

```
memreduct.exe                 # Windows
target/release/memreduct      # Linux
```

On Windows, run as administrator for the system-wide memory layers.
On Linux, dropping caches needs root rights (run with sudo).

### Screenshot mode (for automated checks)

```
memreduct --screenshot shot.png
```

Paints about 120 frames, saves the window picture to `shot.png` and exits.

## Linux AppImage

Run `packaging/build-appimage.sh` with `linuxdeploy-x86_64.AppImage`
downloaded (see the script header), or push a `v*` tag and the GitHub Actions
workflow builds a Windows exe and a Linux AppImage for you.

## License

GPL-3.0-or-later, in step with the original program's license.
