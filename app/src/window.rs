use std::cell::{Cell, RefCell};
use std::fs;
use std::path::{Path, PathBuf};
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

fn export_dir() -> PathBuf {
    glib::user_cache_dir().join("tansu").join("drag")
}

/// Loads the shelf. A damaged file is kept as `shelf.json.corrupt` instead
/// of being overwritten, and files that are gone are dropped.
fn load_shelf() -> Shelf {
    let path = shelf_path();
    match Shelf::load(&path) {
        Ok(mut shelf) => {
            shelf.items.retain(|i| match i {
                // Keep items on drives that are merely not mounted right now.
                ShelfItem::File { path } => {
                    path.exists() || !path.parent().is_some_and(Path::exists)
                }
                _ => true,
            });
            shelf
        }
        Err(e) => {
            eprintln!("tansu: could not read {}: {e}", path.display());
            let _ = fs::rename(&path, path.with_extension("json.corrupt"));
            Shelf::default()
        }
    }
}

/// Writes text and image items to temporary files so that folders and file
/// managers, which only accept files, can receive them.
fn export_to_files(items: &[ShelfItem]) -> Vec<gio::File> {
    let dir = export_dir().join(glib::monotonic_time().to_string());
    let mut out = Vec::new();
    for item in items {
        let Some(bytes) = item.payload() else {
            continue;
        };
        if fs::create_dir_all(&dir).is_err() {
            break;
        }
        let name = PathBuf::from(item.export_file_name());
        let stem = name
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let ext = name.extension().map(|e| e.to_string_lossy().into_owned());
        let mut path = dir.join(&name);
        let mut n = 1;
        while path.exists() {
            n += 1;
            path = dir.join(match &ext {
                Some(ext) => format!("{stem} ({n}).{ext}"),
                None => format!("{stem} ({n})"),
            });
        }
        if fs::write(&path, bytes).is_ok() {
            out.push(gio::File::for_path(path));
        }
    }
    out
}

struct Ui {
    window: adw::ApplicationWindow,
    toasts: adw::ToastOverlay,
    list: gtk4::ListBox,
    stack: gtk4::Stack,
    shelf: RefCell<Shelf>,
    monitors: RefCell<Vec<gio::FileMonitor>>,
    /// Items of the drag that is in progress.
    dragging: RefCell<Vec<ShelfItem>>,
    /// Yoink-style: drop an item out of the shelf once it was dragged out.
    remove_after_drag: Cell<bool>,
}

fn texture_item(tex: &gdk::Texture) -> Option<ShelfItem> {
    ShelfItem::image("image/png", tex.save_to_png_bytes().to_vec())
}

fn drag_provider(items: &[ShelfItem]) -> Option<gdk::ContentProvider> {
    let mut provs = Vec::new();

    let mut files: Vec<gio::File> = items
        .iter()
        .filter_map(|i| match i {
            ShelfItem::File { path } => Some(gio::File::for_path(path)),
            _ => None,
        })
        .collect();
    files.extend(export_to_files(items));
    if !files.is_empty() {
        // Plain host-path URIs first, so file managers always get real paths.
        let uris: String = files.iter().map(|f| format!("{}\r\n", f.uri())).collect();
        provs.push(gdk::ContentProvider::for_bytes(
            "text/uri-list",
            &glib::Bytes::from_owned(uris.into_bytes()),
        ));
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
        if let Err(e) = self.shelf.borrow().save(shelf_path()) {
            eprintln!("tansu: could not save the shelf: {e}");
        }
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

    /// Double-click: open a file, or copy text/images back to the clipboard.
    fn install_row_activation(self: &Rc<Self>) {
        let ui = self.clone();
        self.list.connect_row_activated(move |_, row| {
            let item = ui.shelf.borrow().items.get(row.index() as usize).cloned();
            let clipboard = ui.window.clipboard();
            match item {
                Some(ShelfItem::File { path }) => {
                    let uri = gio::File::for_path(&path).uri();
                    if let Err(e) =
                        gio::AppInfo::launch_default_for_uri(&uri, gio::AppLaunchContext::NONE)
                    {
                        ui.toasts
                            .add_toast(adw::Toast::new(&format!("Could not open: {e}")));
                    }
                }
                Some(ShelfItem::Text { text }) => {
                    clipboard.set_text(&text);
                    ui.toasts.add_toast(adw::Toast::new("Text copied"));
                }
                Some(ShelfItem::Image { data, .. }) => {
                    if let Ok(tex) = gdk::Texture::from_bytes(&glib::Bytes::from(&data[..])) {
                        clipboard.set_texture(&tex);
                        ui.toasts.add_toast(adw::Toast::new("Image copied"));
                    }
                }
                None => {}
            }
        });
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
        let toggle = gio::SimpleAction::new_stateful(
            "remove-after-drag",
            None,
            &self.remove_after_drag.get().to_variant(),
        );
        let ui = self.clone();
        toggle.connect_activate(move |a, _| {
            let on = !ui.remove_after_drag.get();
            ui.remove_after_drag.set(on);
            a.set_state(&on.to_variant());
        });
        self.window.add_action(&toggle);
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
        source.connect_prepare(move |s, x, y| {
            let row = ui.list.row_at_y(y as i32)?;
            let (rx, ry) = ui
                .list
                .translate_coordinates(&row, x, y)
                .unwrap_or((0.0, 0.0));
            s.set_icon(
                Some(&gtk4::WidgetPaintable::new(Some(&row))),
                rx as i32,
                ry as i32,
            );
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
            let provider = drag_provider(&items);
            drop(shelf);
            *ui.dragging.borrow_mut() = items;
            provider
        });
        let ui = self.clone();
        source.connect_drag_begin(move |_s, drag| {
            let ui = ui.clone();
            // Only fires when the receiver really took the data.
            drag.connect_dnd_finished(move |_| {
                let dragged = std::mem::take(&mut *ui.dragging.borrow_mut());
                if !ui.remove_after_drag.get() || dragged.is_empty() {
                    return;
                }
                let ui = ui.clone();
                glib::idle_add_local_once(move || {
                    {
                        let mut shelf = ui.shelf.borrow_mut();
                        for d in dragged {
                            if let Some(pos) = shelf.items.iter().position(|i| *i == d) {
                                shelf.items.remove(pos);
                            }
                        }
                    }
                    ui.persist();
                    ui.refresh();
                });
            });
        });
        self.list.add_controller(source);
    }

    fn restore_window_size(self: &Rc<Self>) {
        if let Ok(bytes) = std::fs::read(settings_path()) {
            if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&bytes) {
                let w = v["width"].as_i64().unwrap_or(340).clamp(240, 4000) as i32;
                let h = v["height"].as_i64().unwrap_or(520).clamp(240, 4000) as i32;
                self.window.set_default_size(w, h);
                if let Some(b) = v["remove_after_drag"].as_bool() {
                    self.remove_after_drag.set(b);
                }
            }
        }
    }

    fn save_window_size_on_close(self: &Rc<Self>) {
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
        let toasts = adw::ToastOverlay::new();
        toasts.set_child(Some(&stack));

        let menu = gio::Menu::new();
        let edit = gio::Menu::new();
        edit.append(Some("Paste"), Some("win.paste"));
        edit.append(Some("Remove Selected"), Some("win.remove-selected"));
        edit.append(Some("Clear Shelf"), Some("win.clear"));
        edit.append(
            Some("Remove After Dragging Out"),
            Some("win.remove-after-drag"),
        );
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
        toolbar.set_content(Some(&toasts));
        window.set_content(Some(&toolbar));

        let shelf = load_shelf();
        let _ = fs::remove_dir_all(export_dir());
        let ui = Rc::new(Ui {
            window,
            toasts,
            list,
            stack,
            shelf: RefCell::new(shelf),
            monitors: RefCell::new(Vec::new()),
            dragging: RefCell::new(Vec::new()),
            remove_after_drag: Cell::new(true),
        });
        ui.restore_window_size();
        ui.install_actions();
        ui.install_drop_target(&toolbar);
        ui.install_drag_source();
        ui.install_row_activation();
        ui.save_window_size_on_close();
        ui.refresh();
        Self { ui }
    }

    pub fn present(&self) {
        self.ui.window.present();
    }
}
