use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{gdk, gio, glib};
use libadwaita as adw;
use libadwaita::prelude::*;
use tansu_core::{Shelf, ShelfItem};

use crate::rows;

fn config_dir() -> PathBuf {
    glib::user_config_dir().join("tansu")
}

fn shelf_path() -> PathBuf {
    config_dir().join("shelf.json")
}

fn settings_path() -> PathBuf {
    config_dir().join("window.json")
}

struct Ui {
    window: adw::ApplicationWindow,
    list: gtk4::ListBox,
    stack: gtk4::Stack,
    shelf: RefCell<Shelf>,
    monitors: RefCell<Vec<gio::FileMonitor>>,
}

fn texture_item(tex: &gdk::Texture) -> Option<ShelfItem> {
    ShelfItem::image("image/png", tex.save_to_png_bytes().to_vec())
}

fn drag_provider(items: &[ShelfItem]) -> Option<gdk::ContentProvider> {
    let mut provs = Vec::new();

    let files: Vec<gio::File> = items
        .iter()
        .filter_map(|i| match i {
            ShelfItem::File { path } => Some(gio::File::for_path(path)),
            _ => None,
        })
        .collect();
    if !files.is_empty() {
        let list = gdk::FileList::from_array(&files);
        provs.push(gdk::ContentProvider::for_value(&list.to_value()));
    }

    let texts: Vec<&str> = items
        .iter()
        .filter_map(|i| match i {
            ShelfItem::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    if !texts.is_empty() {
        provs.push(gdk::ContentProvider::for_value(
            &texts.join("\n").to_value(),
        ));
    }

    let image = items.iter().find_map(|i| match i {
        ShelfItem::Image { data, .. } => Some(data),
        _ => None,
    });
    if let Some(data) = image {
        if let Ok(tex) = gdk::Texture::from_bytes(&glib::Bytes::from(&data[..])) {
            provs.push(gdk::ContentProvider::for_value(&tex.to_value()));
        }
    }

    match provs.len() {
        0 => None,
        1 => provs.pop(),
        _ => Some(gdk::ContentProvider::new_union(&provs)),
    }
}

impl Ui {
    fn persist(&self) {
        let _ = self.shelf.borrow().save(shelf_path());
    }

    fn add(self: &Rc<Self>, items: Vec<ShelfItem>) {
        {
            let mut shelf = self.shelf.borrow_mut();
            for item in items {
                if let ShelfItem::File { path } = &item {
                    let dup = shelf
                        .items
                        .iter()
                        .any(|i| matches!(i, ShelfItem::File { path: p } if p == path));
                    if dup {
                        continue;
                    }
                }
                shelf.items.push(item);
            }
        }
        self.persist();
        self.refresh();
    }

    fn remove_indices(self: &Rc<Self>, mut idx: Vec<usize>) {
        idx.sort_unstable_by(|a, b| b.cmp(a));
        idx.dedup();
        {
            let mut shelf = self.shelf.borrow_mut();
            for i in idx {
                if i < shelf.items.len() {
                    shelf.items.remove(i);
                }
            }
        }
        self.persist();
        self.refresh();
    }

    fn selected_indices(&self) -> Vec<usize> {
        self.list
            .selected_rows()
            .iter()
            .map(|r| r.index() as usize)
            .collect()
    }

    fn refresh(self: &Rc<Self>) {
        while let Some(row) = self.list.row_at_index(0) {
            self.list.remove(&row);
        }
        let items = self.shelf.borrow().items.clone();
        for item in &items {
            let ui = self.clone();
            let row = rows::build_row(item, move |r| {
                let idx = r.index() as usize;
                let ui = ui.clone();
                glib::idle_add_local_once(move || ui.remove_indices(vec![idx]));
            });
            self.list.append(&row);
        }
        self.stack
            .set_visible_child_name(if items.is_empty() { "empty" } else { "list" });
        self.watch(&items);
    }

    /// Follows files that are renamed, moved away or deleted.
    fn watch(self: &Rc<Self>, items: &[ShelfItem]) {
        let mut monitors = Vec::new();
        for item in items {
            let ShelfItem::File { path } = item else {
                continue;
            };
            let file = gio::File::for_path(path);
            let Ok(monitor) =
                file.monitor_file(gio::FileMonitorFlags::WATCH_MOVES, gio::Cancellable::NONE)
            else {
                continue;
            };
            let ui = Rc::downgrade(self);
            let old = path.clone();
            monitor.connect_changed(move |_m, _f, other, ev| {
                let Some(ui) = ui.upgrade() else { return };
                let old = old.clone();
                match ev {
                    gio::FileMonitorEvent::Renamed => {
                        if let Some(new) = other.and_then(|f| f.path()) {
                            glib::idle_add_local_once(move || {
                                for i in ui.shelf.borrow_mut().items.iter_mut() {
                                    if matches!(i, ShelfItem::File { path } if *path == old) {
                                        *i = ShelfItem::File { path: new.clone() };
                                    }
                                }
                                ui.persist();
                                ui.refresh();
                            });
                        }
                    }
                    gio::FileMonitorEvent::Deleted | gio::FileMonitorEvent::MovedOut => {
                        glib::idle_add_local_once(move || {
                            if old.exists() {
                                return;
                            }
                            ui.shelf
                                .borrow_mut()
                                .items
                                .retain(|i| !matches!(i, ShelfItem::File { path } if *path == old));
                            ui.persist();
                            ui.refresh();
                        });
                    }
                    _ => {}
                }
            });
            monitors.push(monitor);
        }
        *self.monitors.borrow_mut() = monitors;
    }

    fn paste(self: &Rc<Self>) {
        let clip = self.window.clipboard();
        let formats = clip.formats();
        let ui = self.clone();
        if formats.contains_type(gdk::FileList::static_type()) {
            glib::spawn_future_local(async move {
                let value = clip
                    .read_value_future(gdk::FileList::static_type(), glib::Priority::DEFAULT)
                    .await;
                if let Ok(list) = value.and_then(|v| {
                    v.get::<gdk::FileList>()
                        .map_err(|e| glib::Error::new(glib::FileError::Inval, &e.to_string()))
                }) {
                    let items = list
                        .files()
                        .iter()
                        .filter_map(|f| f.path())
                        .filter_map(ShelfItem::file)
                        .collect::<Vec<_>>();
                    if !items.is_empty() {
                        ui.add(items);
                    }
                }
            });
        } else if formats.contains_type(gdk::Texture::static_type()) {
            clip.read_texture_async(gio::Cancellable::NONE, move |res| {
                if let Ok(Some(item)) = res.map(|t| t.and_then(|t| texture_item(&t))) {
                    ui.add(vec![item]);
                }
            });
        } else {
            clip.read_text_async(gio::Cancellable::NONE, move |res| {
                if let Ok(Some(text)) = res {
                    if let Some(item) = ShelfItem::text(text.to_string()) {
                        ui.add(vec![item]);
                    }
                }
            });
        }
    }

    fn action(self: &Rc<Self>, name: &str, f: impl Fn(&Rc<Ui>) + 'static) {
        let action = gio::SimpleAction::new(name, None);
        let ui = self.clone();
        action.connect_activate(move |_, _| f(&ui));
        self.window.add_action(&action);
    }

    fn install_actions(self: &Rc<Self>) {
        self.action("paste", |u| u.paste());
        self.action("remove-selected", |u| {
            let idx = u.selected_indices();
            u.remove_indices(idx);
        });
        self.action("clear", |u| {
            u.shelf.borrow_mut().items.clear();
            u.persist();
            u.refresh();
        });
        self.action("about", |u| {
            let dlg = adw::AboutDialog::builder()
                .application_name("Tansu")
                .application_icon("io.github.Westenradev.Tansu")
                .developer_name("Westenradev")
                .version("0.1.0")
                .license_type(gtk4::License::MitX11)
                .website("https://github.com/Westenradev/Tansu_for_Linux")
                .build();
            dlg.present(Some(&u.window));
        });
    }

    /// Dropping works anywhere in the window, even when the shelf is empty.
    fn install_drop_target(self: &Rc<Self>, widget: &impl IsA<gtk4::Widget>) {
        let target = gtk4::DropTarget::new(glib::Type::INVALID, gdk::DragAction::COPY);
        target.set_types(&[
            gdk::FileList::static_type(),
            gdk::Texture::static_type(),
            glib::Type::STRING,
        ]);
        let ui = self.clone();
        target.connect_drop(move |_t, value, _x, _y| {
            let mut items = Vec::new();
            if let Ok(list) = value.get::<gdk::FileList>() {
                items.extend(
                    list.files()
                        .iter()
                        .filter_map(|f| f.path())
                        .filter_map(ShelfItem::file),
                );
            } else if let Ok(tex) = value.get::<gdk::Texture>() {
                items.extend(texture_item(&tex));
            } else if let Ok(text) = value.get::<String>() {
                items.extend(ShelfItem::text(text));
            }
            if items.is_empty() {
                return false;
            }
            ui.add(items);
            true
        });
        widget.add_controller(target);
    }

    /// Dragging a row drags the whole selection, or just that row.
    fn install_drag_source(self: &Rc<Self>) {
        let source = gtk4::DragSource::new();
        source.set_actions(gdk::DragAction::COPY);
        let ui = self.clone();
        source.connect_prepare(move |_s, _x, y| {
            let row = ui.list.row_at_y(y as i32)?;
            let mut idx = if row.is_selected() {
                ui.selected_indices()
            } else {
                vec![row.index() as usize]
            };
            idx.sort_unstable();
            let shelf = ui.shelf.borrow();
            let items: Vec<ShelfItem> = idx
                .iter()
                .filter_map(|&i| shelf.items.get(i).cloned())
                .collect();
            drag_provider(&items)
        });
        self.list.add_controller(source);
    }

    fn restore_window_size(&self) {
        if let Ok(bytes) = std::fs::read(settings_path()) {
            if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&bytes) {
                let w = v["width"].as_i64().unwrap_or(340).clamp(240, 4000) as i32;
                let h = v["height"].as_i64().unwrap_or(520).clamp(240, 4000) as i32;
                self.window.set_default_size(w, h);
            }
        }
    }

    fn save_window_size_on_close(&self) {
        self.window.connect_close_request(move |w| {
            let size =
                serde_json::json!({ "width": w.default_width(), "height": w.default_height() });
            let _ = std::fs::create_dir_all(config_dir());
            let _ = std::fs::write(settings_path(), size.to_string());
            glib::Propagation::Proceed
        });
    }
}

#[cfg(feature = "layer-shell")]
fn apply_float(window: &adw::ApplicationWindow) {
    use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
    if gtk4_layer_shell::is_supported() {
        window.init_layer_shell();
        window.set_anchor(Edge::Right, true);
        window.set_anchor(Edge::Top, true);
        window.set_anchor(Edge::Bottom, true);
        window.set_layer(Layer::Top);
        window.set_keyboard_mode(KeyboardMode::OnDemand);
        window.auto_exclusive_zone_enable();
    } else {
        eprintln!("tansu: this desktop has no layer-shell support; using a normal window");
    }
}

#[cfg(not(feature = "layer-shell"))]
fn apply_float(_window: &adw::ApplicationWindow) {
    eprintln!("tansu: built without layer-shell; using a normal window");
}

pub struct ShelfWindow {
    ui: Rc<Ui>,
}

impl ShelfWindow {
    pub fn new(app: &adw::Application, float: bool) -> Self {
        let window = adw::ApplicationWindow::builder()
            .application(app)
            .title("Tansu")
            .default_width(340)
            .default_height(520)
            .build();
        if float {
            apply_float(&window);
        }

        let list = gtk4::ListBox::builder()
            .selection_mode(gtk4::SelectionMode::Multiple)
            .margin_top(12)
            .margin_bottom(12)
            .margin_start(12)
            .margin_end(12)
            .valign(gtk4::Align::Start)
            .build();
        list.add_css_class("boxed-list");
        let scrolled = gtk4::ScrolledWindow::builder()
            .child(&list)
            .vexpand(true)
            .hscrollbar_policy(gtk4::PolicyType::Never)
            .build();

        let empty = adw::StatusPage::builder()
            .icon_name("folder-download-symbolic")
            .title("Shelf is empty")
            .description("Drop files, text or images here")
            .build();

        let stack = gtk4::Stack::new();
        stack.add_named(&empty, Some("empty"));
        stack.add_named(&scrolled, Some("list"));

        let menu = gio::Menu::new();
        let edit = gio::Menu::new();
        edit.append(Some("Paste"), Some("win.paste"));
        edit.append(Some("Remove Selected"), Some("win.remove-selected"));
        edit.append(Some("Clear Shelf"), Some("win.clear"));
        let app_section = gio::Menu::new();
        app_section.append(Some("About Tansu"), Some("win.about"));
        app_section.append(Some("Quit"), Some("app.quit"));
        menu.append_section(None, &edit);
        menu.append_section(None, &app_section);
        let menu_button = gtk4::MenuButton::builder()
            .icon_name("open-menu-symbolic")
            .menu_model(&menu)
            .tooltip_text("Main Menu")
            .primary(true)
            .build();
        let header = adw::HeaderBar::new();
        header.pack_end(&menu_button);

        let toolbar = adw::ToolbarView::new();
        toolbar.add_top_bar(&header);
        toolbar.set_content(Some(&stack));
        window.set_content(Some(&toolbar));

        let shelf = Shelf::load(shelf_path()).unwrap_or_default();
        let ui = Rc::new(Ui {
            window,
            list,
            stack,
            shelf: RefCell::new(shelf),
            monitors: RefCell::new(Vec::new()),
        });
        ui.install_actions();
        ui.install_drop_target(&toolbar);
        ui.install_drag_source();
        ui.restore_window_size();
        ui.save_window_size_on_close();
        ui.refresh();
        Self { ui }
    }

    pub fn present(&self) {
        self.ui.window.present();
    }
}
