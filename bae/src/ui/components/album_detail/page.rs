use crate::db::ImportStatus;
use crate::import::ImportProgress;
use crate::library::{use_import_service, use_library_manager};
use crate::ui::Route;
use dioxus::prelude::*;

use super::back_button::BackButton;
use super::error::AlbumDetailError;
use super::loading::AlbumDetailLoading;
use super::utils::{get_selected_release_id_from_params, load_album_and_releases, maybe_not_empty};
use super::view::AlbumDetailView;
use crate::db::{DbAlbum, DbArtist, DbRelease, DbTrack};
use crate::library::LibraryError;

/// Album detail page showing album info and tracklist
#[component]
pub fn AlbumDetail(
    album_id: ReadOnlySignal<String>,
    release_id: ReadOnlySignal<String>, // May be empty string, will default to first release
) -> Element {
    let maybe_release_id = use_memo(move || maybe_not_empty(release_id()));
    let data = use_album_detail_data(album_id, maybe_release_id);
    let import_progress = use_release_progress(data.album, maybe_release_id);

    rsx! {
        PageContainer {
            BackButton {}
            match data.album.value().read().as_ref() {
                None => rsx! { AlbumDetailLoading {} },

                Some(Err(e)) => rsx! { AlbumDetailError { message: format!("Failed to load album: {e}") } },

                Some(Ok((album, releases))) => {
                    let selected_release_result = get_selected_release_id_from_params(&data.album, maybe_release_id())
                        .expect("Resource value should be present");

                    if let Err(e) = selected_release_result {
                        return rsx! { AlbumDetailError { message: format!("Failed to load release: {e}") } };
                    }

                    let selected_release_id = selected_release_result.ok().unwrap();

                    rsx! {
                        AlbumDetailView {
                            album: album.clone(),
                            releases: releases.clone(),
                            artists: data.artists.value().read().as_ref().and_then(|r| r.as_ref().ok()).cloned().unwrap_or_default(),
                            selected_release_id: selected_release_id,
                            on_release_select: move |new_release_id: String| {
                                navigator().push(Route::AlbumDetail {
                                    album_id: album_id().clone(),
                                    release_id: new_release_id,
                                });
                            },
                            tracks: data.tracks.value().read().as_ref().and_then(|r| r.as_ref().ok()).cloned().unwrap_or_default(),
                            import_progress: import_progress()
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn PageContainer(children: Element) -> Element {
    rsx! {
        div {
            class: "container mx-auto p-6",
            {children}
        }
    }
}

struct AlbumDetailData {
    album: Resource<Result<(DbAlbum, Vec<DbRelease>), LibraryError>>,
    tracks: Resource<Result<Vec<DbTrack>, LibraryError>>,
    artists: Resource<Result<Vec<DbArtist>, LibraryError>>,
}

fn use_album_detail_data(
    album_id: ReadOnlySignal<String>,
    maybe_release_id: Memo<Option<String>>,
) -> AlbumDetailData {
    let library_manager = use_library_manager();

    let album_resource = {
        let library_manager = library_manager.clone();
        use_resource(move || {
            let album_id = album_id();
            let library_manager = library_manager.clone();
            async move { load_album_and_releases(&library_manager, &album_id).await }
        })
    };

    let current_release_id = use_memo(move || {
        get_selected_release_id_from_params(&album_resource, maybe_release_id())
            .and_then(|r| r.ok())
    });

    let tracks_resource = {
        let library_manager = library_manager.clone();
        use_resource(move || {
            let release_id = current_release_id();
            let library_manager = library_manager.clone();
            async move {
                match release_id {
                    Some(id) => library_manager.get().get_tracks(&id).await,
                    None => Ok(Vec::new()),
                }
            }
        })
    };

    let current_album_id = use_memo(move || {
        album_resource
            .value()
            .read()
            .as_ref()
            .and_then(|result| result.as_ref().ok())
            .map(|(album, _)| album.id.clone())
    });

    let artists_resource = {
        let library_manager = library_manager.clone();
        use_resource(move || {
            let album_id = current_album_id();
            let library_manager = library_manager.clone();
            async move {
                match album_id {
                    Some(id) => library_manager.get().get_artists_for_album(&id).await,
                    None => Ok(Vec::new()),
                }
            }
        })
    };

    AlbumDetailData {
        album: album_resource,
        tracks: tracks_resource,
        artists: artists_resource,
    }
}

fn use_release_progress(
    album_resource: Resource<Result<(DbAlbum, Vec<DbRelease>), LibraryError>>,
    maybe_release_id: Memo<Option<String>>,
) -> Signal<Option<(usize, usize, u8)>> {
    let import_service = use_import_service();
    let mut progress = use_signal(|| None::<(usize, usize, u8)>);

    use_effect(move || {
        let releases_data = album_resource
            .value()
            .read()
            .as_ref()
            .and_then(|r| r.as_ref().ok())
            .map(|(_, releases)| releases.clone());

        let selected_id = get_selected_release_id_from_params(&album_resource, maybe_release_id())
            .and_then(|r| r.ok());

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

                            while let Some(progress_event) = progress_rx.recv().await {
                                match progress_event {
                                    ImportProgress::ProcessingProgress {
                                        current,
                                        total,
                                        percent,
                                        ..
                                    } => {
                                        progress.set(Some((current, total, percent)));
                                    }
                                    ImportProgress::Complete { .. }
                                    | ImportProgress::Failed { .. } => {
                                        progress.set(None);
                                        break;
                                    }
                                    _ => {}
                                }
                            }
                        });
                    } else {
                        progress.set(None);
                    }
                }
            }
        }
    });

    progress
}
