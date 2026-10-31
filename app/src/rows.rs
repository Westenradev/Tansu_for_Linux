use gtk4::prelude::*;
use tansu_core::ShelfItem;

/// One row in the shelf list: an icon plus the item's display name.
pub struct ShelfRow {
    row: gtk4::ListBoxRow,
}

impl ShelfRow {
    pub fn new(item: &ShelfItem) -> Self {
        let icon_name = match item {
            ShelfItem::File { .. } => "text-x-generic-symbolic",
            ShelfItem::Text { .. } => "text-x-generic-symbolic",
            ShelfItem::Image { .. } => "image-x-generic-symbolic",
        };
        let icon = gtk4::Image::from_icon_name(icon_name);
        let label = gtk4::Label::builder()
            .label(item.name())
            .xalign(0.0)
            .hexpand(true)
            .ellipsize(gtk4::pango::EllipsizeMode::End)
            .build();
        let box_ = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Horizontal)
            .spacing(8)
            .margin_top(6)
            .margin_bottom(6)
            .margin_start(6)
            .margin_end(6)
            .build();
        box_.append(&icon);
        box_.append(&label);

        let row = gtk4::ListBoxRow::builder().child(&box_).build();
        Self { row }
    }

    pub fn widget(&self) -> gtk4::ListBoxRow {
        self.row.clone()
    }
}
