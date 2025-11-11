#include "bae_storage_helpers.h"
#include "bae_storage.h"
#include <libtorrent/session.hpp>
#include <libtorrent/session_params.hpp>
#include <libtorrent/add_torrent_params.hpp>
#include <libtorrent/torrent_handle.hpp>
#include <libtorrent/torrent_status.hpp>
#include <libtorrent/torrent_info.hpp>
#include <libtorrent/disk_interface.hpp>
#include <libtorrent/io_context.hpp>
#include <libtorrent/magnet_uri.hpp>
#include <libtorrent/error_code.hpp>

namespace libtorrent {

std::unique_ptr<session_params> create_session_params_with_storage(
    std::function<std::unique_ptr<disk_interface>(io_context&, settings_interface const&, counters&)> disk_io_ctor
) {
    auto params = std::make_unique<session_params>();
    params->disk_io_constructor = std::move(disk_io_ctor);
    return params;
}

std::unique_ptr<session> create_session_with_params(
    std::unique_ptr<session_params> params
) {
    return std::make_unique<session>(std::move(*params));
}

session* get_session_ptr(std::unique_ptr<session>& sess) {
    return sess.get();
}

std::unique_ptr<add_torrent_params> parse_magnet_uri(const std::string& magnet, const std::string& save_path) {
    error_code ec;
    auto params = std::make_unique<add_torrent_params>(libtorrent::parse_magnet_uri(magnet, ec));
    if (ec) {
        return nullptr;
    }
    params->save_path = save_path;
    return params;
}

torrent_handle* session_add_torrent(session* sess, std::unique_ptr<add_torrent_params>& params) {
    if (!sess || !params) {
        return nullptr;
    }
    error_code ec;
    torrent_handle handle = sess->add_torrent(std::move(*params), ec);
    if (ec) {
        return nullptr;
    }
    // Return a pointer to a heap-allocated handle
    // Note: The caller is responsible for managing this memory
    // In practice, libtorrent-rs manages handles internally
    return new torrent_handle(std::move(handle));
}

std::string torrent_get_name(torrent_handle* handle) {
    if (!handle) {
        return "";
    }
    torrent_status status = handle->status();
    return status.name;
}

bool torrent_has_metadata(torrent_handle* handle) {
    if (!handle) {
        return false;
    }
    torrent_status status = handle->status();
    return status.has_metadata;
}

int32_t torrent_get_storage_index(torrent_handle* handle) {
    if (!handle) {
        return -1;
    }
    torrent_status status = handle->status();
    // storage_index is available in torrent_status
    return static_cast<int32_t>(status.storage_index);
}

int32_t torrent_get_piece_length(torrent_handle* handle) {
    if (!handle) {
        return 0;
    }
    torrent_status status = handle->status();
    if (!status.torrent_file) {
        return 0;
    }
    auto const& info = *status.torrent_file;
    return static_cast<int32_t>(info.piece_length());
}

int64_t torrent_get_total_size(torrent_handle* handle) {
    if (!handle) {
        return 0;
    }
    torrent_status status = handle->status();
    return status.total_wanted;
}

int32_t torrent_get_num_pieces(torrent_handle* handle) {
    if (!handle) {
        return 0;
    }
    torrent_status status = handle->status();
    if (!status.torrent_file) {
        return 0;
    }
    auto const& info = *status.torrent_file;
    return static_cast<int32_t>(info.num_pieces());
}

} // namespace libtorrent

