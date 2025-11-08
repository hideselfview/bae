use super::{
    folder_detection::FolderDetectionPage, import_workflow::ImportWorkflow,
    release_list::ReleaseList, search_masters::SearchMastersPage,
    search_musicbrainz::SearchMusicBrainzPage,
};
use crate::ui::import_context::{ImportContext, ImportStep};
use dioxus::prelude::*;
use std::rc::Rc;

/// Manages the import workflow and navigation between search and releases views
#[component]
pub fn ImportWorkflowManager() -> Element {
    let album_import_ctx = use_context::<Rc<ImportContext>>();

    let on_release_back = {
        let album_import_ctx = album_import_ctx.clone();
        move |_| album_import_ctx.navigate_back()
    };

    let current_step = album_import_ctx.current_step();

    match current_step {
        ImportStep::SearchResults => {
            rsx! {
                SearchMastersPage {}
            }
        }
        ImportStep::ReleaseDetails {
            master_id,
            master_title,
        } => {
            rsx! {
                ReleaseList {
                    master_id: master_id.clone(),
                    master_title: master_title.clone(),
                    on_back: on_release_back,
                }
            }
        }
        ImportStep::FolderIdentification { .. } => {
            rsx! {
                FolderDetectionPage {}
            }
        }
        ImportStep::MusicBrainzSearch { .. } => {
            rsx! {
                SearchMusicBrainzPage {}
            }
        }
        ImportStep::ImportWorkflow {
            master_id,
            release_id,
            master_year,
        } => {
            rsx! {
                ImportWorkflow {
                    master_id: master_id.clone(),
                    release_id: release_id.clone(),
                    master_year: master_year,
                }
            }
        }
    }
}
