use dioxus::desktop::{wry, Config as DioxusConfig, WindowBuilder};
use dioxus::prelude::*;
use std::borrow::Cow;
use tracing::warn;
use wry::http::Response as HttpResponse;

use crate::ui::components::import::ImportWorkflowManager;
use crate::ui::components::*;
#[cfg(target_os = "macos")]
use crate::ui::window_activation::setup_macos_window_activation;
use crate::ui::AppContext;

pub const FAVICON: Asset = asset!("/assets/favicon.ico");
pub const MAIN_CSS: Asset = asset!("/assets/main.css");
pub const TAILWIND_CSS: Asset = asset!("/assets/tailwind.css");

#[derive(Debug, Clone, Routable, PartialEq)]
#[rustfmt::skip]
pub enum Route {
    #[layout(Navbar)]
    #[route("/")]
    Library {},
    #[route("/album/:album_id?:release_id")]
    AlbumDetail { 
        album_id: String,
        release_id: String,
    },
    #[route("/import")]
    ImportWorkflowManager {},
    #[route("/settings")]
    Settings {},
}

/// Get MIME type from file extension
fn mime_type_for_extension(ext: &str) -> &'static str {
    match ext.to_lowercase().as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "ico" => "image/x-icon",
        "svg" => "image/svg+xml",
        "tiff" | "tif" => "image/tiff",
        _ => "application/octet-stream",
    }
}

pub fn make_config() -> DioxusConfig {
    DioxusConfig::default()
        .with_window(make_window())
        // Enable native file drop handler (false = don't disable) to get full file paths
        // On macOS/Linux: Native handler captures paths and merges them with HTML drag events
        // On Windows: Native handler captures paths and uses WindowsDragDrop events to bridge to HTML drag events
        .with_disable_drag_drop_handler(false)
        // Custom protocol for serving local files (images, etc.)
        // Usage: bae://local/path/to/file.jpg
        .with_asynchronous_custom_protocol("bae", |_webview_id, request, responder| {
            let uri = request.uri().to_string();

            // Parse the URI: bae://local/path/to/file
            // The path comes after "bae://local"
            let path = if uri.starts_with("bae://local") {
                // URL decode the path
                let encoded_path = uri.strip_prefix("bae://local").unwrap_or("");
                urlencoding::decode(encoded_path)
                    .map(|s| s.into_owned())
                    .unwrap_or_else(|_| encoded_path.to_string())
            } else {
                warn!("Invalid bae:// URL: {}", uri);
                responder.respond(
                    HttpResponse::builder()
                        .status(400)
                        .body(Cow::Borrowed(b"Invalid URL" as &[u8]))
                        .unwrap(),
                );
                return;
            };

            // Spawn async task to read the file
            tokio::spawn(async move {
                match tokio::fs::read(&path).await {
                    Ok(data) => {
                        // Determine MIME type from extension
                        let mime_type = std::path::Path::new(&path)
                            .extension()
                            .and_then(|e| e.to_str())
                            .map(mime_type_for_extension)
                            .unwrap_or("application/octet-stream");

                        responder.respond(
                            HttpResponse::builder()
                                .status(200)
                                .header("Content-Type", mime_type)
                                .body(Cow::Owned(data))
                                .unwrap(),
                        );
                    }
                    Err(e) => {
                        warn!("Failed to read file {}: {}", path, e);
                        responder.respond(
                            HttpResponse::builder()
                                .status(404)
                                .body(Cow::Borrowed(b"File not found" as &[u8]))
                                .unwrap(),
                        );
                    }
                }
            });
        })
}

fn make_window() -> WindowBuilder {
    WindowBuilder::new()
        .with_title("bae")
        .with_always_on_top(false)
        .with_decorations(true)
        .with_inner_size(dioxus::desktop::LogicalSize::new(1200, 800))
}

pub fn launch_app(context: AppContext) {
    #[cfg(target_os = "macos")]
    setup_macos_window_activation();

    LaunchBuilder::desktop()
        .with_cfg(make_config())
        .with_context_provider(move || Box::new(context.clone()))
        .launch(App);
}
