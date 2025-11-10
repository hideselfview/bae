use super::file_list::FileInfo;
use super::{
    file_list::FileList, folder_selector::FolderSelector, manual_search_panel::ManualSearchPanel,
    match_list::MatchList,
};
use crate::import::{ImportRequestParams, MatchCandidate, MatchSource};
use crate::library::use_import_service;
use crate::library::use_library_manager;
use crate::musicbrainz::lookup_by_discid;
use crate::ui::import_context::ImportContext;
use crate::ui::Route;
use dioxus::prelude::*;
use std::path::PathBuf;
use std::rc::Rc;
use tracing::info;

#[component]
pub fn FolderDetectionPage() -> Element {
    let import_context = use_context::<Rc<ImportContext>>();
    let library_manager = use_library_manager();
    let import_service = use_import_service();
    let navigator = use_navigator();

    // Copy signals out of Rc (signals are Copy)
    let folder_path = import_context.folder_path;
    let detected_metadata = import_context.detected_metadata;
    let import_phase = import_context.import_phase;
    let exact_match_candidates = import_context.exact_match_candidates;
    let selected_match_index = import_context.selected_match_index;
    let confirmed_candidate = import_context.confirmed_candidate;
    let is_detecting = import_context.is_detecting;
    let is_looking_up = import_context.is_looking_up;
    let import_error_message = import_context.import_error_message;
    let duplicate_album_id = import_context.duplicate_album_id;
    let search_query = import_context.search_query;
    let folder_files = import_context.folder_files;

    let on_folder_select = {
        let import_context_for_detect = import_context.clone();

        let mut folder_path = folder_path;
        let mut detected_metadata = detected_metadata;
        let mut exact_match_candidates = exact_match_candidates;
        let mut selected_match_index = selected_match_index;
        let mut confirmed_candidate = confirmed_candidate;
        let mut import_error_message = import_error_message;
        let mut duplicate_album_id = duplicate_album_id;
        let mut import_phase = import_phase;
        let mut is_detecting = is_detecting;

        move |path: String| {
            folder_path.set(path.clone());
            detected_metadata.set(None);
            exact_match_candidates.set(Vec::new());
            selected_match_index.set(None);
            confirmed_candidate.set(None);
            import_error_message.set(None);
            duplicate_album_id.set(None);
            import_phase.set(crate::ui::import_context::ImportPhase::MetadataDetection);
            is_detecting.set(true);

            // Read files from folder
            let folder_path_clone = path.clone();
            let import_context_for_files = import_context_for_detect.clone();
            let mut folder_files_for_read = import_context_for_files.folder_files;
            spawn(async move {
                let mut files = Vec::new();
                if let Ok(entries) = std::fs::read_dir(&folder_path_clone) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if path.is_file() {
                            let name = path
                                .file_name()
                                .and_then(|n| n.to_str())
                                .unwrap_or("unknown")
                                .to_string();
                            let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
                            let format = path
                                .extension()
                                .and_then(|e| e.to_str())
                                .unwrap_or("")
                                .to_uppercase();
                            files.push(FileInfo { name, size, format });
                        }
                    }
                    files.sort_by(|a, b| a.name.cmp(&b.name));
                }
                folder_files_for_read.set(files);
            });

            let import_context_for_detect = import_context_for_detect.clone();
            let mut detected_metadata = import_context_for_detect.detected_metadata;
            let mut is_detecting = import_context_for_detect.is_detecting;
            let mut is_looking_up = import_context_for_detect.is_looking_up;
            let mut import_phase = import_context_for_detect.import_phase;
            let mut confirmed_candidate = import_context_for_detect.confirmed_candidate;
            let mut exact_match_candidates = import_context_for_detect.exact_match_candidates;
            let mut import_error_message = import_context_for_detect.import_error_message;
            let mut search_query = import_context_for_detect.search_query;

            spawn(async move {
                // Helper to initialize search query from metadata
                let mut init_search_query = |metadata: &crate::import::FolderMetadata| {
                    let mut query_parts = Vec::new();
                    if let Some(ref artist) = metadata.artist {
                        query_parts.push(artist.clone());
                    }
                    if let Some(ref album) = metadata.album {
                        query_parts.push(album.clone());
                    }
                    search_query.set(query_parts.join(" "));
                };

                // Phase 2: Detect metadata
                match import_context_for_detect
                    .detect_folder_metadata(path.clone())
                    .await
                {
                    Ok(metadata) => {
                        detected_metadata.set(Some(metadata.clone()));
                        is_detecting.set(false);

                        // Phase 3: Exact lookup if MB DiscID available
                        if let Some(ref mb_discid) = metadata.mb_discid {
                            is_looking_up.set(true);
                            info!("🎵 Found MB DiscID: {}, performing exact lookup", mb_discid);

                            match lookup_by_discid(mb_discid).await {
                                Ok((releases, _external_urls)) => {
                                    if releases.is_empty() {
                                        info!(
                                            "No exact matches found, proceeding to manual search"
                                        );
                                        init_search_query(&metadata);
                                        import_phase.set(
                                            crate::ui::import_context::ImportPhase::ManualSearch,
                                        );
                                    } else if releases.len() == 1 {
                                        // Single exact match - auto-proceed to confirmation
                                        info!("✅ Single exact match found, auto-proceeding");
                                        let mb_release = releases[0].clone();
                                        let candidate = MatchCandidate {
                                            source: MatchSource::MusicBrainz(mb_release),
                                            confidence: 100.0,
                                            match_reasons: vec!["Exact DiscID match".to_string()],
                                        };
                                        confirmed_candidate.set(Some(candidate));
                                        import_phase.set(
                                            crate::ui::import_context::ImportPhase::Confirmation,
                                        );
                                    } else {
                                        // Multiple exact matches - show for selection
                                        info!(
                                            "Found {} exact matches, showing for selection",
                                            releases.len()
                                        );
                                        let candidates: Vec<MatchCandidate> = releases
                                            .into_iter()
                                            .map(|mb_release| MatchCandidate {
                                                source: MatchSource::MusicBrainz(mb_release),
                                                confidence: 100.0,
                                                match_reasons: vec![
                                                    "Exact DiscID match".to_string()
                                                ],
                                            })
                                            .collect();
                                        exact_match_candidates.set(candidates);
                                        import_phase.set(
                                            crate::ui::import_context::ImportPhase::ExactLookup,
                                        );
                                    }
                                    is_looking_up.set(false);
                                }
                                Err(e) => {
                                    info!(
                                        "MB DiscID lookup failed: {}, proceeding to manual search",
                                        e
                                    );
                                    is_looking_up.set(false);
                                    init_search_query(&metadata);
                                    import_phase
                                        .set(crate::ui::import_context::ImportPhase::ManualSearch);
                                }
                            }
                        } else {
                            // No MB DiscID, proceed to manual search
                            info!("No MB DiscID found, proceeding to manual search");
                            init_search_query(&metadata);
                            import_phase.set(crate::ui::import_context::ImportPhase::ManualSearch);
                        }
                    }
                    Err(e) => {
                        import_error_message.set(Some(e));
                        is_detecting.set(false);
                        import_phase.set(crate::ui::import_context::ImportPhase::FolderSelection);
                    }
                }
            });
        }
    };

    let on_exact_match_select = {
        let mut selected_match_index = selected_match_index;
        let mut confirmed_candidate = confirmed_candidate;
        let mut import_phase = import_phase;
        move |index: usize| {
            selected_match_index.set(Some(index));
            if let Some(candidate) = exact_match_candidates.read().get(index) {
                confirmed_candidate.set(Some(candidate.clone()));
                import_phase.set(crate::ui::import_context::ImportPhase::Confirmation);
            }
        }
    };

    let on_manual_match_select = {
        let mut selected_match_index = selected_match_index;
        move |index: usize| {
            selected_match_index.set(Some(index));
        }
    };

    let on_confirm_from_manual = {
        let import_context_for_reset = import_context.clone();
        let library_manager = library_manager.clone();
        let import_service = import_service.clone();
        move |candidate: MatchCandidate| {
            let folder = folder_path.read().clone();
            let metadata = detected_metadata.read().clone();
            let import_service = import_service.clone();
            let mut duplicate_album_id = duplicate_album_id;
            let mut import_error_message = import_error_message;
            let import_context_for_reset = import_context_for_reset.clone();
            let library_manager = library_manager.clone();

            spawn(async move {
                // Check for duplicates before importing
                match &candidate.source {
                    MatchSource::Discogs(discogs_result) => {
                        let master_id = discogs_result.master_id.map(|id| id.to_string());
                        let release_id = Some(discogs_result.id.to_string());

                        if let Ok(Some(duplicate)) = library_manager
                            .get()
                            .find_duplicate_by_discogs(master_id.as_deref(), release_id.as_deref())
                            .await
                        {
                            duplicate_album_id.set(Some(duplicate.id));
                            import_error_message.set(Some(format!(
                                "This release already exists in your library: {}",
                                duplicate.title
                            )));
                            return;
                        }
                    }
                    MatchSource::MusicBrainz(mb_release) => {
                        let release_id = Some(mb_release.release_id.clone());
                        let release_group_id = Some(mb_release.release_group_id.clone());

                        if let Ok(Some(duplicate)) = library_manager
                            .get()
                            .find_duplicate_by_musicbrainz(
                                release_id.as_deref(),
                                release_group_id.as_deref(),
                            )
                            .await
                        {
                            duplicate_album_id.set(Some(duplicate.id));
                            import_error_message.set(Some(format!(
                                "This release already exists in your library: {}",
                                duplicate.title
                            )));
                            return;
                        }
                    }
                }

                // Extract master_year from metadata or release date
                let master_year = metadata.as_ref().and_then(|m| m.year).unwrap_or(1970);

                match candidate.source.clone() {
                    MatchSource::Discogs(discogs_result) => {
                        let master_id = match discogs_result.master_id {
                            Some(id) => id.to_string(),
                            None => {
                                import_error_message
                                    .set(Some("Discogs result has no master_id".to_string()));
                                return;
                            }
                        };
                        let release_id = discogs_result.id.to_string();

                        match import_context_for_reset
                            .import_release(release_id, master_id)
                            .await
                        {
                            Ok(discogs_release) => {
                                info!(
                                    "Starting import for Discogs release: {}",
                                    discogs_release.title
                                );

                                let request = ImportRequestParams::FromFolder {
                                    discogs_release: Some(discogs_release),
                                    mb_release: None,
                                    folder: PathBuf::from(folder),
                                    master_year,
                                };

                                match import_service.send_request(request).await {
                                    Ok((album_id, _release_id)) => {
                                        info!("Import started, navigating to album: {}", album_id);
                                        // Reset import state before navigating
                                        import_context_for_reset.reset();
                                        navigator.push(Route::AlbumDetail {
                                            album_id,
                                            release_id: String::new(),
                                        });
                                    }
                                    Err(e) => {
                                        import_error_message
                                            .set(Some(format!("Failed to start import: {}", e)));
                                    }
                                }
                            }
                            Err(e) => {
                                import_error_message
                                    .set(Some(format!("Failed to fetch Discogs release: {}", e)));
                            }
                        }
                    }
                    MatchSource::MusicBrainz(mb_release) => {
                        info!(
                            "Starting import for MusicBrainz release: {}",
                            mb_release.title
                        );

                        let request = ImportRequestParams::FromFolder {
                            discogs_release: None,
                            mb_release: Some(mb_release.clone()),
                            folder: PathBuf::from(folder),
                            master_year,
                        };

                        match import_service.send_request(request).await {
                            Ok((album_id, _release_id)) => {
                                info!("Import started, navigating to album: {}", album_id);
                                // Reset import state before navigating
                                import_context_for_reset.reset();
                                navigator.push(Route::AlbumDetail {
                                    album_id,
                                    release_id: String::new(),
                                });
                            }
                            Err(e) => {
                                import_error_message
                                    .set(Some(format!("Failed to start import: {}", e)));
                            }
                        }
                    }
                }
            });
        }
    };

    let on_edit = {
        let mut confirmed_candidate = confirmed_candidate;
        let mut selected_match_index = selected_match_index;
        let mut import_phase = import_phase;
        let mut search_query = search_query;
        move |_| {
            confirmed_candidate.set(None);
            selected_match_index.set(None);
            if !exact_match_candidates.read().is_empty() {
                import_phase.set(crate::ui::import_context::ImportPhase::ExactLookup);
            } else {
                // Initialize search query from detected metadata when transitioning to manual search
                if let Some(metadata) = detected_metadata.read().as_ref() {
                    let mut query_parts = Vec::new();
                    if let Some(ref artist) = metadata.artist {
                        query_parts.push(artist.clone());
                    }
                    if let Some(ref album) = metadata.album {
                        query_parts.push(album.clone());
                    }
                    if !query_parts.is_empty() {
                        search_query.set(query_parts.join(" "));
                    }
                }
                import_phase.set(crate::ui::import_context::ImportPhase::ManualSearch);
            }
        }
    };

    let on_confirm = move |_| {
        if let Some(candidate) = confirmed_candidate.read().as_ref().cloned() {
            on_confirm_from_manual(candidate);
        }
    };

    let on_change_folder = {
        let import_context = import_context.clone();
        move |_| {
            import_context.reset();
        }
    };

    rsx! {
        div { class: "max-w-4xl mx-auto p-6",
            div { class: "mb-6",
                h1 { class: "text-2xl font-bold text-white", "Import" }
            }

            // Phase 1: Folder Selection
            if *import_phase.read() == crate::ui::import_context::ImportPhase::FolderSelection {
                div { class: "bg-white rounded-lg shadow p-6",
                    FolderSelector {
                        on_select: on_folder_select,
                        on_error: {
                            let mut import_error_message = import_error_message;
                            move |e: String| {
                                import_error_message.set(Some(e));
                            }
                        }
                    }
                }
            } else {
                div { class: "space-y-6",
                    // Show selected folder
                    div { class: "bg-white rounded-lg shadow p-6",
                        div { class: "mb-6 pb-4 border-b border-gray-200",
                            div { class: "flex items-start justify-between mb-3",
                                h3 { class: "text-sm font-semibold text-gray-700 uppercase tracking-wide", "Selected Folder" }
                                button {
                                    class: "px-3 py-1 text-sm text-blue-600 hover:text-blue-800 hover:bg-blue-50 rounded-md transition-colors",
                                    onclick: on_change_folder,
                                    "Clear"
                                }
                            }
                            div { class: "inline-block px-4 py-2 bg-gray-100 hover:bg-gray-200 rounded-full border border-gray-300 transition-colors",
                                p {
                                    class: "text-sm text-gray-900 font-mono select-text cursor-text break-all",
                                    "{folder_path.read()}"
                                }
                            }
                        }

                        if *is_detecting.read() {
                            div { class: "text-center py-8",
                                p { class: "text-gray-600", "Detecting metadata..." }
                            }
                        } else if !folder_files.read().is_empty() {
                            div { class: "mt-4",
                                h4 { class: "text-sm font-semibold text-gray-700 uppercase tracking-wide mb-3", "Files" }
                                FileList {
                                    files: folder_files.read().clone(),
                                }
                            }
                        }
                    }

                    // Phase 2: Metadata Detection (handled in on_folder_select)

                    // Phase 3: Exact Lookup
                    if *import_phase.read() == crate::ui::import_context::ImportPhase::ExactLookup {
                        if *is_looking_up.read() {
                            div { class: "bg-white rounded-lg shadow p-6 text-center",
                                p { class: "text-gray-600", "Looking up release by DiscID..." }
                            }
                        } else if !exact_match_candidates.read().is_empty() {
                            div { class: "bg-white rounded-lg shadow p-6",
                                h3 { class: "text-lg font-semibold text-gray-900 mb-4", "Multiple Exact Matches Found" }
                                p { class: "text-sm text-gray-600 mb-4", "Select the correct release:" }
                                MatchList {
                                    candidates: exact_match_candidates.read().clone(),
                                    selected_index: selected_match_index.read().as_ref().copied(),
                                    on_select: on_exact_match_select,
                                }
                            }
                        }
                    }

                    // Phase 4: Manual Search
                    if *import_phase.read() == crate::ui::import_context::ImportPhase::ManualSearch {
                        ManualSearchPanel {
                            detected_metadata: detected_metadata,
                            on_match_select: on_manual_match_select,
                            on_confirm: {
                                let mut confirmed_candidate = confirmed_candidate;
                                let mut import_phase = import_phase;
                                move |candidate: MatchCandidate| {
                                    confirmed_candidate.set(Some(candidate.clone()));
                                    import_phase.set(crate::ui::import_context::ImportPhase::Confirmation);
                                }
                            },
                            selected_index: selected_match_index,
                        }
                    }

                    // Phase 5: Confirmation
                    if *import_phase.read() == crate::ui::import_context::ImportPhase::Confirmation {
                        if let Some(candidate) = confirmed_candidate.read().as_ref() {
                            div { class: "space-y-4",
                                div { class: "bg-blue-50 border-2 border-blue-500 rounded-lg p-6",
                                    div { class: "flex items-start justify-between mb-4",
                                        div { class: "flex-1",
                                            h3 { class: "text-lg font-semibold text-gray-900 mb-2",
                                                "Selected Release"
                                            }
                                            div { class: "text-sm text-gray-600 space-y-1",
                                                p { class: "text-lg font-medium text-gray-900", "{candidate.title()}" }
                                                if let Some(ref year) = candidate.year() {
                                                    p { "Year: {year}" }
                                                }
                                                {
                                                    let (format_text, country_text, label_text) = match &candidate.source {
                                                        MatchSource::MusicBrainz(release) => (
                                                            release.format.as_ref().map(|f| format!("Format: {}", f)),
                                                            release.country.as_ref().map(|c| format!("Country: {}", c)),
                                                            release.label.as_ref().map(|l| format!("Label: {}", l)),
                                                        ),
                                                        MatchSource::Discogs(_) => (None, None, None),
                                                    };
                                                    rsx! {
                                                        if let Some(ref fmt) = format_text {
                                                            p { "{fmt}" }
                                                        }
                                                        if let Some(ref country) = country_text {
                                                            p { "{country}" }
                                                        }
                                                        if let Some(ref label) = label_text {
                                                            p { "{label}" }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                                div { class: "flex justify-end gap-3",
                                    button {
                                        class: "px-6 py-2 bg-gray-600 text-white rounded hover:bg-gray-700",
                                        onclick: on_edit,
                                        "Edit"
                                    }
                                    button {
                                        class: "px-6 py-2 bg-green-600 text-white rounded hover:bg-green-700",
                                        onclick: on_confirm,
                                        "Import"
                                    }
                                }
                            }
                        }
                    }

                    // Error messages
                    if let Some(ref error) = import_error_message.read().as_ref() {
                        div { class: "bg-red-50 border border-red-200 rounded-lg p-4",
                            p { class: "text-sm text-red-700", "Error: {error}" }
                            {
                                let dup_id_opt = duplicate_album_id.read().clone();
                                if let Some(dup_id) = dup_id_opt {
                                    let dup_id_clone = dup_id.clone();
                                    rsx! {
                                        div { class: "mt-2",
                                            a {
                                                href: "#",
                                                class: "text-sm text-blue-600 hover:underline",
                                                onclick: move |_| {
                                                    navigator.push(Route::AlbumDetail {
                                                        album_id: dup_id_clone.clone(),
                                                        release_id: String::new(),
                                                    });
                                                },
                                                "View existing album"
                                            }
                                        }
                                    }
                                } else {
                                    rsx! { div {} }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
