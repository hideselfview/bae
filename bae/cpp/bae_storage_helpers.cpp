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

std::unique_ptr<session_params> create_session_params_default() {
    // Create session_params without setting disk_io_constructor
    // This uses libtorrent's default disk storage
    return std::make_unique<session_params>();
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

std::unique_ptr<add_torrent_params> load_torrent_file(const std::string& file_path, const std::string& save_path) {
    error_code ec;
    torrent_info ti(file_path, ec);
    if (ec) {
        return nullptr;
    }
    auto params = std::make_unique<add_torrent_params>();
    params->ti = std::make_shared<torrent_info>(std::move(ti));
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

void session_remove_torrent(session* sess, torrent_handle* handle, bool delete_files) {
    if (!sess || !handle) {
        return;
    }
    // Remove torrent from session
    // If delete_files is true, also delete the downloaded files from disk
    if (delete_files) {
        sess->remove_torrent(*handle, session::delete_files);
    } else {
        sess->remove_torrent(*handle);
    }
}

std::string torrent_get_name_internal(torrent_handle* handle) {
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
    // storage_index is not directly available in torrent_status
    // We need to get it from the session's internal state
    // For now, return -1 and let Rust track it via storage_index_map
    // TODO: Find proper way to get storage_index from torrent_handle
    return -1;
}

int32_t torrent_get_piece_length(torrent_handle* handle) {
    if (!handle) {
        return 0;
    }
    torrent_status status = handle->status();
    auto torrent_file = status.torrent_file.lock();
    if (!torrent_file) {
        return 0;
    }
    return static_cast<int32_t>(torrent_file->piece_length());
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
    auto torrent_file = status.torrent_file.lock();
    if (!torrent_file) {
        return 0;
    }
    return static_cast<int32_t>(torrent_file->num_pieces());
}

bool torrent_have_piece(torrent_handle* handle, int32_t piece_index) {
    if (!handle) {
        return false;
    }
    return handle->have_piece(piece_index_t(piece_index));
}

std::vector<LibTorrentFileInfo> torrent_get_file_list_internal(torrent_handle* handle) {
    std::vector<LibTorrentFileInfo> files;
    if (!handle) {
        return files;
    }
    torrent_status status = handle->status();
    auto torrent_file = status.torrent_file.lock();
    if (!torrent_file) {
        return files;
    }
    auto file_storage = torrent_file->files();
    for (int i = 0; i < file_storage.num_files(); ++i) {
        LibTorrentFileInfo info;
        info.index = i;
        auto file_path = file_storage.file_path(i);
        info.path = std::string(file_path.begin(), file_path.end());
        info.size = file_storage.file_size(i);
        files.push_back(info);
    }
    return files;
}

bool torrent_set_file_priorities_internal(torrent_handle* handle, const std::vector<uint8_t>& priorities) {
    if (!handle) {
        return false;
    }
    // Convert vector<uint8_t> to vector<download_priority_t>
    std::vector<download_priority_t> libtorrent_priorities;
    libtorrent_priorities.reserve(priorities.size());
    for (uint8_t p : priorities) {
        libtorrent_priorities.push_back(static_cast<download_priority_t>(p));
    }
    handle->prioritize_files(libtorrent_priorities);
    return true;
}

float torrent_get_progress_internal(torrent_handle* handle) {
    if (!handle) {
        return 0.0f;
    }
    torrent_status status = handle->status();
    // progress_ppm is parts per million (0 to 1000000)
    // Convert to 0.0 to 1.0
    return static_cast<float>(status.progress_ppm) / 1000000.0f;
}

} // namespace libtorrent

