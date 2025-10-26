use crate::db::ImportStatus;
use crate::import::ImportProgress;
use crate::library::{use_import_service, use_library_manager};
use crate::ui::Route;
use dioxus::prelude::*;
use std::collections::HashSet;

use super::utils::{load_album_and_releases, maybe_not_empty_string};
use super::view::AlbumDetailView;

/// Album detail page showing album info and tracklist
#[component]
pub fn AlbumDetail(
    album_id: String,
    release_id: String, // May be empty string, will default to first release
) -> Element {
    let nav = navigator();
    let library_manager = use_library_manager();
    let import_service = use_import_service();
    let mut import_progress = use_signal(|| None::<(usize, usize, u8)>);
    let mut completed_tracks = use_signal(HashSet::<String>::new);

    let release_id = maybe_not_empty_string(release_id);

    let album_resource = use_resource({
        let album_id = album_id.clone();
        let library_manager = library_manager.clone();
        move || {
            let album_id = album_id.clone();
            let library_manager = library_manager.clone();
            async move { load_album_and_releases(&library_manager, &album_id).await }
        }
    });

    // Determine which release to show based on URL or default to first
    let current_release_id = album_resource
        .value()
        .read()
        .as_ref()
        .and_then(|result| result.as_ref().ok())
        .and_then(|(_, releases)| match &release_id {
            Some(id) => releases.iter().find(|r| &r.id == id).map(|r| r.id.clone()),
            None => releases.first().map(|r| r.id.clone()),
        });

    let tracks_resource = use_resource({
        let library_manager = library_manager.clone();
        move || {
            let release_id = current_release_id.clone();
            let library_manager = library_manager.clone();
            async move {
                match release_id {
                    Some(id) => library_manager.get().get_tracks(&id).await,
                    None => Ok(Vec::new()),
                }
            }
        }
    });

    let current_album_id = album_resource
        .value()
        .read()
        .as_ref()
        .and_then(|result| result.as_ref().ok())
        .map(|(album, _)| album.id.clone());

    let artists_resource = use_resource({
        let library_manager = library_manager.clone();
        move || {
            let album_id = current_album_id.clone();
            let library_manager = library_manager.clone();
            async move {
                match album_id {
                    Some(id) => library_manager.get().get_artists_for_album(&id).await,
                    None => Ok(Vec::new()),
                }
            }
        }
    });

    // Subscribe to import progress for the selected release
    use_effect({
        let release_id_for_effect = release_id.clone();
        move || {
            let releases_data = album_resource
                .value()
                .read()
                .as_ref()
                .and_then(|r| r.as_ref().ok())
                .map(|(_, releases)| releases.clone());
            let selected_id = album_resource
                .value()
                .read()
                .as_ref()
                .and_then(|result| result.as_ref().ok())
                .and_then(|(_, releases)| match &release_id_for_effect {
                    Some(id) => releases.iter().find(|r| &r.id == id).map(|r| r.id.clone()),
                    None => releases.first().map(|r| r.id.clone()),
                });

            if let Some(releases) = releases_data {
                if let Some(ref id) = selected_id {
                    if let Some(release) = releases.iter().find(|r| &r.id == id) {
                        if release.import_status == ImportStatus::Importing
                            || release.import_status == ImportStatus::Queued
                        {
                            let release_id = release.id.clone();
                            let import_service = import_service.clone();

                            spawn(async move {
                                let mut progress_rx = import_service.subscribe_release(release_id);

                                while let Some(progress) = progress_rx.recv().await {
                                    match progress {
                                        ImportProgress::ProcessingProgress {
                                            current,
                                            total,
                                            percent,
                                            ..
                                        } => {
                                            import_progress.set(Some((current, total, percent)));
                                        }
                                        ImportProgress::TrackComplete { track_id, .. } => {
                                            completed_tracks.write().insert(track_id);
                                        }
                                        ImportProgress::Complete { .. } => {
                                            import_progress.set(None);
                                            break;
                                        }
                                        ImportProgress::Failed { .. } => {
                                            import_progress.set(None);
                                            break;
                                        }
                                        _ => {}
                                    }
                                }
                            });
                        } else {
                            import_progress.set(None);
                            completed_tracks.set(HashSet::new());
                        }
                    }
                }
            }
        }
    });

    rsx! {
        div {
            class: "container mx-auto p-6",

            div {
                class: "mb-6",
                Link {
                    to: Route::Library {},
                    class: "inline-flex items-center text-blue-400 hover:text-blue-300 transition-colors",
                    "← Back to Library"
                }
            }

            match album_resource.value().read().as_ref() {
                None => rsx! {
                    div {
                        class: "flex justify-center items-center py-12",
                        div {
                            class: "animate-spin rounded-full h-12 w-12 border-b-2 border-blue-500"
                        }
                        p {
                            class: "ml-4 text-gray-300",
                            "Loading album details..."
                        }
                    }
                },
                Some(Err(e)) => rsx! {
                    div {
                        class: "bg-red-900 border border-red-700 text-red-100 px-4 py-3 rounded mb-4",
                        p { "Failed to load album: {e}" }
                    }
                },
                Some(Ok((album, releases))) => {
                    let selected_id = match &release_id {
                        Some(id) => releases.iter().find(|r| &r.id == id).map(|r| r.id.clone()),
                        None => releases.first().map(|r| r.id.clone()),
                    };
                    let completed_track_ids: Vec<String> = completed_tracks().iter().cloned().collect();
                    rsx! {
                        AlbumDetailView {
                            album: album.clone(),
                            releases: releases.clone(),
                            artists: artists_resource.value().read().as_ref().and_then(|r| r.as_ref().ok()).cloned().unwrap_or_default(),
                            selected_release_id: selected_id,
                            on_release_select: move |new_release_id: String| {
                                nav.push(Route::AlbumDetail {
                                    album_id: album_id.clone(),
                                    release_id: new_release_id,
                                });
                            },
                            tracks: tracks_resource.value().read().as_ref().and_then(|r| r.as_ref().ok()).cloned().unwrap_or_default(),
                            import_progress: import_progress(),
                            completed_tracks: completed_track_ids
                        }
                    }
                }
            }
        }
    }
}
