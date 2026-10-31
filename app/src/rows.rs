use gtk4::prelude::*;
use gtk4::{gdk, glib};
use tansu_core::ShelfItem;

const THUMB: i32 = 48;
const MAX_PREVIEW_BYTES: u64 = 32 * 1024 * 1024;

fn is_image_path(path: &std::path::Path) -> bool {
    matches!(
        path.extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .as_deref(),
        Some("png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "svg" | "tif" | "tiff")
    )
}

/// A preview image for the item, if it has one.
fn thumbnail(item: &ShelfItem) -> Option<gdk::Texture> {
    match item {
        ShelfItem::Image { data, .. } => {
            gdk::Texture::from_bytes(&glib::Bytes::from(&data[..])).ok()
        }
        ShelfItem::File { path } if is_image_path(path) => {
            let small = std::fs::metadata(path)
                .map(|m| m.len() <= MAX_PREVIEW_BYTES)
                .unwrap_or(false);
            if small {
                gdk::Texture::from_filename(path).ok()
            } else {
                None
            }
        }
        _ => None,
    }
}

fn subtitle(item: &ShelfItem) -> String {
    match item {
        ShelfItem::File { path } => path
            .parent()
            .map(|p| p.display().to_string())
            .unwrap_or_default(),
        ShelfItem::Text { .. } => "Text".to_string(),
        ShelfItem::Image { .. } => "Image".to_string(),
    }
}

/// Builds one shelf row: preview or icon, name, source, and a remove button.
pub fn build_row(
    item: &ShelfItem,
    on_remove: impl Fn(&gtk4::ListBoxRow) + 'static,
) -> gtk4::ListBoxRow {
    let lead: gtk4::Widget = match thumbnail(item) {
        Some(tex) => {
            let pic = gtk4::Image::from_paintable(Some(&tex));
            pic.set_pixel_size(THUMB);
            pic.set_overflow(gtk4::Overflow::Hidden);
            pic.add_css_class("card");
            pic.upcast()
        }
        None => {
            let name = match item {
                ShelfItem::Image { .. } => "image-x-generic-symbolic",
                ShelfItem::Text { .. } => "text-x-generic-symbolic",
                ShelfItem::File { .. } => "document-symbolic",
            };
            let icon = gtk4::Image::from_icon_name(name);
            icon.set_pixel_size(24);
            icon.set_size_request(THUMB, THUMB);
            icon.upcast()
        }
    };

    let title = gtk4::Label::builder()
        .label(item.name())
        .xalign(0.0)
        .ellipsize(gtk4::pango::EllipsizeMode::End)
        .build();
    let sub = gtk4::Label::builder()
        .label(subtitle(item))
        .xalign(0.0)
        .ellipsize(gtk4::pango::EllipsizeMode::Middle)
        .build();
    sub.add_css_class("caption");
    sub.add_css_class("dim-label");
    let text = gtk4::Box::builder()
        .orientation(gtk4::Orientation::Vertical)
        .valign(gtk4::Align::Center)
        .hexpand(true)
        .build();
    text.append(&title);
    text.append(&sub);

    let remove = gtk4::Button::from_icon_name("window-close-symbolic");
    remove.add_css_class("flat");
    remove.add_css_class("circular");
    remove.set_valign(gtk4::Align::Center);
    remove.set_tooltip_text(Some("Remove from shelf"));

    let content = gtk4::Box::builder()
        .orientation(gtk4::Orientation::Horizontal)
        .spacing(12)
        .margin_top(6)
        .margin_bottom(6)
        .margin_start(8)
        .margin_end(8)
        .build();
    content.append(&lead);
    content.append(&text);
    content.append(&remove);

    let row = gtk4::ListBoxRow::builder().child(&content).build();
    let weak = row.downgrade();
    remove.connect_clicked(move |_| {
        if let Some(row) = weak.upgrade() {
            on_remove(&row);
        }
    });
    row
}
