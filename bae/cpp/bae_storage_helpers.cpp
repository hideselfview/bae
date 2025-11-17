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
#include <libtorrent/settings_pack.hpp>
#include <libtorrent/session_status.hpp>
#include <libtorrent/alert_types.hpp>
#include <libtorrent/alert.hpp>
#include <libtorrent/sha1_hash.hpp>
#include <sstream>
#include <iomanip>

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

int32_t torrent_get_num_peers(torrent_handle* handle) {
    if (!handle) {
        return 0;
    }
    torrent_status status = handle->status();
    return static_cast<int32_t>(status.num_peers);
}

int32_t torrent_get_num_seeds(torrent_handle* handle) {
    if (!handle) {
        return 0;
    }
    torrent_status status = handle->status();
    return static_cast<int32_t>(status.num_seeds);
}

std::string torrent_get_tracker_status(torrent_handle* handle) {
    if (!handle) {
        return "No handle";
    }
    
    torrent_status status = handle->status();
    
    // Get tracker count from torrent_info if available
    auto torrent_file = status.torrent_file.lock();
    if (!torrent_file) {
        return "No metadata (trackers unknown)";
    }
    
    // Get tracker URLs from torrent_info
    auto trackers = torrent_file->trackers();
    if (trackers.empty()) {
        return "No trackers in torrent";
    }
    
    std::string result = std::to_string(trackers.size()) + " tracker(s): ";
    bool first = true;
    for (const auto& tracker : trackers) {
        if (!first) {
            result += ", ";
        }
        result += tracker.url;
        first = false;
    }
    
    return result;
}

std::unique_ptr<LibTorrentInfo> get_torrent_info_internal(const std::string& file_path) {
    error_code ec;
    torrent_info ti(file_path, ec);
    if (ec) {
        return nullptr;
    }
    
    auto info = std::make_unique<LibTorrentInfo>();
    
    // Extract basic info
    info->name = ti.name();
    info->total_size = ti.total_size();
    info->piece_length = ti.piece_length();
    info->num_pieces = ti.num_pieces();
    info->is_private = ti.priv();
    
    // Extract comment and creator
    info->comment = ti.comment();
    info->creator = ti.creator();
    
    // Extract creation date
    info->creation_date = ti.creation_date();
    
    // Extract trackers
    auto trackers = ti.trackers();
    for (const auto& tracker : trackers) {
        info->trackers.push_back(tracker.url);
    }
    
    // Extract file list
    auto file_storage = ti.files();
    for (int i = 0; i < file_storage.num_files(); ++i) {
        LibTorrentFileInfo file_info;
        file_info.index = i;
        auto file_path = file_storage.file_path(i);
        file_info.path = std::string(file_path.begin(), file_path.end());
        file_info.size = file_storage.file_size(i);
        info->files.push_back(file_info);
    }
    
    return info;
}

void set_seed_mode(add_torrent_params* params, bool seed_mode) {
    if (params && seed_mode) {
        params->flags |= torrent_flags::seed_mode;
    }
}

void set_paused(add_torrent_params* params, bool paused) {
    if (params && paused) {
        params->flags |= torrent_flags::paused;
    }
}

void set_listen_interfaces(session_params* params, const std::string& interfaces) {
    if (params && !interfaces.empty()) {
        params->settings.set_str(settings_pack::listen_interfaces, interfaces);
    }
}

std::string session_get_listen_interfaces(session* sess) {
    if (!sess) {
        return "No session";
    }
    try {
        auto settings = sess->get_settings();
        std::string interfaces = settings.get_str(settings_pack::listen_interfaces);
        if (interfaces.empty()) {
            return "Default (not explicitly set)";
        }
        return interfaces;
    } catch (...) {
        return "Error querying interfaces";
    }
}

std::string session_get_listening_port(session* sess) {
    if (!sess) {
        return "No session";
    }
    // Note: session_status doesn't expose listen_port directly in libtorrent 2.0
    // We can get it from settings, but for now just return a placeholder
    // The important thing is confirming the interface is set correctly
    return "Port: (checking via settings)";
}

void torrent_pause(torrent_handle* handle) {
    if (handle) {
        handle->pause();
    }
}

void torrent_resume(torrent_handle* handle) {
    if (handle) {
        handle->resume();
    }
}

// Helper function to convert sha1_hash to hex string
std::string hash_to_string(const libtorrent::sha1_hash& hash) {
    std::ostringstream oss;
    oss << std::hex << std::uppercase;
    for (int i = 0; i < 20; ++i) {
        oss << std::setw(2) << std::setfill('0') << static_cast<int>(hash[i]);
    }
    return oss.str();
}

std::vector<libtorrent::AlertData> session_pop_alerts(session* sess) {
    std::vector<libtorrent::AlertData> alerts;
    if (!sess) {
        return alerts;
    }
    
    // Pop all pending alerts from the session
    std::vector<libtorrent::alert*> alert_queue;
    sess->pop_alerts(&alert_queue);
    
    for (auto* alert_ptr : alert_queue) {
        if (!alert_ptr) {
            continue;
        }
        
        libtorrent::AlertData alert_data;
        alert_data.type = libtorrent::ALERT_UNKNOWN;
        alert_data.num_peers = 0;
        alert_data.num_seeds = 0;
        alert_data.progress = 0.0f;
        
        // Get info_hash from alert if available
        auto* torrent_alert = dynamic_cast<libtorrent::torrent_alert*>(alert_ptr);
        if (torrent_alert) {
            alert_data.info_hash = hash_to_string(torrent_alert->handle.info_hash());
        }
        
        // Handle different alert types
        if (auto* tracker_alert = dynamic_cast<libtorrent::tracker_announce_alert*>(alert_ptr)) {
            alert_data.type = libtorrent::ALERT_TRACKER_ANNOUNCE;
            alert_data.tracker_url = tracker_alert->tracker_url();
            alert_data.tracker_message = "Announcing";
        } else if (auto* tracker_error = dynamic_cast<libtorrent::tracker_error_alert*>(alert_ptr)) {
            alert_data.type = libtorrent::ALERT_TRACKER_ERROR;
            alert_data.tracker_url = tracker_error->tracker_url();
            alert_data.tracker_message = tracker_error->message();
            alert_data.error_message = tracker_error->message();
        } else if (auto* peer_alert = dynamic_cast<libtorrent::peer_alert*>(alert_ptr)) {
            if (dynamic_cast<libtorrent::peer_connect_alert*>(alert_ptr)) {
                alert_data.type = libtorrent::ALERT_PEER_CONNECT;
            } else if (dynamic_cast<libtorrent::peer_disconnected_alert*>(alert_ptr)) {
                alert_data.type = libtorrent::ALERT_PEER_DISCONNECT;
            }
            // Get peer counts from torrent status
            auto status = peer_alert->handle.status();
            alert_data.num_peers = static_cast<int32_t>(status.num_peers);
            alert_data.num_seeds = static_cast<int32_t>(status.num_seeds);
        } else if (auto* file_alert = dynamic_cast<libtorrent::file_completed_alert*>(alert_ptr)) {
            alert_data.type = libtorrent::ALERT_FILE_COMPLETED;
            // file_completed_alert has index property (file_index_t)
            // Get file path from torrent_info if available
            auto status = file_alert->handle.status();
            auto torrent_file = status.torrent_file.lock();
            if (torrent_file) {
                auto file_storage = torrent_file->files();
                auto file_index = file_alert->index;
                // Convert file_index_t to int for comparison
                int file_index_int = static_cast<int>(file_index);
                if (file_index_int >= 0 && file_index_int < file_storage.num_files()) {
                    auto file_path = file_storage.file_path(file_index);
                    alert_data.file_path = std::string(file_path.begin(), file_path.end());
                }
            }
            alert_data.progress = static_cast<float>(status.progress_ppm) / 1000000.0f;
        } else if (auto* metadata_alert = dynamic_cast<libtorrent::metadata_received_alert*>(alert_ptr)) {
            alert_data.type = libtorrent::ALERT_METADATA_RECEIVED;
        } else if (auto* state_alert = dynamic_cast<libtorrent::state_changed_alert*>(alert_ptr)) {
            alert_data.type = libtorrent::ALERT_STATE_CHANGED;
            auto status = state_alert->handle.status();
            alert_data.num_peers = static_cast<int32_t>(status.num_peers);
            alert_data.num_seeds = static_cast<int32_t>(status.num_seeds);
            alert_data.progress = static_cast<float>(status.progress_ppm) / 1000000.0f;
        }
        
        alerts.push_back(alert_data);
    }
    
    return alerts;
}

} // namespace libtorrent

