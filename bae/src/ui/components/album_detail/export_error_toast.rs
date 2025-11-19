use dioxus::prelude::*;

#[component]
pub fn ExportErrorToast(error: String, on_dismiss: EventHandler<()>) -> Element {
    rsx! {
        div {
            class: "fixed bottom-4 right-4 bg-red-600 text-white px-6 py-4 rounded-lg shadow-lg z-50 max-w-md",
            div {
                class: "flex items-center justify-between gap-4",
                span { {error} }
                button {
                    class: "text-white hover:text-gray-200",
                    onclick: move |_| on_dismiss.call(()),
                    "✕"
                }
            }
        }
    }
}
