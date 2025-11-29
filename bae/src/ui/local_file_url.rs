//! Helper for converting local file paths to bae:// URLs
//!
//! The bae:// custom protocol is registered in app.rs and serves local files
//! through the webview. This avoids issues with file:// URLs which don't work
//! reliably in Dioxus desktop webviews.

/// Convert a local file path to a bae:// URL for serving via custom protocol.
///
/// The path will be URL-encoded to handle special characters.
///
/// # Example
/// ```
/// let url = local_file_url("/Users/me/Music/cover.jpg");
/// // Returns: bae://local%2FUsers%2Fme%2FMusic%2Fcover.jpg
/// ```
pub fn local_file_url(path: &str) -> String {
    format!("bae://local{}", urlencoding::encode(path))
}
