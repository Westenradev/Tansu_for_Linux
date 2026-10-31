use std::path::PathBuf;

use gtk4::prelude::*;
use libadwaita::prelude::*;
use tansu_core::{Shelf, ShelfItem};

use crate::rows::ShelfRow;

/// Returns the path of the JSON file holding the shelf contents.
fn shelf_path() -> PathBuf {
    glib_config_dir().join("tansu").join("shelf.json")
}

fn glib_config_dir() -> PathBuf {
    PathBuf::from(gtk4::glib::user_config_dir().to_string_lossy().as_ref())
}

pub struct ShelfWindow {
    window: libadwaita::ApplicationWindow,
    list: gtk4::ListBox,
    shelf: Shelf,
}

impl ShelfWindow {
    pub fn new(app: &libadwaita::Application) -> Self {
        let window = libadwaita::ApplicationWindow::builder()
            .application(app)
            .title("Tansu")
            .default_width(320)
            .default_height(480)
            .build();

        let list = gtk4::ListBox::builder()
            .selection_mode(gtk4::SelectionMode::Multiple)
            .build();
        list.add_css_class("boxed-list");

        let scrolled = gtk4::ScrolledWindow::builder()
            .child(&list)
            .vexpand(true)
            .build();
        let clamp = libadwaita::Clamp::builder()
            .child(&scrolled)
            .margin_top(12)
            .margin_bottom(12)
            .margin_start(12)
            .margin_end(12)
            .build();
        window.set_content(Some(&clamp));

        let shelf = Shelf::load(shelf_path()).unwrap_or_default();
        let mut this = Self {
            window,
            list,
            shelf,
        };
        this.reload_rows();
        this
    }

    pub fn present(&self) {
        self.window.present();
    }

    fn reload_rows(&mut self) {
        while let Some(child) = self.list.first_child() {
            self.list.remove(&child);
        }
        for item in &self.shelf.items {
            let row = ShelfRow::new(item);
            self.list.append(&row.widget());
        }
    }

    #[allow(dead_code)]
    fn save(&self) {
        let _ = self.shelf.save(shelf_path());
    }
}
