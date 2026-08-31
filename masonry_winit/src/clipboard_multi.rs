//! Windows multi-format clipboard backend.
//!
//! Bypasses `copypasta` (which only exposes `get_contents`/`set_contents`
//! String API) and calls `clipboard-win` directly so we can register custom
//! formats (`HTML Format`, `application/x-suess-rich+json`) and write/read
//! raw bytes without NUL truncation.

use clipboard_win::raw::{
    get_vec, register_format, set_html, set_string_with, set_without_clear,
};
use clipboard_win::options::NoClear;
use masonry_core::core::ClipboardFormat;

const RICH_MIME: &str = "application/x-suess-rich+json";
const HTML_FORMAT_NAME: &str = "HTML Format";

/// Windows multi-format clipboard backend. Caches registered format codes.
#[derive(Debug)]
pub struct WindowsMultiClipboard {
    html_fmt: u32,
    rich_fmt: u32,
}

impl WindowsMultiClipboard {
    /// Register the two custom formats. Safe to call once; the format codes
    /// are stable for the process lifetime.
    pub fn new() -> Self {
        let html_fmt = register_format(HTML_FORMAT_NAME)
            .map(|n| n.get())
            .unwrap_or(0);
        let rich_fmt = register_format(RICH_MIME)
            .map(|n| n.get())
            .unwrap_or(0);
        Self { html_fmt, rich_fmt }
    }

    /// Write all formats atomically: open clipboard, empty once, then
    /// set_without_clear per format so they coexist.
    pub fn set(&mut self, formats: &[ClipboardFormat]) -> Result<(), Box<dyn std::error::Error>> {
        let _clip = clipboard_win::Clipboard::new_attempts(10)?;
        clipboard_win::raw::empty()?;
        for fmt in formats {
            match fmt.mime.as_str() {
                "text/plain" => {
                    let text = std::str::from_utf8(&fmt.data).unwrap_or("");
                    let _ = set_string_with(text, NoClear);
                }
                "text/html" => {
                    if self.html_fmt != 0 {
                        let fragment = std::str::from_utf8(&fmt.data).unwrap_or("");
                        let _ = set_html(self.html_fmt, fragment);
                    }
                }
                _ if fmt.mime == RICH_MIME => {
                    if self.rich_fmt != 0 {
                        let _ = set_without_clear(self.rich_fmt, &fmt.data);
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// Read the custom `application/x-suess-rich+json` format, if present.
    /// Returns raw bytes — no NUL truncation.
    pub fn get_rich(&self) -> Option<Vec<u8>> {
        if self.rich_fmt == 0 {
            return None;
        }
        let _clip = clipboard_win::Clipboard::new_attempts(10).ok()?;
        let mut out = Vec::new();
        get_vec(self.rich_fmt, &mut out).ok()?;
        Some(out)
    }
}


