use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{gio, glib};
use libadwaita::prelude::*;
use tansu_core::{Shelf, ShelfItem};

use crate::rows::ShelfRow;

fn shelf_path() -> PathBuf {
    PathBuf::from(glib::user_config_dir().to_string_lossy().as_ref())
        .join("tansu")
        .join("shelf.json")
}

fn settings_path() -> PathBuf {
    PathBuf::from(glib::user_config_dir().to_string_lossy().as_ref())
        .join("tansu")
        .join("window.json")
}

struct State {
    shelf: Shelf,
    monitors: Vec<gio::FileMonitor>,
}

fn reload(list: &gtk4::ListBox, state: &Rc<RefCell<State>>) {
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }
    for item in &state.borrow().shelf.items {
        list.append(&ShelfRow::new(item).widget());
    }
}

pub struct ShelfWindow {
    window: libadwaita::ApplicationWindow,
    list: gtk4::ListBox,
    state: Rc<RefCell<State>>,
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
        let state = Rc::new(RefCell::new(State {
            shelf,
            monitors: Vec::new(),
        }));

        let this = Self {
            window: window.clone(),
            list: list.clone(),
            state: state.clone(),
        };
        this.reload_rows();
        this.install_drop_target();
        this.watch_files();
        this.restore_window_size();
        this.save_window_size_on_close();
        this
    }

    pub fn present(&self) {
        self.window.present();
    }

    fn reload_rows(&self) {
        while let Some(child) = self.list.first_child() {
            self.list.remove(&child);
        }
        let items = self.state.borrow().shelf.items.clone();
        for item in &items {
            let row = ShelfRow::new(item);
            self.list.append(&row.widget());
        }
    }

    fn save(&self) {
        let shelf = self.state.borrow().shelf.clone();
        let _ = shelf.save(shelf_path());
    }

    fn install_drop_target(&self) {
        let target = gtk4::DropTarget::new(gio::File::static_type(), gdk4::DragAction::COPY);
        target.set_types(&[gio::File::static_type()]);
        let state = self.state.clone();
        let list = self.list.clone();
        target.connect_drop(move |_t, value, _x, _y| {
            if let Some(file) = value.get::<gio::File>() {
                if let Some(path) = file.path() {
                    if let Some(item) = ShelfItem::file(&path) {
                        state.borrow_mut().shelf.items.push(item);
                        let shelf = state.borrow().shelf.clone();
                        let _ = shelf.save(shelf_path());
                        reload(&list, &state);
                        return true;
                    }
                }
            }
            false
        });
        self.list.add_controller(target);

        let text_target = gtk4::DropTarget::new(glib::types::Type::STRING, gdk4::DragAction::COPY);
        let state = self.state.clone();
        let list = self.list.clone();
        text_target.connect_drop(move |_t, value, _x, _y| {
            if let Some(text) = value.get::<String>() {
                if let Some(item) = ShelfItem::text(text) {
                    state.borrow_mut().shelf.items.push(item);
                    let shelf = state.borrow().shelf.clone();
                    let _ = shelf.save(shelf_path());
                    reload(&list, &state);
                    return true;
                }
            }
            false
        });
        self.list.add_controller(text_target);
    }

    fn watch_files(&self) {
        let mut monitors = Vec::new();
        let items = self.state.borrow().shelf.items.clone();
        for item in items {
            if let ShelfItem::File { path } = item {
                if let Ok(file) = gio::File::for_path(&path) {
                    if let Ok(monitor) = file
                        .monitor_file(gio::FileMonitorFlags::NONE, gio::Cancellable::NONE.as_ref())
                    {
                        let state = self.state.clone();
                        let list = self.list.clone();
                        monitor.connect_changed(move |m, f, _e| {
                            let _ = m;
                            if let Some(p) = f.path() {
                                let mut st = state.borrow_mut();
                                if let Some(item) =
                                    st.shelf.items.iter_mut().find(
                                        |i| matches!(i, ShelfItem::File { path } if *path == p),
                                    )
                                {
                                    *item = ShelfItem::File { path: p.clone() };
                                } else if !p.exists() {
                                    st.shelf.items.retain(
                                        |i| !matches!(i, ShelfItem::File { path } if *path == p),
                                    );
                                    drop(st);
                                    reload(&list, &state);
                                }
                            }
                        });
                        monitors.push(monitor);
                    }
                }
            }
        }
        self.state.borrow_mut().monitors = monitors;
    }

    fn restore_window_size(&self) {
        if let Ok(bytes) = std::fs::read(settings_path()) {
            if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&bytes) {
                let w = v["width"].as_u64().unwrap_or(320) as i32;
                let h = v["height"].as_u64().unwrap_or(480) as i32;
                self.window.set_default_size(w, h);
            }
        }
    }

    fn save_window_size_on_close(&self) {
        let window = self.window.clone();
        window.connect_close_request(move |w| {
            let size = serde_json::json!({
                "width": w.default_width(),
                "height": w.default_height(),
            });
            let _ = std::fs::write(settings_path(), size.to_string());
            glib::Propagation::Proceed
        });
    }
}
