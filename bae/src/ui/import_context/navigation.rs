use super::state::ImportContext;
use super::types::ImportPhase;
use crate::import::{MatchCandidate, MatchSource};
use crate::musicbrainz;
use crate::ui::components::import::{ImportSource, TorrentInputMode};
use dioxus::prelude::*;
use std::rc::Rc;
use tracing::warn;

/// Check if there is unclean state for the current import source
/// Returns true if switching tabs would lose progress
fn has_unclean_state(ctx: &ImportContext) -> bool {
    let current_source = *ctx.selected_import_source().read();
    match current_source {
        ImportSource::Folder => {
            // Folder tab has unclean state if a folder is selected
            !ctx.folder_path().read().is_empty()
        }
        ImportSource::Torrent => {
            // Torrent tab has unclean state if a torrent is selected or magnet link is entered
            ctx.torrent_source().read().is_some() || !ctx.magnet_link().read().is_empty()
        }
        ImportSource::Cd => {
            // CD tab has unclean state if a drive is selected or TOC is loaded
            !ctx.folder_path().read().is_empty() || ctx.cd_toc_info().read().is_some()
        }
    }
}

/// Try to switch import source, showing dialog if there's unclean state
pub fn try_switch_import_source(ctx: &Rc<ImportContext>, source: ImportSource) {
    // Don't show confirmation if switching to the same tab
    if *ctx.selected_import_source().read() == source {
        return;
    }

    // Check if there's unclean state
    if has_unclean_state(ctx) {
        let ctx_for_callback = Rc::clone(ctx);
        ctx.dialog.show_with_callback(
            "Watch out!".to_string(),
            "You have unsaved work. Navigating away will discard your current progress."
                .to_string(),
            "Switch Tab".to_string(),
            "Cancel".to_string(),
            move || {
                ctx_for_callback.set_selected_import_source(source);
                ctx_for_callback.reset();
            },
        );
    } else {
        // No unclean state, proceed with switch
        ctx.set_selected_import_source(source);
        ctx.reset();
    }
}

/// Try to switch torrent input mode, showing dialog if magnet link is not empty
pub fn try_switch_torrent_input_mode(ctx: &Rc<ImportContext>, mode: TorrentInputMode) {
    let current_mode = *ctx.torrent_input_mode().read();

    // Check if switching from Magnet mode and magnet link is not empty
    if current_mode == TorrentInputMode::Magnet && !ctx.magnet_link().read().is_empty() {
        let ctx_for_callback = Rc::clone(ctx);
        ctx.dialog.show_with_callback(
            "Watch out!".to_string(),
            "If you switch to Torrent File mode, you will lose the magnet link you entered."
                .to_string(),
            "Switch Mode".to_string(),
            "Cancel".to_string(),
            move || {
                ctx_for_callback.set_torrent_input_mode(mode);
                ctx_for_callback.set_magnet_link(String::new());
            },
        );
    } else {
        // No magnet link text, proceed with switch and clear it
        ctx.set_torrent_input_mode(mode);
        ctx.set_magnet_link(String::new());
    }
}

/// Select an exact match candidate by index and move to confirmation.
///
/// This transitions from ExactLookup phase to Confirmation phase.
pub fn select_exact_match(ctx: &ImportContext, index: usize) {
    ctx.set_selected_match_index(Some(index));
    if let Some(candidate) = ctx.exact_match_candidates().read().get(index).cloned() {
        ctx.set_confirmed_candidate(Some(candidate.clone()));
        ctx.set_import_phase(ImportPhase::Confirmation);

        // Fetch original album year for MusicBrainz releases
        if let MatchSource::MusicBrainz(ref release) = candidate.source {
            let release_group_id = release.release_group_id.clone();
            let mut original_album_year = ctx.original_album_year;
            spawn(async move {
                match musicbrainz::fetch_release_group_first_date(&release_group_id).await {
                    Ok(first_date) => {
                        original_album_year.set(first_date);
                    }
                    Err(e) => {
                        warn!("Failed to fetch original album year: {}", e);
                    }
                }
            });
        }
    }
}

/// Confirm a match candidate and move to confirmation phase.
///
/// This is used when confirming from manual search results.
pub fn confirm_candidate(ctx: &ImportContext, candidate: MatchCandidate) {
    ctx.set_confirmed_candidate(Some(candidate.clone()));
    ctx.set_import_phase(ImportPhase::Confirmation);

    // Fetch original album year for MusicBrainz releases
    if let MatchSource::MusicBrainz(ref release) = candidate.source {
        let release_group_id = release.release_group_id.clone();
        let mut original_album_year = ctx.original_album_year;
        spawn(async move {
            match musicbrainz::fetch_release_group_first_date(&release_group_id).await {
                Ok(first_date) => {
                    original_album_year.set(first_date);
                }
                Err(e) => {
                    warn!("Failed to fetch original album year: {}", e);
                }
            }
        });
    }
}

/// Reject the current confirmation and go back to previous phase.
///
/// This handles:
/// - Clearing confirmed candidate and selection
/// - Determining whether to go back to ExactLookup or ManualSearch
/// - Initializing search query from detected metadata if going to ManualSearch
pub fn reject_confirmation(ctx: &ImportContext) {
    ctx.set_confirmed_candidate(None);
    ctx.set_selected_match_index(None);

    if !ctx.exact_match_candidates().read().is_empty() {
        ctx.set_import_phase(ImportPhase::ExactLookup);
    } else {
        // Initialize search query from detected metadata when transitioning to manual search
        if let Some(metadata) = ctx.detected_metadata().read().as_ref() {
            ctx.init_search_query_from_metadata(metadata);
        }
        ctx.set_import_phase(ImportPhase::ManualSearch);
    }
}
