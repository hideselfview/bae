use crate::ui::Route;
use dioxus::desktop::use_window;
use dioxus::prelude::*;

#[cfg(target_os = "macos")]
use cocoa::appkit::NSApplication;
#[cfg(target_os = "macos")]
use cocoa::base::{id, nil};
#[cfg(target_os = "macos")]
use objc::{msg_send, sel, sel_impl};

/// Custom title bar component with navigation (macOS: native traffic lights + nav)
#[component]
pub fn TitleBar() -> Element {
    let window = use_window();
    let current_route = use_route::<Route>();

    rsx! {
        div {
            id: "title-bar",
            class: "fixed top-0 left-0 right-0 h-10 bg-[#1e222d] flex items-center pl-20 pr-2 cursor-move z-[1000] border-b border-[#2d3138]",
            onmousedown: move |_| {
                let _ = window.drag_window();
            },
            ondoubleclick: move |_| {
                perform_zoom();
            },
            div {
                class: "flex gap-2 flex-none items-center",
                style: "-webkit-app-region: no-drag;",
                NavButton {
                    route: Route::Library {},
                    label: "Library",
                    is_active: matches!(current_route, Route::Library {} | Route::AlbumDetail { .. })
                }
                NavButton {
                    route: Route::ImportWorkflowManager {},
                    label: "Import",
                    is_active: matches!(current_route, Route::ImportWorkflowManager {})
                }
                NavButton {
                    route: Route::Settings {},
                    label: "Settings",
                    is_active: matches!(current_route, Route::Settings {})
                }
            }
        }
    }
}

/// Navigation button component for titlebar
#[component]
fn NavButton(route: Route, label: &'static str, is_active: bool) -> Element {
    rsx! {
        span {
            class: "inline-block",
            onmousedown: move |evt| {
                evt.stop_propagation();
            },
            Link {
                to: route,
                class: if is_active {
                    "text-white no-underline text-[12px] cursor-pointer px-2 py-1 rounded bg-gray-700"
                } else {
                    "text-gray-400 no-underline text-[12px] cursor-pointer px-2 py-1 rounded hover:bg-gray-800 hover:text-white transition-colors"
                },
                "{label}"
            }
        }
    }
}

/// Perform window zoom (maximize/restore) using native macOS API
#[cfg(target_os = "macos")]
fn perform_zoom() {
    unsafe {
        let app = NSApplication::sharedApplication(nil);
        let window: id = msg_send![app, keyWindow];

        if window != nil {
            let _: () = msg_send![window, performSelector: sel!(performZoom:) withObject: nil];
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn perform_zoom() {
    // No-op on non-macOS platforms
}
