use crate::import_service::ImportProgress;
use std::sync::{mpsc::Receiver, Arc, Mutex};
use tokio::sync::broadcast;

/// Progress service that broadcasts import progress updates
#[derive(Clone)]
pub struct ProgressService {
    // Broadcast channel for progress updates
    progress_tx: Arc<broadcast::Sender<ImportProgress>>,
}

impl ProgressService {
    /// Create a new progress service and spawn background task to process progress updates
    pub fn new(progress_rx: Arc<Mutex<Receiver<ImportProgress>>>) -> Self {
        // Create broadcast channel with buffer of 100 messages
        let (progress_tx, _) = broadcast::channel(100);
        let progress_tx = Arc::new(progress_tx);

        let tx_clone = progress_tx.clone();

        // Spawn std::thread to block on channel recv
        std::thread::spawn(move || loop {
            let progress = {
                let rx = progress_rx.lock().unwrap();
                rx.recv()
            };

            match progress {
                Ok(progress) => {
                    // Broadcast to all subscribers
                    let _ = tx_clone.send(progress);
                }
                Err(_) => {
                    println!("ProgressService: Channel closed, exiting");
                    break;
                }
            }
        });

        Self { progress_tx }
    }

    /// Subscribe to album progress updates
    /// Returns a receiver that yields progress updates for the specified album
    /// Note: Currently returns all progress updates; filtering happens in component
    pub fn subscribe_album(&self, _album_id: String) -> broadcast::Receiver<ImportProgress> {
        self.progress_tx.subscribe()
    }

    /// Subscribe to all progress updates
    pub fn subscribe(&self) -> broadcast::Receiver<ImportProgress> {
        self.progress_tx.subscribe()
    }
}
