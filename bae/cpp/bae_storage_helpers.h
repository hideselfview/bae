#ifndef BAE_STORAGE_HELPERS_H
#define BAE_STORAGE_HELPERS_H

#include <libtorrent/session.hpp>
#include <libtorrent/session_params.hpp>
#include <libtorrent/add_torrent_params.hpp>
#include <libtorrent/torrent_handle.hpp>
#include <libtorrent/io_context.hpp>
#include <libtorrent/disk_interface.hpp>
#include <functional>
#include <memory>
#include <string>

namespace libtorrent {

// Forward declarations
class disk_interface;
struct settings_interface;
struct counters;

/// Create session_params with custom disk I/O constructor
///
/// This is a helper function to create session_params and set a custom
/// disk_io_constructor, which libtorrent-rs doesn't expose.
/// disk_io_constructor is a std::function that creates a disk_interface.
std::unique_ptr<session_params> create_session_params_with_storage(
    std::function<std::unique_ptr<disk_interface>(io_context&, settings_interface const&, counters&)> disk_io_ctor
);

/// Create session_params with default disk storage (no custom storage)
///
/// This creates a session that uses libtorrent's default disk storage,
/// which writes files directly to disk.
std::unique_ptr<session_params> create_session_params_default();

/// Create a session from session_params
///
/// libtorrent-rs only exposes lt_create_session() which doesn't accept params.
/// This function allows us to create a session with custom storage or default storage.
std::unique_ptr<session> create_session_with_params(
    std::unique_ptr<session_params> params
);

/// Get raw session pointer from Session unique_ptr
///
/// This allows us to extract the raw pointer to use with libtorrent-rs API
/// which expects raw pointers.
session* get_session_ptr(std::unique_ptr<session>& sess);

/// Wrapper functions for libtorrent operations using our Session type
///
/// These functions allow us to use our custom Session with libtorrent operations
/// that libtorrent-rs doesn't expose for custom sessions.

/// Parse a magnet URI and return add_torrent_params
std::unique_ptr<add_torrent_params> parse_magnet_uri(const std::string& magnet, const std::string& save_path);

/// Load a torrent file and return add_torrent_params
std::unique_ptr<add_torrent_params> load_torrent_file(const std::string& file_path, const std::string& save_path);

/// Add a torrent to a session using our Session type
torrent_handle* session_add_torrent(session* sess, std::unique_ptr<add_torrent_params>& params);

/// Remove a torrent from a session
/// If delete_files is true, also deletes the downloaded files from disk
void session_remove_torrent(session* sess, torrent_handle* handle, bool delete_files);

/// Get the name of a torrent from its handle (internal version)
/// Note: This is wrapped for cxx bridge - the global namespace version returns rust::String
std::string torrent_get_name_internal(torrent_handle* handle);

/// Check if a torrent has metadata available
bool torrent_has_metadata(torrent_handle* handle);

/// Get the storage index for a torrent handle
/// Returns the storage_index_t assigned by libtorrent for this torrent
int32_t torrent_get_storage_index(torrent_handle* handle);

/// Get torrent metadata for piece mapper
int32_t torrent_get_piece_length(torrent_handle* handle);
int64_t torrent_get_total_size(torrent_handle* handle);
int32_t torrent_get_num_pieces(torrent_handle* handle);

/// Check if a piece is available (downloaded and verified)
bool torrent_have_piece(torrent_handle* handle, int32_t piece_index);

/// Internal C++ struct for file info (not exposed to Rust)
struct LibTorrentFileInfo {
    int32_t index;
    std::string path;
    int64_t size;
};

/// Get the list of files in the torrent (internal C++ function)
std::vector<LibTorrentFileInfo> torrent_get_file_list_internal(torrent_handle* handle);

/// Set file priorities for a torrent (internal C++ function)
bool torrent_set_file_priorities_internal(torrent_handle* handle, const std::vector<uint8_t>& priorities);

/// Get download progress (0.0 to 1.0) for a torrent (internal C++ function)
float torrent_get_progress_internal(torrent_handle* handle);

/// Get number of connected peers
int32_t torrent_get_num_peers(torrent_handle* handle);

/// Get number of seeders
int32_t torrent_get_num_seeds(torrent_handle* handle);

/// Get tracker status as a formatted string
std::string torrent_get_tracker_status(torrent_handle* handle);

/// Set seed_mode flag on add_torrent_params to skip hash verification
void set_seed_mode(add_torrent_params* params, bool seed_mode);

/// Set paused flag on add_torrent_params to add torrent in paused state
void set_paused(add_torrent_params* params, bool paused);

/// Set listen_interfaces on session_params
/// interfaces can be an interface name (e.g. "eth0", "tun0") or IP:port (e.g. "0.0.0.0:6881")
void set_listen_interfaces(session_params* params, const std::string& interfaces);

/// Get the actual listen_interfaces setting from a session
std::string session_get_listen_interfaces(session* sess);

/// Get the listening port from a session
std::string session_get_listening_port(session* sess);

/// Pause a torrent
void torrent_pause(torrent_handle* handle);

/// Resume a torrent
void torrent_resume(torrent_handle* handle);

/// Alert handling functions for libtorrent's alert system
/// Alert types (matching libtorrent alert_category_t)
enum AlertType {
    ALERT_TRACKER_ANNOUNCE = 0,
    ALERT_TRACKER_ERROR = 1,
    ALERT_PEER_CONNECT = 2,
    ALERT_PEER_DISCONNECT = 3,
    ALERT_FILE_COMPLETED = 4,
    ALERT_METADATA_RECEIVED = 5,
    ALERT_TORRENT_ADDED = 6,
    ALERT_TORRENT_REMOVED = 7,
    ALERT_TORRENT_PAUSED = 8,
    ALERT_TORRENT_RESUMED = 9,
    ALERT_STATE_CHANGED = 10,
    ALERT_STATS = 11,
    ALERT_UNKNOWN = 99,
};

/// Alert data structure for passing to Rust
struct AlertData {
    AlertType type;
    std::string info_hash;
    std::string tracker_url;
    std::string tracker_message;
    int32_t num_peers;
    int32_t num_seeds;
    std::string file_path;
    float progress;
    std::string error_message;
};

/// Pop all pending alerts from session
std::vector<AlertData> session_pop_alerts(session* sess);

} // namespace libtorrent

// Type aliases for cxx bridge (must be in global namespace)
using BaeStorageConstructor = std::function<std::unique_ptr<libtorrent::disk_interface>(libtorrent::io_context&, libtorrent::settings_interface const&, libtorrent::counters&)>;
using SessionParams = libtorrent::session_params;
using Session = libtorrent::session;
using AddTorrentParams = libtorrent::add_torrent_params;
using TorrentHandle = libtorrent::torrent_handle;

// Forward declarations for global namespace wrappers
// cxx expects functions in global namespace, all are implemented in bae_storage_cxx_wrappers.cpp
std::unique_ptr<Session> create_session_with_params(std::unique_ptr<SessionParams> params);
Session* get_session_ptr(std::unique_ptr<Session>& sess);
TorrentHandle* session_add_torrent(Session* sess, std::unique_ptr<AddTorrentParams>& params);
void session_remove_torrent(Session* sess, TorrentHandle* handle, bool delete_files);
bool torrent_has_metadata(TorrentHandle* handle);
int32_t torrent_get_storage_index(TorrentHandle* handle);
int32_t torrent_get_piece_length(TorrentHandle* handle);
int64_t torrent_get_total_size(TorrentHandle* handle);
int32_t torrent_get_num_pieces(TorrentHandle* handle);
bool torrent_have_piece(TorrentHandle* handle, int32_t piece_index);

// Functions wrapped for cxx bridge (implemented in bae_storage_cxx_wrappers.cpp)
// These use cxx Rust types and convert to C++ types
#include "rust/cxx.h"

// Forward declaration - TorrentFileInfo is generated by cxx bridge
struct TorrentFileInfo;
rust::Vec<TorrentFileInfo> torrent_get_file_list(TorrentHandle* handle);
bool torrent_set_file_priorities(TorrentHandle* handle, rust::Vec<uint8_t> priorities);
float torrent_get_progress(TorrentHandle* handle);
int32_t torrent_get_num_peers(TorrentHandle* handle);
int32_t torrent_get_num_seeds(TorrentHandle* handle);
rust::String torrent_get_tracker_status(TorrentHandle* handle);
rust::String session_get_listen_interfaces(Session* sess);
rust::String session_get_listening_port(Session* sess);
void set_paused(AddTorrentParams* params, bool paused);
void torrent_pause(TorrentHandle* handle);
void torrent_resume(TorrentHandle* handle);

// Alert handling (implemented in bae_storage_helpers.cpp)
struct AlertData;
rust::Vec<AlertData> session_pop_alerts(Session* sess);

std::unique_ptr<BaeStorageConstructor> create_bae_storage_constructor(
    rust::Fn<rust::Vec<uint8_t>(int32_t, int32_t, int32_t, int32_t)> read_cb,
    rust::Fn<bool(int32_t, int32_t, int32_t, rust::Slice<const uint8_t>)> write_cb,
    rust::Fn<bool(int32_t, int32_t, rust::Slice<const uint8_t>)> hash_cb
);
std::unique_ptr<SessionParams> create_session_params_with_storage(std::unique_ptr<BaeStorageConstructor> disk_io);
std::unique_ptr<SessionParams> create_session_params_default();
std::unique_ptr<AddTorrentParams> parse_magnet_uri(rust::Str magnet, rust::Str save_path);
std::unique_ptr<AddTorrentParams> load_torrent_file(rust::Str file_path, rust::Str save_path);
void set_seed_mode(AddTorrentParams* params, bool seed_mode);
void set_listen_interfaces(SessionParams* params, rust::Str interfaces);
rust::String torrent_get_name(TorrentHandle* handle);

#endif // BAE_STORAGE_HELPERS_H

