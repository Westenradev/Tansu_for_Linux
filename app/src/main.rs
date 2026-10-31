mod rows;
mod window;

use gtk4::prelude::*;

fn main() {
    let app = libadwaita::Application::builder()
        .application_id("io.github.Westenradev.Tansu")
        .build();

    app.connect_activate(|app| {
        let window = window::ShelfWindow::new(app);
        window.present();
    });

    app.run();
}
