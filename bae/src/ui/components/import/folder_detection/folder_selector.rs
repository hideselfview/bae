use dioxus::prelude::*;
use rfd::AsyncFileDialog;

#[component]
pub fn FolderSelector(on_select: EventHandler<String>, on_error: EventHandler<String>) -> Element {
    rsx! {
        div { class: "border border-gray-200 rounded-lg p-4",
            div { class: "flex items-center justify-between",
                div { class: "text-sm font-medium text-gray-900",
                    "Select a folder containing your music files"
                }
                button {
                    class: "px-4 py-2 bg-blue-600 text-white rounded hover:bg-blue-700",
                    onclick: move |_| {
                        spawn(async move {
                            if let Some(folder_handle) = AsyncFileDialog::new()
                                .set_title("Select Music Folder")
                                .pick_folder()
                                .await
                            {
                                let folder_path = folder_handle.path().to_string_lossy().to_string();
                                on_select.call(folder_path);
                            }
                        });
                    },
                    "Select Folder"
                }
            }
        }
    }
}
