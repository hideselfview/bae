use crate::torrent::client::TorrentError;
use crate::torrent::ffi::{get_torrent_info, TorrentFileInfo, TorrentInfo};
use std::path::Path;

/// Parse a torrent file and extract all available metadata
///
/// This is a static function that directly calls the FFI, bypassing TorrentManager.
/// Useful for quickly inspecting torrent files without adding them to a session.
pub fn parse_torrent_info(file_path: &Path) -> Result<TorrentInfo, TorrentError> {
    let path_str = file_path
        .to_str()
        .ok_or_else(|| TorrentError::InvalidTorrent("Invalid file path encoding".to_string()))?;

    let info = get_torrent_info(path_str);

    // Check if parsing failed (empty name indicates error)
    if info.name.is_empty() && info.total_size == 0 {
        return Err(TorrentError::InvalidTorrent(
            "Failed to parse torrent file".to_string(),
        ));
    }

    Ok(info)
}

/// Convert TorrentFileInfo to a more convenient format
pub fn file_info_to_path_size(file_info: &TorrentFileInfo) -> (String, i64) {
    (file_info.path.clone(), file_info.size)
}
