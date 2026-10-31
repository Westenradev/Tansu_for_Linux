# Tansu for Linux — Handover Document

Written in ASL-ESL100 style (Alternative Sign Language / Easy-to-read English, short sentences, one idea per sentence, active voice).

## What this project is

Tansu for Linux is a port of the Haiku OS app Tansu.
The Haiku app is a floating shelf. You park files, text, and images on it.
You drag them back out later.

This Linux version uses Rust. It uses GTK4 and libadwaita. It follows the GNOME Human Interface Guidelines. The app ID is `io.github.Westenradev.Tansu`. It targets Fedora GNOME. It is built as a Flatpak for Flathub. An RPM spec also exists.

## Repo

GitHub repo: `Westenradev/Tansu_for_Linux`. It must be public for Flathub.

## Project layout

- `core/` — crate `tansu-core`. Pure data model. No GUI. Unit tests live here.
- `app/` — crate `tansu`. The GTK4 app. Files: `main.rs`, `window.rs`, `rows.rs`.
- `data/` — desktop file, AppStream metainfo, SVG icon.
- `io.github.Westenradev.Tansu.yml` — Flatpak manifest.
- `cargo-sources.json` — pinned crates for the offline Flatpak build. Generated. Do not edit by hand.
- `packaging/tansu.spec` — Fedora RPM spec.
- `.github/workflows/flatpak.yml` — CI build.
- `agents.md` — working rules for coding agents.

## How to build on Fedora

```
sudo dnf install flatpak-builder
flatpak remote-add --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo
flatpak install flathub org.gnome.Sdk//51 org.gnome.Platform//51
flatpak install --user flathub org.freedesktop.Sdk.Extension.rust-stable//26.08
git clone https://github.com/Westenradev/Tansu_for_Linux.git
cd Tansu_for_Linux
flatpak-builder --user --install --force-clean build-dir io.github.Westenradev.Tansu.yml
flatpak run io.github.Westenradev.Tansu
```

## Checks that pass

- `cargo test -p tansu-core` — 8 tests pass.
- `cargo clippy -p tansu-core` — clean.
- `cargo fmt --check` — clean.

## Known issues and fixes already applied

1. Repo was private. Fixed: make it public on GitHub.
2. `Cargo.lock` was gitignored. Fixed: committed. Apps must commit it.
3. Flatpak manifest used `tag` and `branch` together. Fixed: tag only.
4. Rust toolchain missing in SDK. Fixed: `sdk-extensions: [org.freedesktop.Sdk.Extension.rust-stable]` plus `append-path: /usr/lib/sdk/rust-stable/bin`. The extension branch follows the GNOME 51 SDK. Do not pin `//26.08` in the manifest. Install `//26.08` on the host instead.
5. Cargo tried to reach the network. Fixed: build offline from `cargo-sources.json` with `.cargo/config.toml` vendored-sources. The `mkdir -p .cargo` line is required.
6. TOML parse error in generated config. Fixed: multi-line `printf`.
7. gtk4-rs 0.10 API fixes: `gdk4` is `gtk4::gdk`. `Value::get` returns `Result`. `gio::File::for_path` returns `File`, not `Result`. Multi-file drag uses `ContentProvider::new_union`. Clipboard read uses `read_text_async`. `AboutDialog` needs libadwaita feature `v1_7`. gtk4-layer-shell 0.6 functions take `&impl IsA<gtk::Window>` directly. No `set_always_on_top` in GTK4.

## What is still unverified

The `app` crate had never compiled until the last build attempt. The last error list had 19 errors. All were fixed in commit after `d7b7868`. The next Flatpak build must be run to confirm the app crate compiles. This was not yet verified.

## Current state (session 6)

Features that work (tested on X11 under Xvfb with a second GTK4 app as drag source/target):

- Drop files, text and images anywhere in the window. Paste with Ctrl+V.
- Drag rows out. Files go out as paths. Text and images are also written to temp files in `$XDG_CACHE_HOME/tansu/drag/` so folders and file managers accept them. The temp dir is cleared at startup.
- Remove-after-drag (menu toggle, default on) fires on `GdkDrag::dnd-finished`, so a cancelled drop keeps the item.
- Double-click: opens a file, or copies text/image back to the clipboard.
- Header bar with close button and menu. Ctrl+Q quits, Ctrl+W closes, Delete removes selection.
- Image thumbnails. Empty-state page. Window size and the toggle persist in `window.json`.
- Images are saved as base64 (the old number-array form still loads). A damaged `shelf.json` is renamed to `shelf.json.corrupt` instead of being overwritten. Missing files are dropped at startup unless their parent folder is missing (unmounted drive).
- `tansu --float` uses layer-shell (Sway, Hyprland, KDE only; GNOME Mutter has no layer-shell).
- Flatpak has `--filesystem=host:ro` so shelved files can be read and dragged out.

Checks: `cargo test -p tansu-core` (12 tests), `cargo clippy`, `cargo fmt --check`, `desktop-file-validate`, `appstreamcli validate --no-net`.

Build note: the app was only compile-checked against libadwaita 1.5 (`v1_5`). The real target is 1.7+ (`v1_7`), gtk4 feature `v4_8`. The real Flatpak build was run by the user.

Use `io.github.Westenradev.Tansu.local.yml` to build the local checkout. The main manifest fetches git tag `v0.1.0`, which points at old code. Cut a new tag before Flathub.

## Remaining work

1. Verify drag and drop on a real Wayland GNOME session (only X11 was simulated).
2. Add screenshots with captions to the metainfo (Flathub requires them).
3. Redraw the placeholder symbolic icon.
4. Flathub: reviewers may question `host:ro`. A narrower option is portal-only access.
5. App ID has uppercase letters (appstream pedantic note `cid-contains-uppercase-letter`). Decide before submitting.
6. RPM: Fedora may lack packaged gtk4-rs 0.10 crates; COPR will need vendored sources. Untested.
7. Thumbnails are decoded on every refresh. Cache them if shelves get large.
8. Submit to Flathub: needs public repo, new tag, screenshots.
