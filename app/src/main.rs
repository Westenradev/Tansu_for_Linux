mod rows;
mod window;

use gtk4::gio;
use gtk4::prelude::*;

fn main() -> gtk4::glib::ExitCode {
    // `--float` asks the compositor to keep the shelf above other windows.
    // It only works where layer-shell exists (Sway, Hyprland, KDE), not GNOME.
    let float = std::env::args().any(|a| a == "--float");
    let prog = std::env::args().next().unwrap_or_default();

    let app = libadwaita::Application::builder()
        .application_id("io.github.Westenradev.Tansu")
        .build();

    let quit = gio::SimpleAction::new("quit", None);
    {
        let app = app.clone();
        quit.connect_activate(move |_, _| app.quit());
    }
    app.add_action(&quit);
    app.set_accels_for_action("app.quit", &["<primary>q"]);
    app.set_accels_for_action("window.close", &["<primary>w"]);
    app.set_accels_for_action("win.paste", &["<primary>v"]);
    app.set_accels_for_action("win.remove-selected", &["Delete"]);

    app.connect_activate(move |app| {
        if let Some(existing) = app.active_window() {
            existing.present();
            return;
        }
        window::ShelfWindow::new(app, float).present();
    });

    app.run_with_args(&[prog])
}
