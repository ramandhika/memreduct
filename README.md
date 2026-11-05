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
  toggle autoreduction, exit. (Windows; on Linux via the freedesktop
  StatusNotifierItem spec, which KDE Plasma shows in the status bar.)
* Closing to tray instead of quitting.
* Settings are saved in the user's config directory:
  `%APPDATA%\memreduct-rs\settings.json` (Windows) or
  `$XDG_CONFIG_HOME/memreduct-rs/settings.json` (Linux).

## Build on Windows

```
cargo build --release
target\release\memreduct.exe    # run as administrator for the system-wide layers
```

## Build on Linux

Requirements: Rust and a couple of system dev libraries.

1. Install Rust:

   ```
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   source "$HOME/.cargo/env"
   ```

2. Install the build tools and libraries (Debian/Ubuntu/KDE-Neon examples,
   the same names exist on Fedora/Arch: `gcc`, `pkg-config`, `libxkbcommon`,
   `fontconfig`):

   ```
   sudo apt install build-essential pkg-config libxkbcommon-dev libfontconfig1-dev
   ```

3. Build:

   ```
   cargo build --release
   ```

   The binary is `target/release/memreduct`. Start it from a desktop session
   (X11 or Wayland): running it over plain SSH without a display fails with a
   "no display" error.

4. Run it:

   ```
   ./target/release/memreduct
   ```

   Watching memory works as a normal user. Cleaning the page cache writes to
   `/proc/sys/vm/drop_caches`, which only root can do — the app reports
   "drop_caches failed: … (root required?)" if it lacks the rights. To clean
   with the system layers, start it elevated:

   ```
   sudo ./target/release/memreduct
   ```

5. Tray (KDE Plasma): the app registers a StatusNotifierItem icon that shows
   the live memory percentage. Other desktops pick it up if they support the
   same spec (most do; GNOME needs the AppIndicator extension).

### Install on Linux (optional)

```
sudo cp target/release/memreduct /usr/local/bin/
```

## Run

```
target\release\memreduct.exe   # Windows
./target/release/memreduct     # Linux
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
