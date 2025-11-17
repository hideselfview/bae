// C++ wrapper functions for cxx bridge
// These functions convert between Rust types (rust::Fn, rust::Str, rust::String) and C++ types
// This file is compiled as part of the cxx bridge code generation

// Include cxx.h first - it's in the cxxbridge/include/rust directory
// The include path is set up automatically by cxx_build
#include "rust/cxx.h"  // cxx.h is in the rust subdirectory
#include "bae_storage_helpers.h"
#include "bae_storage.h"
#include <vector>
#include <string>

// Include the cxx bridge generated header for TorrentFileInfo
// The path is relative to cxxbridge/include which is in the include path
#include "bae/Users/dima/dev/bae/bae/src/torrent/ffi.rs.h"

// Wrapper for create_bae_storage_constructor
// Converts rust::Fn callbacks to std::function callbacks
std::unique_ptr<BaeStorageConstructor> create_bae_storage_constructor(
    rust::Fn<rust::Vec<uint8_t>(int32_t, int32_t, int32_t, int32_t)> read_cb,
    rust::Fn<bool(int32_t, int32_t, int32_t, rust::Slice<const uint8_t>)> write_cb,
    rust::Fn<bool(int32_t, int32_t, rust::Slice<const uint8_t>)> hash_cb
) {
    // Convert rust::Fn to std::function
    auto read_fn = [read_cb](int32_t storage_index, int32_t piece_index, int32_t offset, int32_t size) -> std::vector<uint8_t> {
        rust::Vec<uint8_t> result = read_cb(storage_index, piece_index, offset, size);
        return std::vector<uint8_t>(result.begin(), result.end());
    };
    
    auto write_fn = [write_cb](int32_t storage_index, int32_t piece_index, int32_t offset, const std::vector<uint8_t>& data) -> bool {
        rust::Slice<const uint8_t> slice(data.data(), data.size());
        return write_cb(storage_index, piece_index, offset, slice);
    };
    
    auto hash_fn = [hash_cb](int32_t storage_index, int32_t piece_index, const std::vector<uint8_t>& hash) -> bool {
        rust::Slice<const uint8_t> slice(hash.data(), hash.size());
        return hash_cb(storage_index, piece_index, slice);
    };
    
    auto ctor = libtorrent::create_bae_disk_io_constructor(std::move(read_fn), std::move(write_fn), std::move(hash_fn));
    return std::make_unique<BaeStorageConstructor>(std::move(ctor));
}

// Wrapper for create_session_params_with_storage - converts UniquePtr<BaeStorageConstructor> to std::function
std::unique_ptr<SessionParams> create_session_params_with_storage(std::unique_ptr<BaeStorageConstructor> disk_io) {
    // Extract the std::function from the UniquePtr and pass to libtorrent version
    return libtorrent::create_session_params_with_storage(std::move(*disk_io));
}

// Wrapper for create_session_params_default
std::unique_ptr<SessionParams> create_session_params_default() {
    return libtorrent::create_session_params_default();
}

// Wrapper for parse_magnet_uri - converts rust::Str to std::string
std::unique_ptr<AddTorrentParams> parse_magnet_uri(rust::Str magnet, rust::Str save_path) {
    return libtorrent::parse_magnet_uri(std::string(magnet), std::string(save_path));
}

// Wrapper for load_torrent_file - converts rust::Str to std::string
std::unique_ptr<AddTorrentParams> load_torrent_file(rust::Str file_path, rust::Str save_path) {
    return libtorrent::load_torrent_file(std::string(file_path), std::string(save_path));
}

// Wrapper for set_seed_mode
void set_seed_mode(AddTorrentParams* params, bool seed_mode) {
    libtorrent::set_seed_mode(params, seed_mode);
}

// Wrapper for set_paused
void set_paused(AddTorrentParams* params, bool paused) {
    libtorrent::set_paused(params, paused);
}

// Wrapper for set_listen_interfaces - converts rust::Str to std::string
void set_listen_interfaces(SessionParams* params, rust::Str interfaces) {
    libtorrent::set_listen_interfaces(params, std::string(interfaces));
}

// Wrapper for torrent_get_name - converts std::string to rust::String
rust::String torrent_get_name(TorrentHandle* handle) {
    std::string name = libtorrent::torrent_get_name_internal(handle);
    return rust::String(name.data(), name.size());
}

// Wrappers for functions that don't need type conversion but must be in global namespace for cxx
std::unique_ptr<Session> create_session_with_params(std::unique_ptr<SessionParams> params) {
    return libtorrent::create_session_with_params(std::move(params));
}

Session* get_session_ptr(std::unique_ptr<Session>& sess) {
    return libtorrent::get_session_ptr(sess);
}

TorrentHandle* session_add_torrent(Session* sess, std::unique_ptr<AddTorrentParams>& params) {
    return libtorrent::session_add_torrent(sess, params);
}

bool torrent_has_metadata(TorrentHandle* handle) {
    return libtorrent::torrent_has_metadata(handle);
}

int32_t torrent_get_storage_index(TorrentHandle* handle) {
    return libtorrent::torrent_get_storage_index(handle);
}

int32_t torrent_get_piece_length(TorrentHandle* handle) {
    return libtorrent::torrent_get_piece_length(handle);
}

int64_t torrent_get_total_size(TorrentHandle* handle) {
    return libtorrent::torrent_get_total_size(handle);
}

int32_t torrent_get_num_pieces(TorrentHandle* handle) {
    return libtorrent::torrent_get_num_pieces(handle);
}

bool torrent_have_piece(TorrentHandle* handle, int32_t piece_index) {
    return libtorrent::torrent_have_piece(handle, piece_index);
}

// Note: TorrentFileInfo is defined by cxx bridge in the generated header
// The function signature matches what's declared in the Rust FFI bridge
rust::Vec<TorrentFileInfo> torrent_get_file_list(TorrentHandle* handle) {
    auto files = libtorrent::torrent_get_file_list_internal(handle);
    rust::Vec<TorrentFileInfo> result;
    for (const auto& file : files) {
        // TorrentFileInfo is generated by cxx bridge from the Rust struct definition
        TorrentFileInfo info;
        info.index = file.index;
        info.path = rust::String(file.path.data(), file.path.size());
        info.size = file.size;
        result.push_back(info);
    }
    return result;
}

bool torrent_set_file_priorities(TorrentHandle* handle, rust::Vec<uint8_t> priorities) {
    std::vector<uint8_t> cpp_priorities(priorities.begin(), priorities.end());
    return libtorrent::torrent_set_file_priorities_internal(handle, cpp_priorities);
}

float torrent_get_progress(TorrentHandle* handle) {
    return libtorrent::torrent_get_progress_internal(handle);
}

int32_t torrent_get_num_peers(TorrentHandle* handle) {
    return libtorrent::torrent_get_num_peers(handle);
}

int32_t torrent_get_num_seeds(TorrentHandle* handle) {
    return libtorrent::torrent_get_num_seeds(handle);
}

rust::String torrent_get_tracker_status(TorrentHandle* handle) {
    std::string status = libtorrent::torrent_get_tracker_status(handle);
    return rust::String(status.data(), status.size());
}

rust::String session_get_listen_interfaces(Session* sess) {
    std::string interfaces = libtorrent::session_get_listen_interfaces(sess);
    return rust::String(interfaces.data(), interfaces.size());
}

rust::String session_get_listening_port(Session* sess) {
    std::string port = libtorrent::session_get_listening_port(sess);
    return rust::String(port.data(), port.size());
}

// Wrapper for get_torrent_info - converts C++ LibTorrentInfo to Rust TorrentInfo
TorrentInfo get_torrent_info(rust::Str file_path) {
    auto cpp_info = libtorrent::get_torrent_info_internal(std::string(file_path));
    if (!cpp_info) {
        // Return empty struct on error - Rust will handle the error
        TorrentInfo info;
        info.name = rust::String("");
        info.trackers = rust::Vec<rust::String>();
        info.comment = rust::String("");
        info.creator = rust::String("");
        info.creation_date = 0;
        info.is_private = false;
        info.total_size = 0;
        info.piece_length = 0;
        info.num_pieces = 0;
        info.files = rust::Vec<TorrentFileInfo>();
        return info;
    }
    
    TorrentInfo info;
    info.name = rust::String(cpp_info->name.data(), cpp_info->name.size());
    
    rust::Vec<rust::String> trackers;
    for (const auto& tracker : cpp_info->trackers) {
        trackers.push_back(rust::String(tracker.data(), tracker.size()));
    }
    info.trackers = trackers;
    
    info.comment = rust::String(cpp_info->comment.data(), cpp_info->comment.size());
    info.creator = rust::String(cpp_info->creator.data(), cpp_info->creator.size());
    info.creation_date = cpp_info->creation_date;
    info.is_private = cpp_info->is_private;
    info.total_size = cpp_info->total_size;
    info.piece_length = cpp_info->piece_length;
    info.num_pieces = cpp_info->num_pieces;
    
    rust::Vec<TorrentFileInfo> files;
    for (const auto& file : cpp_info->files) {
        TorrentFileInfo file_info;
        file_info.index = file.index;
        file_info.path = rust::String(file.path.data(), file.path.size());
        file_info.size = file.size;
        files.push_back(file_info);
    }
    info.files = files;
    
    return info;
}

void torrent_pause(TorrentHandle* handle) {
    libtorrent::torrent_pause(handle);
}

void torrent_resume(TorrentHandle* handle) {
    libtorrent::torrent_resume(handle);
}

void session_remove_torrent(Session* sess, TorrentHandle* handle, bool delete_files) {
    libtorrent::session_remove_torrent(sess, handle, delete_files);
}

// Wrapper for session_pop_alerts - converts C++ AlertData to Rust AlertData
rust::Vec<AlertData> session_pop_alerts(Session* sess) {
    auto cpp_alerts = libtorrent::session_pop_alerts(sess);
    rust::Vec<AlertData> rust_alerts;
    for (const auto& cpp_alert : cpp_alerts) {
        AlertData rust_alert;
        rust_alert.alert_type = static_cast<int32_t>(cpp_alert.type);
        rust_alert.info_hash = rust::String(cpp_alert.info_hash.data(), cpp_alert.info_hash.size());
        rust_alert.tracker_url = rust::String(cpp_alert.tracker_url.data(), cpp_alert.tracker_url.size());
        rust_alert.tracker_message = rust::String(cpp_alert.tracker_message.data(), cpp_alert.tracker_message.size());
        rust_alert.num_peers = cpp_alert.num_peers;
        rust_alert.num_seeds = cpp_alert.num_seeds;
        rust_alert.file_path = rust::String(cpp_alert.file_path.data(), cpp_alert.file_path.size());
        rust_alert.progress = cpp_alert.progress;
        rust_alert.error_message = rust::String(cpp_alert.error_message.data(), cpp_alert.error_message.size());
        rust_alerts.push_back(rust_alert);
    }
    return rust_alerts;
}

