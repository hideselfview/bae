// FFI bindings for custom libtorrent storage backend
//
// This module provides CXX bindings to extend libtorrent-rs with session_params
// support and custom storage integration.

#[cxx::bridge]
#[allow(clippy::module_inception)]
mod ffi {
    unsafe extern "C++" {
        include!("bae_storage.h");
        include!("bae_storage_helpers.h");
        include!("libtorrent/session.hpp");
        include!("libtorrent/session_params.hpp");

        // Opaque types
        type BaeStorageConstructor;
        type SessionParams;
        type Session;
        type AddTorrentParams;
        type TorrentHandle;

        /// Create a custom storage constructor that libtorrent can use
        ///
        /// The callbacks are Rust functions that will be called from C++:
        /// - read_cb: Called when libtorrent needs to read a piece
        ///   storage_index identifies which torrent's storage to use
        /// - write_cb: Called when libtorrent needs to write a piece
        /// - hash_cb: Called when libtorrent needs to verify a piece hash
        fn create_bae_storage_constructor(
            read_callback: fn(
                storage_index: i32,
                piece_index: i32,
                offset: i32,
                size: i32,
            ) -> Vec<u8>,
            write_callback: fn(
                storage_index: i32,
                piece_index: i32,
                offset: i32,
                data: &[u8],
            ) -> bool,
            hash_callback: fn(storage_index: i32, piece_index: i32, hash: &[u8]) -> bool,
        ) -> UniquePtr<BaeStorageConstructor>;

        /// Create session_params with custom disk I/O constructor
        fn create_session_params_with_storage(
            disk_io: UniquePtr<BaeStorageConstructor>,
        ) -> UniquePtr<SessionParams>;

        /// Create session_params with default disk storage (no custom storage)
        fn create_session_params_default() -> UniquePtr<SessionParams>;

        /// Set listen_interfaces on session_params
        ///
        /// # Safety
        /// `params` must be a valid pointer to SessionParams that outlives the call.
        unsafe fn set_listen_interfaces(params: *mut SessionParams, interfaces: &str);

        /// Create a session from session_params (extends libtorrent-rs)
        fn create_session_with_params(params: UniquePtr<SessionParams>) -> UniquePtr<Session>;

        /// Get raw session pointer from Session unique_ptr
        /// Returns a raw pointer that can be used with libtorrent-rs API
        ///
        /// # Safety
        /// The returned pointer is only valid while `sess` is alive and not moved.
        /// The caller must ensure the pointer is not used after `sess` is dropped.
        fn get_session_ptr(sess: &mut UniquePtr<Session>) -> *mut Session;

        /// Parse a magnet URI and return add_torrent_params
        fn parse_magnet_uri(magnet: &str, save_path: &str) -> UniquePtr<AddTorrentParams>;

        /// Load a torrent file and return add_torrent_params
        fn load_torrent_file(file_path: &str, save_path: &str) -> UniquePtr<AddTorrentParams>;

        /// Set seed_mode flag on add_torrent_params to skip hash verification
        ///
        /// # Safety
        /// `params` must be a valid pointer to AddTorrentParams that outlives the call.
        unsafe fn set_seed_mode(params: *mut AddTorrentParams, seed_mode: bool);

        /// Set paused flag on add_torrent_params to add torrent in paused state
        ///
        /// # Safety
        /// `params` must be a valid pointer to AddTorrentParams that outlives the call.
        unsafe fn set_paused(params: *mut AddTorrentParams, paused: bool);

        /// Add a torrent to a session using our Session type
        ///
        /// # Safety
        /// `sess` must be a valid pointer to a Session object that outlives the call.
        /// `params` must be a valid UniquePtr to AddTorrentParams.
        unsafe fn session_add_torrent(
            sess: *mut Session,
            params: &mut UniquePtr<AddTorrentParams>,
        ) -> *mut TorrentHandle;

        /// Get the name of a torrent from its handle
        ///
        /// # Safety
        /// `handle` must be a valid pointer to a TorrentHandle that outlives the call.
        unsafe fn torrent_get_name(handle: *mut TorrentHandle) -> String;

        /// Check if a torrent has metadata available
        ///
        /// # Safety
        /// `handle` must be a valid pointer to a TorrentHandle that outlives the call.
        unsafe fn torrent_has_metadata(handle: *mut TorrentHandle) -> bool;

        /// Get the storage index for a torrent handle
        /// Returns the storage_index_t assigned by libtorrent for this torrent
        ///
        /// # Safety
        /// `handle` must be a valid pointer to a TorrentHandle that outlives the call.
        unsafe fn torrent_get_storage_index(handle: *mut TorrentHandle) -> i32;

        /// Get torrent metadata for piece mapper
        ///
        /// # Safety
        /// `handle` must be a valid pointer to a TorrentHandle that outlives the call.
        unsafe fn torrent_get_piece_length(handle: *mut TorrentHandle) -> i32;
        /// Get the total size of the torrent
        ///
        /// # Safety
        /// `handle` must be a valid pointer to a TorrentHandle that outlives the call.
        unsafe fn torrent_get_total_size(handle: *mut TorrentHandle) -> i64;
        /// Get the number of pieces in the torrent
        ///
        /// # Safety
        /// `handle` must be a valid pointer to a TorrentHandle that outlives the call.
        unsafe fn torrent_get_num_pieces(handle: *mut TorrentHandle) -> i32;

        /// Get the list of files in the torrent
        ///
        /// # Safety
        /// `handle` must be a valid pointer to a TorrentHandle that outlives the call.
        unsafe fn torrent_get_file_list(handle: *mut TorrentHandle) -> Vec<TorrentFileInfo>;

        /// Set file priorities for a torrent
        ///
        /// # Safety
        /// `handle` must be a valid pointer to a TorrentHandle that outlives the call.
        unsafe fn torrent_set_file_priorities(
            handle: *mut TorrentHandle,
            priorities: Vec<u8>,
        ) -> bool;

        /// Get download progress (0.0 to 1.0) for a torrent
        ///
        /// # Safety
        /// `handle` must be a valid pointer to a TorrentHandle that outlives the call.
        unsafe fn torrent_get_progress(handle: *mut TorrentHandle) -> f32;

        /// Get number of connected peers
        ///
        /// # Safety
        /// `handle` must be a valid pointer to a TorrentHandle that outlives the call.
        unsafe fn torrent_get_num_peers(handle: *mut TorrentHandle) -> i32;

        /// Get number of seeders
        ///
        /// # Safety
        /// `handle` must be a valid pointer to a TorrentHandle that outlives the call.
        unsafe fn torrent_get_num_seeds(handle: *mut TorrentHandle) -> i32;

        /// Get tracker status as a formatted string
        ///
        /// # Safety
        /// `handle` must be a valid pointer to a TorrentHandle that outlives the call.
        unsafe fn torrent_get_tracker_status(handle: *mut TorrentHandle) -> String;

        /// Get the listen_interfaces setting from a session
        ///
        /// # Safety
        /// `sess` must be a valid pointer to a Session that outlives the call.
        unsafe fn session_get_listen_interfaces(sess: *mut Session) -> String;

        /// Get the listening port from a session
        ///
        /// # Safety
        /// `sess` must be a valid pointer to a Session that outlives the call.
        unsafe fn session_get_listening_port(sess: *mut Session) -> String;

        /// Pause a torrent
        ///
        /// # Safety
        /// `handle` must be a valid pointer to a TorrentHandle that outlives the call.
        unsafe fn torrent_pause(handle: *mut TorrentHandle);

        /// Resume a torrent
        ///
        /// # Safety
        /// `handle` must be a valid pointer to a TorrentHandle that outlives the call.
        unsafe fn torrent_resume(handle: *mut TorrentHandle);

        /// Remove a torrent from a session
        ///
        /// If `delete_files` is true, also deletes the downloaded files from disk.
        ///
        /// # Safety
        /// `sess` must be a valid pointer to a Session object that outlives the call.
        /// `handle` must be a valid pointer to a TorrentHandle that outlives the call.
        unsafe fn session_remove_torrent(
            sess: *mut Session,
            handle: *mut TorrentHandle,
            delete_files: bool,
        );
    }

    /// File info from torrent (shared between Rust and C++)
    struct TorrentFileInfo {
        index: i32,
        path: String,
        size: i64,
    }
}

pub use ffi::{
    create_bae_storage_constructor, create_session_params_default,
    create_session_params_with_storage, create_session_with_params, get_session_ptr,
    load_torrent_file, parse_magnet_uri, session_add_torrent, session_remove_torrent,
    set_listen_interfaces, set_paused, set_seed_mode, torrent_get_file_list, torrent_get_name,
    torrent_get_num_peers, torrent_get_num_pieces, torrent_get_num_seeds, torrent_get_piece_length,
    torrent_get_progress, torrent_get_storage_index, torrent_get_total_size,
    torrent_get_tracker_status, torrent_has_metadata, torrent_pause, torrent_resume,
    torrent_set_file_priorities, AddTorrentParams, BaeStorageConstructor, Session, SessionParams,
    TorrentFileInfo, TorrentHandle,
};
