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
    Image {
        mime: String,
        #[serde(with = "b64")]
        data: Vec<u8>,
    },
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

    /// A safe file name for exporting the item to disk (for example when it
    /// is dragged into a folder): sanitised name plus a fitting extension.
    pub fn export_file_name(&self) -> String {
        match self {
            ShelfItem::File { .. } => self.name(),
            ShelfItem::Text { .. } => format!("{}.txt", sanitize_file_stem(&self.name(), "Text")),
            ShelfItem::Image { mime, .. } => {
                let ext = match mime.as_str() {
                    "image/jpeg" => "jpg",
                    "image/gif" => "gif",
                    "image/webp" => "webp",
                    "image/bmp" => "bmp",
                    "image/svg+xml" => "svg",
                    "image/tiff" => "tif",
                    _ => "png",
                };
                format!("Image.{ext}")
            }
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

/// Turns arbitrary text into a safe file-name stem (no path separators or
/// control characters, no leading dots, never empty, at most 40 chars).
fn sanitize_file_stem(raw: &str, fallback: &str) -> String {
    let cleaned: String = raw
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                ' '
            } else {
                c
            }
        })
        .collect();
    let cleaned = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    let cleaned = cleaned
        .trim_start_matches(|c: char| c == '.' || c.is_whitespace())
        .to_string();
    let cleaned = truncate_chars(&cleaned, MAX_NAME_CHARS).trim().to_string();
    if cleaned.is_empty() {
        fallback.to_string()
    } else {
        cleaned
    }
}

/// Serde helper: image bytes are stored as a base64 string (about 1.3x the
/// size) instead of a JSON array of numbers (about 4x). The older array
/// form is still accepted when loading.
mod b64 {
    use serde::de::{self, SeqAccess, Visitor};
    use serde::{Deserializer, Serializer};
    use std::fmt;

    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    pub fn encode(data: &[u8]) -> String {
        let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
        for chunk in data.chunks(3) {
            let b = [
                chunk[0],
                *chunk.get(1).unwrap_or(&0),
                *chunk.get(2).unwrap_or(&0),
            ];
            let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
            out.push(TABLE[(n >> 18) as usize & 63] as char);
            out.push(TABLE[(n >> 12) as usize & 63] as char);
            out.push(if chunk.len() > 1 {
                TABLE[(n >> 6) as usize & 63] as char
            } else {
                '='
            });
            out.push(if chunk.len() > 2 {
                TABLE[n as usize & 63] as char
            } else {
                '='
            });
        }
        out
    }

    pub fn decode(text: &str) -> Option<Vec<u8>> {
        let bytes: Vec<u8> = text.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
        if bytes.len() & 3 != 0 {
            return None;
        }
        let value = |c: u8| TABLE.iter().position(|&t| t == c).map(|v| v as u32);
        let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
        for (i, quad) in bytes.chunks(4).enumerate() {
            let last = i == bytes.len() / 4 - 1;
            let pad = quad.iter().rev().take_while(|&&c| c == b'=').count();
            if pad > 2 || (pad > 0 && !last) {
                return None;
            }
            let mut n = 0u32;
            for &c in &quad[..4 - pad] {
                n = (n << 6) | value(c)?;
            }
            n <<= 6 * pad as u32;
            out.push((n >> 16) as u8);
            if pad < 2 {
                out.push((n >> 8) as u8);
            }
            if pad < 1 {
                out.push(n as u8);
            }
        }
        Some(out)
    }

    #[allow(clippy::ptr_arg)]
    pub fn serialize<S: Serializer>(data: &Vec<u8>, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&encode(data))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Vec<u8>;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a base64 string or an array of bytes")
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Vec<u8>, E> {
                decode(v).ok_or_else(|| E::custom("invalid base64"))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Vec<u8>, A::Error> {
                let mut out = Vec::new();
                while let Some(b) = seq.next_element::<u8>()? {
                    out.push(b);
                }
                Ok(out)
            }
        }
        d.deserialize_any(V)
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
    fn base64_roundtrips_all_lengths() {
        for len in 0..40usize {
            let data: Vec<u8> = (0..len).map(|i| (i * 37 + 11) as u8).collect();
            let text = b64::encode(&data);
            assert_eq!(b64::decode(&text), Some(data));
        }
        assert_eq!(b64::encode(b"Man"), "TWFu");
        assert_eq!(b64::encode(b"Ma"), "TWE=");
        assert_eq!(b64::encode(b"M"), "TQ==");
    }

    #[test]
    fn base64_rejects_garbage() {
        assert_eq!(b64::decode("abc"), None);
        assert_eq!(b64::decode("ab!d"), None);
        assert_eq!(b64::decode("TQ==TQ=="), None);
    }

    #[test]
    fn image_is_saved_as_base64_and_old_array_form_still_loads() {
        let item = ShelfItem::image("image/png", vec![1, 2, 3, 4]).unwrap();
        let json = serde_json::to_string(&item).unwrap();
        assert!(json.contains("\"AQIDBA==\""), "{json}");
        let back: ShelfItem = serde_json::from_str(&json).unwrap();
        assert_eq!(back, item);
        let old = r#"{"kind":"image","mime":"image/png","data":[1,2,3,4]}"#;
        assert_eq!(serde_json::from_str::<ShelfItem>(old).unwrap(), item);
    }

    #[test]
    fn export_file_names_are_safe() {
        let t = ShelfItem::text("  ../../etc/passwd: a*b?\nmore").unwrap();
        let n = t.export_file_name();
        assert!(n.ends_with(".txt"));
        assert!(!n.contains('/') && !n.contains(':') && !n.starts_with('.'));
        assert_eq!(
            ShelfItem::text("///").unwrap().export_file_name(),
            "Text.txt"
        );
        let img = ShelfItem::image("image/jpeg", vec![1]).unwrap();
        assert_eq!(img.export_file_name(), "Image.jpg");
    }

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
