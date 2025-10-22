use super::types::ImportProgress;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc as tokio_mpsc;

/// Emitter for publishing import progress events during pipeline execution.
///
/// Encapsulates:
/// - Chunk→track mappings (which chunks belong to which tracks)
/// - Completion state (which chunks/tracks are done)
/// - Progress event transmission
///
/// A chunk can contain data from multiple tracks (when small files share a chunk).
/// Used by the import pipeline to emit progress events as chunks complete.
#[derive(Clone)]
pub struct ImportProgressEmitter {
    album_id: String,
    // Chunk→track mappings (a chunk can belong to multiple tracks)
    chunk_to_track: Arc<HashMap<i32, Vec<String>>>,
    track_chunk_counts: Arc<HashMap<String, usize>>,
    // Progress channel
    tx: tokio_mpsc::UnboundedSender<ImportProgress>,
    // Mutable completion state
    completed_chunks: Arc<Mutex<HashSet<i32>>>,
    completed_tracks: Arc<Mutex<HashSet<String>>>,
    total_chunks: usize,
}

impl ImportProgressEmitter {
    /// Create a new progress emitter for an album import.
    pub fn new(
        album_id: String,
        total_chunks: usize,
        chunk_to_track: HashMap<i32, Vec<String>>,
        track_chunk_counts: HashMap<String, usize>,
        tx: tokio_mpsc::UnboundedSender<ImportProgress>,
    ) -> Self {
        Self {
            album_id,
            chunk_to_track: Arc::new(chunk_to_track),
            track_chunk_counts: Arc::new(track_chunk_counts),
            tx,
            completed_chunks: Arc::new(Mutex::new(HashSet::new())),
            completed_tracks: Arc::new(Mutex::new(HashSet::new())),
            total_chunks,
        }
    }

    /// Mark a chunk as complete and return newly completed track IDs.
    ///
    /// Updates internal state, checks all tracks for completion, emits progress events.
    /// Returns track IDs that were just completed (not previously marked).
    pub fn on_chunk_complete(&self, chunk_index: i32) -> Vec<String> {
        let (newly_completed_tracks, progress_update) = {
            let mut completed = self.completed_chunks.lock().unwrap();
            let mut already_completed = self.completed_tracks.lock().unwrap();

            completed.insert(chunk_index);

            // Check all tracks for completion (not just the current chunk's track)
            let newly_completed =
                self.check_all_tracks_for_completion(&completed, &already_completed);

            // Mark these tracks as completed so we don't check them again
            for track_id in &newly_completed {
                already_completed.insert(track_id.clone());
            }

            let percent = calculate_progress(completed.len(), self.total_chunks);
            (newly_completed, (completed.len(), percent))
        };

        // Emit progress event
        let _ = self.tx.send(ImportProgress::ProcessingProgress {
            album_id: self.album_id.clone(),
            percent: progress_update.1,
            current: progress_update.0,
            total: self.total_chunks,
        });

        newly_completed_tracks
    }

    /// Emit a progress event directly.
    pub fn emit(&self, progress: ImportProgress) {
        let _ = self.tx.send(progress);
    }

    /// Check all tracks for completion and return newly completed ones.
    ///
    /// Called after each chunk upload to detect any tracks that have all their chunks done.
    /// Skips tracks that are already marked as complete.
    ///
    /// A track is complete when all chunks containing that track's data have been uploaded.
    /// Since chunks can contain multiple tracks, we check each track independently.
    fn check_all_tracks_for_completion(
        &self,
        completed_chunks: &HashSet<i32>,
        already_completed: &HashSet<String>,
    ) -> Vec<String> {
        let mut newly_completed = Vec::new();

        for (track_id, &total_for_track) in self.track_chunk_counts.iter() {
            // Skip if already marked complete
            if already_completed.contains(track_id) {
                continue;
            }

            // Count how many of this track's chunks are complete
            // A chunk belongs to this track if the track ID appears in the chunk's Vec<String>
            let completed_for_track = self
                .chunk_to_track
                .iter()
                .filter(|(idx, track_ids)| {
                    track_ids.contains(track_id) && completed_chunks.contains(idx)
                })
                .count();

            if completed_for_track == total_for_track {
                newly_completed.push(track_id.clone());
            }
        }

        newly_completed
    }
}

/// Calculate progress percentage
fn calculate_progress(completed: usize, total: usize) -> u8 {
    if total == 0 {
        100
    } else {
        ((completed as f64 / total as f64) * 100.0).min(100.0) as u8
    }
}
