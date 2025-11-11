// FFI bindings for custom libtorrent storage backend
//
// This module provides CXX bindings to extend libtorrent-rs with session_params
// support and custom storage integration.

#[cxx::bridge]
mod ffi {
    unsafe extern "C++" {
        include!("cpp/bae_storage.h");
        include!("cpp/bae_storage_helpers.h");
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

        /// Create a session from session_params (extends libtorrent-rs)
        fn create_session_with_params(params: UniquePtr<SessionParams>) -> UniquePtr<Session>;

        /// Get raw session pointer from Session unique_ptr
        /// Returns a raw pointer that can be used with libtorrent-rs API
        fn get_session_ptr(sess: &mut UniquePtr<Session>) -> *mut Session;

        /// Parse a magnet URI and return add_torrent_params
        fn parse_magnet_uri(magnet: &str, save_path: &str) -> UniquePtr<AddTorrentParams>;

        /// Add a torrent to a session using our Session type
        unsafe fn session_add_torrent(
            sess: *mut Session,
            params: &mut UniquePtr<AddTorrentParams>,
        ) -> *mut TorrentHandle;

        /// Get the name of a torrent from its handle
        unsafe fn torrent_get_name(handle: *mut TorrentHandle) -> String;

        /// Check if a torrent has metadata available
        unsafe fn torrent_has_metadata(handle: *mut TorrentHandle) -> bool;

        /// Get the storage index for a torrent handle
        /// Returns the storage_index_t assigned by libtorrent for this torrent
        unsafe fn torrent_get_storage_index(handle: *mut TorrentHandle) -> i32;

        /// Get torrent metadata for piece mapper
        unsafe fn torrent_get_piece_length(handle: *mut TorrentHandle) -> i32;
        unsafe fn torrent_get_total_size(handle: *mut TorrentHandle) -> i64;
        unsafe fn torrent_get_num_pieces(handle: *mut TorrentHandle) -> i32;
    }
}

pub use ffi::{
    create_bae_storage_constructor, create_session_params_with_storage, create_session_with_params,
    get_session_ptr, parse_magnet_uri, session_add_torrent, torrent_get_name,
    torrent_get_num_pieces, torrent_get_piece_length, torrent_get_storage_index,
    torrent_get_total_size, torrent_has_metadata, AddTorrentParams, BaeStorageConstructor, Session,
    SessionParams, TorrentHandle,
};
