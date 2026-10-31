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
        Self::try_layer_shell(&window);

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
        this.install_drag_source();
        this.install_context_menu();
        this.watch_files();
        this.restore_window_size();
        this.save_window_size_on_close();
        this
    }

    #[cfg(all(target_os = "linux", feature = "layer-shell"))]
    fn try_layer_shell(window: &libadwaita::ApplicationWindow) {
        if gtk4_layer_shell::is_supported() {
            gtk4_layer_shell::init_for_window(window);
            gtk4_layer_shell::set_anchor(window, gtk4_layer_shell::Edge::Right, true);
            gtk4_layer_shell::set_anchor(window, gtk4_layer_shell::Edge::Top, true);
            gtk4_layer_shell::set_anchor(window, gtk4_layer_shell::Edge::Bottom, true);
            gtk4_layer_shell::set_layer(window, gtk4_layer_shell::Layer::Top);
            gtk4_layer_shell::auto_exclusive_zone_enable(window);
        }
    }

    #[cfg(not(all(target_os = "linux", feature = "layer-shell")))]
    fn try_layer_shell(_window: &libadwaita::ApplicationWindow) {}

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

    fn install_drop_target(&self) {
        let target = gtk4::DropTarget::new(gio::File::static_type(), gtk4::gdk::DragAction::COPY);
        target.set_types(&[gio::File::static_type()]);
        let state = self.state.clone();
        let list = self.list.clone();
        target.connect_drop(move |_t, value, _x, _y| {
            if let Ok(file) = value.get::<gio::File>() {
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

        let text_target =
            gtk4::DropTarget::new(glib::types::Type::STRING, gtk4::gdk::DragAction::COPY);
        let state = self.state.clone();
        let list = self.list.clone();
        text_target.connect_drop(move |_t, value, _x, _y| {
            if let Ok(text) = value.get::<String>() {
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

        let image_target = gtk4::DropTarget::new(
            gtk4::gdk::Texture::static_type(),
            gtk4::gdk::DragAction::COPY,
        );
        let state = self.state.clone();
        let list = self.list.clone();
        image_target.connect_drop(move |_t, value, _x, _y| {
            if let Ok(texture) = value.get::<gtk4::gdk::Texture>() {
                let bytes = texture.save_to_png_bytes();
                if let Some(item) = ShelfItem::image("image/png", bytes.to_vec()) {
                    state.borrow_mut().shelf.items.push(item);
                    let shelf = state.borrow().shelf.clone();
                    let _ = shelf.save(shelf_path());
                    reload(&list, &state);
                    return true;
                }
            }
            false
        });
        self.list.add_controller(image_target);
    }

    fn install_drag_source(&self) {
        let source = gtk4::DragSource::new();
        source.set_actions(gtk4::gdk::DragAction::COPY);
        let state = self.state.clone();
        source.connect_prepare(move |_s, x, y| {
            let st = state.borrow();
            if st.shelf.items.is_empty() {
                return None;
            }
            let items = st.shelf.items.clone();
            drop(st);
            let files: Vec<gio::File> = items
                .iter()
                .filter_map(|i| match i {
                    ShelfItem::File { path } => Some(gio::File::for_path(path)),
                    _ => None,
                })
                .collect();
            let _ = (x, y);
            if !files.is_empty() {
                let providers: Vec<gtk4::gdk::ContentProvider> = files
                    .iter()
                    .map(|f| gtk4::gdk::ContentProvider::for_value(&f.to_value()))
                    .collect();
                return Some(gtk4::gdk::ContentProvider::new_union(&providers));
            }
            if let Some(item) = items.first() {
                match item {
                    ShelfItem::Text { text } => {
                        return Some(gtk4::gdk::ContentProvider::for_value(
                            &text.as_str().to_value(),
                        ))
                    }
                    ShelfItem::Image { mime, data } => {
                        return Some(gtk4::gdk::ContentProvider::for_bytes(
                            mime,
                            &glib::Bytes::from(&data[..]),
                        ));
                    }
                    ShelfItem::File { .. } => unreachable!(),
                }
            }
            None
        });
        self.list.add_controller(source);
    }

    fn install_context_menu(&self) {
        let gesture = gtk4::GestureClick::builder().button(3).build();
        let state = self.state.clone();
        let list = self.list.clone();
        let window = self.window.clone();
        gesture.connect_released(move |g, _n, x, y| {
            let menu = gio::Menu::new();
            menu.append(Some("Remove"), Some("win.remove"));
            menu.append(Some("Clear shelf"), Some("win.clear"));
            menu.append(Some("Paste"), Some("win.paste"));
            menu.append(Some("About Tansu"), Some("win.about"));
            let popover = gtk4::PopoverMenu::builder()
                .menu_model(&menu)
                .pointing_to(&gtk4::gdk::Rectangle::new(x as i32, y as i32, 1, 1))
                .build();
            popover.set_parent(&g.widget().unwrap());

            let group = gio::SimpleActionGroup::new();
            let remove = gio::SimpleAction::new("remove", None);
            let clear = gio::SimpleAction::new("clear", None);
            let paste = gio::SimpleAction::new("paste", None);
            let about = gio::SimpleAction::new("about", None);
            let st_r = state.clone();
            let ls_r = list.clone();
            remove.connect_activate(move |_, _| {
                let mut idx: Vec<usize> = ls_r
                    .selected_rows()
                    .iter()
                    .map(|r| r.index() as usize)
                    .collect();
                idx.sort_unstable_by(|a, b| b.cmp(a));
                for i in idx {
                    if i < st_r.borrow().shelf.items.len() {
                        st_r.borrow_mut().shelf.items.remove(i);
                    }
                }
                let shelf = st_r.borrow().shelf.clone();
                let _ = shelf.save(shelf_path());
                reload(&ls_r, &st_r);
            });
            let st_c = state.clone();
            let ls_c = list.clone();
            clear.connect_activate(move |_, _| {
                st_c.borrow_mut().shelf.items.clear();
                let shelf = st_c.borrow().shelf.clone();
                let _ = shelf.save(shelf_path());
                reload(&ls_c, &st_c);
            });
            let st_p = state.clone();
            let ls_p = list.clone();
            paste.connect_activate(move |_, _| {
                let display = ls_p.display();
                let clip = display.clipboard();
                let st_p = st_p.clone();
                let ls_p = ls_p.clone();
                clip.read_text_async(gio::Cancellable::NONE, move |res| {
                    if let Ok(Some(text)) = res {
                        if let Some(item) = ShelfItem::text(text.to_string()) {
                            st_p.borrow_mut().shelf.items.push(item);
                            let shelf = st_p.borrow().shelf.clone();
                            let _ = shelf.save(shelf_path());
                            reload(&ls_p, &st_p);
                        }
                    }
                });
            });
            let w_a = window.clone();
            about.connect_activate(move |_, _| {
                let dlg = libadwaita::AboutDialog::builder()
                    .application_name("Tansu")
                    .version("0.1.0")
                    .license_type(gtk4::License::MitX11)
                    .website("https://github.com/Westenradev/Tansu_for_Linux")
                    .build();
                dlg.present(Some(&w_a));
            });
            group.add_action(&remove);
            group.add_action(&clear);
            group.add_action(&paste);
            group.add_action(&about);
            window.insert_action_group("win", Some(&group));
            popover.popup();
        });
        self.list.add_controller(gesture);
    }

    fn watch_files(&self) {
        let mut monitors = Vec::new();
        let items = self.state.borrow().shelf.items.clone();
        for item in items {
            if let ShelfItem::File { path } = item {
                let file = gio::File::for_path(&path);
                if let Ok(monitor) =
                    file.monitor_file(gio::FileMonitorFlags::NONE, gio::Cancellable::NONE)
                {
                    let state = self.state.clone();
                    let list = self.list.clone();
                    monitor.connect_changed(move |_m, f, _other, _e| {
                        if let Some(p) = f.path() {
                            let mut st = state.borrow_mut();
                            if let Some(item) = st
                                .shelf
                                .items
                                .iter_mut()
                                .find(|i| matches!(i, ShelfItem::File { path } if *path == p))
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
