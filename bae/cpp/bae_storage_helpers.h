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

/// Create a session from session_params
///
/// libtorrent-rs only exposes lt_create_session() which doesn't accept params.
/// This function allows us to create a session with custom storage.
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

/// Add a torrent to a session using our Session type
torrent_handle* session_add_torrent(session* sess, std::unique_ptr<add_torrent_params>& params);

/// Get the name of a torrent from its handle
std::string torrent_get_name(torrent_handle* handle);

/// Check if a torrent has metadata available
bool torrent_has_metadata(torrent_handle* handle);

/// Get the storage index for a torrent handle
/// Returns the storage_index_t assigned by libtorrent for this torrent
int32_t torrent_get_storage_index(torrent_handle* handle);

/// Get torrent metadata for piece mapper
int32_t torrent_get_piece_length(torrent_handle* handle);
int64_t torrent_get_total_size(torrent_handle* handle);
int32_t torrent_get_num_pieces(torrent_handle* handle);

} // namespace libtorrent

#endif // BAE_STORAGE_HELPERS_H

