//! Data model and persistence for the Tansu shelf.
//!
//! A shelf item is one of three kinds, mirroring the original Haiku app:
//! a reference to a file on disk, a text snippet, or an image held in
//! memory (and serialized as its original encoded bytes, e.g. PNG).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Maximum number of characters shown as the item's display name.
const MAX_NAME_CHARS: usize = 40;

/// A single item parked on the shelf.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum ShelfItem {
    /// A reference to a file or folder on disk. The path is the source of
    /// truth; the item is expected to follow renames and disappear when
    /// the file is deleted (handled by the UI layer via file monitoring).
    File { path: PathBuf },
    /// A UTF-8 text snippet.
    Text { text: String },
    /// An image, stored as its original encoded bytes (PNG, JPEG, ...).
    Image { mime: String, data: Vec<u8> },
}

impl ShelfItem {
    /// Creates a file item. Returns `None` if the path does not exist.
    pub fn file(path: impl AsRef<Path>) -> Option<Self> {
        let path = path.as_ref();
        if path.exists() {
            Some(ShelfItem::File {
                path: path.to_path_buf(),
            })
        } else {
            None
        }
    }

    /// Creates a text item. Returns `None` for empty text.
    pub fn text(text: impl Into<String>) -> Option<Self> {
        let text = text.into();
        if text.is_empty() {
            None
        } else {
            Some(ShelfItem::Text { text })
        }
    }

    /// Creates an image item. Returns `None` for empty data.
    pub fn image(mime: impl Into<String>, data: Vec<u8>) -> Option<Self> {
        if data.is_empty() {
            None
        } else {
            Some(ShelfItem::Image {
                mime: mime.into(),
                data,
            })
        }
    }

    /// Short display name, matching the original app: the file name for
    /// files, or the first line of the text (truncated) for text items.
    pub fn name(&self) -> String {
        match self {
            ShelfItem::File { path } => path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.to_string_lossy().into_owned()),
            ShelfItem::Text { text } => {
                let trimmed = text.trim();
                let first_line = trimmed.lines().next().unwrap_or("");
                let single_line = first_line.replace('\t', " ");
                truncate_chars(&single_line, MAX_NAME_CHARS)
            }
            ShelfItem::Image { .. } => "Image".to_string(),
        }
    }

    /// The MIME type offered when dragging the item out, if any.
    pub fn mime_type(&self) -> Option<&str> {
        match self {
            ShelfItem::Text { .. } => Some("text/plain"),
            ShelfItem::Image { mime, .. } if mime.starts_with("image/") => Some(mime),
            _ => None,
        }
    }

    /// Raw bytes written when the item is dropped onto the filesystem
    /// as a file: the text itself, or the encoded image bytes.
    pub fn payload(&self) -> Option<&[u8]> {
        match self {
            ShelfItem::Text { text } => Some(text.as_bytes()),
            ShelfItem::Image { data, .. } => Some(data),
            ShelfItem::File { .. } => None,
        }
    }
}

/// Truncates on character (not byte) boundaries, so UTF-8 stays valid.
fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        s.chars().take(max).collect()
    }
}

/// The whole shelf, persisted as JSON.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Shelf {
    pub items: Vec<ShelfItem>,
}

impl Shelf {
    /// Loads the shelf from `path`. A missing file yields an empty shelf.
    pub fn load(path: impl AsRef<Path>) -> io::Result<Shelf> {
        match fs::read(path) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Shelf::default()),
            Err(e) => Err(e),
        }
    }

    /// Saves the shelf atomically: write to a temp file in the same
    /// directory, then rename, so a crash never leaves a corrupt file.
    pub fn save(&self, path: impl AsRef<Path>) -> io::Result<()> {
        let path = path.as_ref();
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        let bytes = serde_json::to_vec_pretty(self)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        fs::write(&tmp, bytes)?;
        fs::rename(&tmp, path)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_item_rejects_empty() {
        assert!(ShelfItem::text("").is_none());
        assert!(ShelfItem::text("hello").is_some());
    }

    #[test]
    fn text_name_uses_first_line_truncated_on_char_boundaries() {
        let item = ShelfItem::text("  first line\nsecond line\n").unwrap();
        assert_eq!(item.name(), "first line");

        let long = "x".repeat(100);
        let item = ShelfItem::text(long).unwrap();
        assert_eq!(item.name().chars().count(), MAX_NAME_CHARS);

        // Multi-byte characters must not be split.
        let emoji = "🙂".repeat(50);
        let item = ShelfItem::text(emoji).unwrap();
        assert_eq!(item.name().chars().count(), MAX_NAME_CHARS);
        assert!(item.name().is_char_boundary(item.name().len()));
    }

    #[test]
    fn file_item_requires_existing_path() {
        assert!(ShelfItem::file("/definitely/not/here").is_none());
        assert!(ShelfItem::file("/tmp").is_some());
    }

    #[test]
    fn file_name_comes_from_path() {
        let item = ShelfItem::file("/tmp").unwrap();
        assert_eq!(item.name(), "tmp");
    }

    #[test]
    fn image_item_rejects_empty_data() {
        assert!(ShelfItem::image("image/png", vec![]).is_none());
        let item = ShelfItem::image("image/png", vec![1, 2, 3]).unwrap();
        assert_eq!(item.mime_type(), Some("image/png"));
        assert_eq!(item.payload(), Some(&[1, 2, 3][..]));
    }

    #[test]
    fn shelf_roundtrip_through_json() {
        let dir = std::env::temp_dir().join(format!("tansu-test-{}", std::process::id()));
        let path = dir.join("shelf.json");
        let _ = fs::remove_dir_all(&dir);

        let shelf = Shelf {
            items: vec![
                ShelfItem::text("hello shelf").unwrap(),
                ShelfItem::image("image/png", vec![137, 80, 78, 71]).unwrap(),
            ],
        };
        shelf.save(&path).unwrap();
        let loaded = Shelf::load(&path).unwrap();
        assert_eq!(shelf, loaded);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_missing_file_gives_empty_shelf() {
        let shelf = Shelf::load("/definitely/not/here/shelf.json").unwrap();
        assert!(shelf.items.is_empty());
    }

    #[test]
    fn load_corrupt_file_is_an_error_not_a_panic() {
        let dir = std::env::temp_dir().join(format!("tansu-test-corrupt-{}", std::process::id()));
        let path = dir.join("shelf.json");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(&path, b"{ not json").unwrap();
        assert!(Shelf::load(&path).is_err());
        let _ = fs::remove_dir_all(&dir);
    }
}
