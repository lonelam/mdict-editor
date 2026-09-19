//! Resource category derivation shared by registry, resolver and UI dispatch.

use serde::{Deserialize, Serialize};

/// Broad classification of a resource; drives tree grouping and editor choice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Category {
    Entry,
    Html,
    Css,
    Js,
    Image,
    Audio,
    Video,
    Font,
    Text,
    Other,
}

pub const ALL_CATEGORIES: [Category; 10] = [
    Category::Entry,
    Category::Html,
    Category::Css,
    Category::Js,
    Category::Image,
    Category::Audio,
    Category::Video,
    Category::Font,
    Category::Text,
    Category::Other,
];

impl Category {
    /// Classification from a resource key's extension. MDX entries are always
    /// [`Category::Entry`]; this handles keys of MDD resources and external files.
    pub fn from_key(key: &str) -> Category {
        let name = key.rsplit(['/']).next().unwrap_or(key);
        let ext = name.rsplit_once('.').map(|(_, e)| e).unwrap_or("");
        match ext.to_ascii_lowercase().as_str() {
            "html" | "htm" | "xhtml" => Category::Html,
            "css" => Category::Css,
            "js" | "mjs" => Category::Js,
            "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "svg" | "ico" => Category::Image,
            "mp3" | "wav" | "ogg" | "spx" | "m4a" | "aac" | "flac" => Category::Audio,
            "mp4" | "m4v" | "webm" | "mov" => Category::Video,
            "ttf" | "otf" | "woff" | "woff2" | "eot" | "ttc" => Category::Font,
            "txt" | "xml" | "csv" | "json" | "md" | "ini" | "srt" => Category::Text,
            "" => Category::Other,
            _ => Category::Other,
        }
    }

    /// MIME type used by the preview protocol and data URLs.
    pub fn mime_for(key: &str) -> &'static str {
        let name = key.rsplit(['/']).next().unwrap_or(key);
        let ext = name.rsplit_once('.').map(|(_, e)| e).unwrap_or("");
        match ext.to_ascii_lowercase().as_str() {
            "png" => "image/png",
            "jpg" | "jpeg" => "image/jpeg",
            "gif" => "image/gif",
            "webp" => "image/webp",
            "bmp" => "image/bmp",
            "svg" => "image/svg+xml",
            "ico" => "image/x-icon",
            "mp3" => "audio/mpeg",
            "wav" => "audio/wav",
            "ogg" | "spx" => "audio/ogg",
            "m4a" | "aac" => "audio/mp4",
            "flac" => "audio/flac",
            "mp4" | "m4v" => "video/mp4",
            "webm" => "video/webm",
            "mov" => "video/quicktime",
            "css" => "text/css",
            "js" | "mjs" => "text/javascript",
            "html" | "htm" | "xhtml" => "text/html",
            "json" => "application/json",
            "xml" => "application/xml",
            "txt" | "md" | "csv" | "ini" | "srt" => "text/plain",
            "ttf" => "font/ttf",
            "otf" => "font/otf",
            "woff" => "font/woff",
            "woff2" => "font/woff2",
            "ttc" | "eot" => "font/collection",
            _ => "application/octet-stream",
        }
    }

    pub fn is_text(self) -> bool {
        matches!(
            self,
            Category::Entry | Category::Html | Category::Css | Category::Js | Category::Text
        )
    }
}
