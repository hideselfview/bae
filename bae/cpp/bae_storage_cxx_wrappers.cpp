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

