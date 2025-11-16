pub mod client;
pub mod ffi;
pub mod manager;
pub mod metadata_detector;
pub mod piece_mapper;
pub mod selective_downloader;
pub mod storage;

pub use client::{TorrentClient, TorrentHandle};
pub use manager::{start_torrent_manager, TorrentManagerHandle};
pub use metadata_detector::detect_metadata_from_torrent_file;
pub use piece_mapper::TorrentPieceMapper;
pub use selective_downloader::SelectiveDownloader;
pub use storage::BaeStorage;
