use crate::import::types::ImportProgress;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc as tokio_mpsc;
use tracing::trace;

/// Tracks import progress and emits progress events during pipeline execution.
///
/// Encapsulates:
/// - Chunk→track mappings (which chunks belong to which tracks)
/// - Completion state (which chunks/tracks are done)
/// - Progress event transmission
///
/// A chunk can contain data from multiple tracks (when small files share a chunk).
/// Used by the import pipeline to track progress and emit events as chunks complete.
#[derive(Clone)]
pub struct ImportProgressTracker {
    release_id: String,
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

impl ImportProgressTracker {
    /// Create a new progress tracker for a release import.
    pub fn new(
        release_id: String,
        total_chunks: usize,
        chunk_to_track: HashMap<i32, Vec<String>>,
        track_chunk_counts: HashMap<String, usize>,
        tx: tokio_mpsc::UnboundedSender<ImportProgress>,
    ) -> Self {
        Self {
            release_id,
            chunk_to_track: Arc::new(chunk_to_track),
            track_chunk_counts: Arc::new(track_chunk_counts),
            tx,
            completed_chunks: Arc::new(Mutex::new(HashSet::new())),
            completed_tracks: Arc::new(Mutex::new(HashSet::new())),
            total_chunks,
        }
    }

    /// Mark a chunk as complete and emit progress events.
    ///
    /// Updates internal state, checks all tracks for completion, emits progress events.
    /// Returns newly completed track IDs for database persistence.
    pub fn on_chunk_complete(&self, chunk_index: i32) -> Vec<String> {
        let (newly_completed_tracks, progress_update, track_progress_updates) = {
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

            // Calculate progress for all tracks that are not yet complete
            let mut track_progress = Vec::new();
            for (track_id, &total_for_track) in self.track_chunk_counts.iter() {
                if !already_completed.contains(track_id) {
                    let completed_for_track = self
                        .chunk_to_track
                        .iter()
                        .filter(|(idx, track_ids)| {
                            track_ids.contains(track_id) && completed.contains(idx)
                        })
                        .count();

                    let percent = calculate_progress(completed_for_track, total_for_track);
                    track_progress.push((track_id.clone(), percent));
                }
            }

            let percent = calculate_progress(completed.len(), self.total_chunks);
            (newly_completed, (completed.len(), percent), track_progress)
        };

        // Emit progress event
        trace!(
            "Chunk {} complete ({}/{}), {}% done",
            chunk_index,
            progress_update.0,
            self.total_chunks,
            progress_update.1
        );

        // Emit release-level progress
        let _ = self.tx.send(ImportProgress::Progress {
            id: self.release_id.clone(),
            percent: progress_update.1,
        });

        // Emit track-level progress for each track that's still importing
        for (track_id, percent) in track_progress_updates {
            trace!("Track {} progress: {}%", track_id, percent);

            let _ = self.tx.send(ImportProgress::Progress {
                id: track_id.clone(),
                percent,
            });
        }

        // Emit Complete for each newly completed track
        for track_id in &newly_completed_tracks {
            trace!("Track {} complete", track_id);

            let _ = self.tx.send(ImportProgress::Complete {
                id: track_id.clone(),
            });
        }

        newly_completed_tracks
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_track_completion_simple_two_tracks() {
        // Replicate the vinyl test scenario: 2 tracks, each gets some chunks
        let (tx, mut rx) = tokio_mpsc::unbounded_channel();

        // Track 1 spans chunks 0-24 (25 chunks)
        // Track 2 spans chunks 25-48 (24 chunks)
        let mut chunk_to_track = HashMap::new();
        let track1_id = "track-1".to_string();
        let track2_id = "track-2".to_string();

        // Map chunks to tracks
        for i in 0..25 {
            chunk_to_track.insert(i, vec![track1_id.clone()]);
        }
        for i in 25..49 {
            chunk_to_track.insert(i, vec![track2_id.clone()]);
        }

        // Track chunk counts
        let mut track_chunk_counts = HashMap::new();
        track_chunk_counts.insert(track1_id.clone(), 25);
        track_chunk_counts.insert(track2_id.clone(), 24);

        let tracker = ImportProgressTracker::new(
            "test-album".to_string(),
            49,
            chunk_to_track,
            track_chunk_counts,
            tx,
        );

        // Complete all chunks
        let mut completed_tracks = Vec::new();
        for i in 0..49 {
            let newly_completed = tracker.on_chunk_complete(i);
            completed_tracks.extend(newly_completed);
        }

        // Assert: Both tracks should be marked complete
        assert_eq!(
            completed_tracks.len(),
            2,
            "Expected 2 tracks to complete, but got: {:?}",
            completed_tracks
        );
        assert!(
            completed_tracks.contains(&track1_id),
            "Track 1 should be complete"
        );
        assert!(
            completed_tracks.contains(&track2_id),
            "Track 2 should be complete"
        );

        // Verify progress events were sent (49 release Progress + 49*2 track Progress + 2 Complete)
        let mut release_progress_count = 0;
        let mut complete_count = 0;
        while let Ok(event) = rx.try_recv() {
            match event {
                ImportProgress::Progress { id, .. } => {
                    if id == "test-album" {
                        release_progress_count += 1;
                    }
                }
                ImportProgress::Complete { .. } => complete_count += 1,
                _ => {}
            }
        }
        assert_eq!(
            release_progress_count, 49,
            "Expected 49 release Progress events"
        );
        assert_eq!(complete_count, 2, "Expected 2 Complete events");
    }
}
