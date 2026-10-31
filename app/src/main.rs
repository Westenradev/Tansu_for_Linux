use gtk4::prelude::*;
use libadwaita::prelude::*;

fn main() {
    let app = libadwaita::Application::builder()
        .application_id("io.github.Westenradev.Tansu")
        .build();

    app.connect_activate(|app| {
        let window = libadwaita::ApplicationWindow::builder()
            .application(app)
            .title("Tansu")
            .default_width(320)
            .default_height(480)
            .build();
        let label = gtk4::Label::builder()
            .label("Tansu shelf (scaffold)")
            .margin_top(12)
            .margin_bottom(12)
            .margin_start(12)
            .margin_end(12)
            .build();
        window.set_content(Some(&label));
        window.present();
    });

    app.run();
}
