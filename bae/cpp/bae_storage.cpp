#include "bae_storage.h"
#include <libtorrent/disk_interface.hpp>
#include <libtorrent/peer_request.hpp>
#include <libtorrent/disk_buffer_holder.hpp>
#include <libtorrent/io_context.hpp>
#include <libtorrent/settings_pack.hpp>
#include <vector>
#include <memory>
#include <string>

namespace libtorrent {

void BaeBufferAllocator::free_disk_buffer(char* b) {
    delete[] b;
}

BaeDiskInterface::BaeDiskInterface(
    ReadPieceCallback read_cb,
    WritePieceCallback write_cb,
    HashPieceCallback hash_cb
) : read_callback_(std::move(read_cb)),
    write_callback_(std::move(write_cb)),
    hash_callback_(std::move(hash_cb)),
    buffer_allocator_()
{
}

BaeDiskInterface::~BaeDiskInterface() = default;

storage_holder BaeDiskInterface::new_torrent(storage_params const& params, std::shared_ptr<void> const&) {
    // Create a storage holder - we don't need actual storage since we handle it in Rust
    return storage_holder();
}

void BaeDiskInterface::remove_torrent(storage_index_t) {
    // No-op: cleanup handled in Rust
}

void BaeDiskInterface::async_read(
    storage_index_t storage_idx,
    peer_request const& r,
    std::function<void(disk_buffer_holder, storage_error const&)> handler,
    disk_job_flags_t
) {
    // Read piece data via Rust callback
    try {
        std::vector<uint8_t> data = read_callback_(static_cast<int32_t>(storage_idx), r.piece, r.start, r.length);
        
        // Allocate buffer and copy data
        auto* buf = new char[data.size()];
        std::copy(data.begin(), data.end(), buf);
        
        // Create buffer holder with our allocator (allocator first, then buffer, then size)
        disk_buffer_holder holder(buffer_allocator_, buf, static_cast<int>(data.size()));
        
        handler(std::move(holder), storage_error());
    } catch (...) {
        storage_error err;
        err.ec = boost::system::error_code(boost::system::errc::io_error, boost::system::generic_category());
        // Create empty buffer holder for error case
        char* empty_buf = new char[0];
        disk_buffer_holder holder(buffer_allocator_, empty_buf, 0);
        handler(std::move(holder), err);
    }
}

bool BaeDiskInterface::async_write(
    storage_index_t storage_idx,
    peer_request const& r,
    char const* buf,
    std::shared_ptr<disk_observer> o,
    std::function<void(storage_error const&)> handler,
    disk_job_flags_t flags
) {
    // Write piece data via Rust callback
    try {
        std::vector<uint8_t> data(buf, buf + r.length);
        bool success = write_callback_(static_cast<int32_t>(storage_idx), r.piece, r.start, data);
        
        if (success) {
            handler(storage_error());
        } else {
            storage_error err;
            err.ec = boost::system::error_code(boost::system::errc::io_error, boost::system::generic_category());
            handler(err);
        }
    } catch (...) {
        storage_error err;
        err.ec = boost::system::error_code(boost::system::errc::io_error, boost::system::generic_category());
        handler(err);
    }
    
    return true; // Indicates async operation started
}

void BaeDiskInterface::async_hash(
    storage_index_t storage_idx,
    piece_index_t piece,
    span<sha256_hash> v2,
    disk_job_flags_t flags,
    std::function<void(piece_index_t, sha1_hash const&, storage_error const&)> handler
) {
    // Hash verification via Rust callback
    try {
        // Read piece data
        std::vector<uint8_t> data = read_callback_(static_cast<int32_t>(storage_idx), piece, 0, 0); // 0 size means read entire piece

        // Calculate SHA-1 hash (libtorrent uses SHA-1 for piece verification)
        sha1_hash hash;
        // TODO: Calculate actual SHA-1 hash using libtorrent's hasher

        // For now, create empty hash and call callback
        // The Rust callback will verify the hash
        bool valid = hash_callback_(static_cast<int32_t>(storage_idx), piece, std::vector<uint8_t>(hash.begin(), hash.end()));
        
        if (valid) {
            handler(piece, hash, storage_error());
        } else {
            storage_error err;
            err.ec = boost::system::error_code(boost::system::errc::invalid_argument, boost::system::generic_category());
            handler(piece, hash, err);
        }
    } catch (...) {
        storage_error err;
        err.ec = boost::system::error_code(boost::system::errc::io_error, boost::system::generic_category());
        sha1_hash hash;
        handler(piece, hash, err);
    }
}

void BaeDiskInterface::async_hash2(
    storage_index_t storage_idx,
    piece_index_t piece,
    int offset,
    disk_job_flags_t flags,
    std::function<void(piece_index_t, sha256_hash const&, storage_error const&)> handler
) {
    // Hash v2 block (SHA-256) via Rust callback
    try {
        // Read block data
        std::vector<uint8_t> data = read_callback_(static_cast<int32_t>(storage_idx), piece, offset, 0); // 0 size means read block

        // Calculate SHA-256 hash
        sha256_hash hash;
        // TODO: Calculate actual SHA-256 hash using libtorrent's hasher

        // For now, create empty hash and call callback
        bool valid = hash_callback_(static_cast<int32_t>(storage_idx), piece, std::vector<uint8_t>(hash.begin(), hash.end()));
        
        if (valid) {
            handler(piece, hash, storage_error());
        } else {
            storage_error err;
            err.ec = boost::system::error_code(boost::system::errc::invalid_argument, boost::system::generic_category());
            handler(piece, hash, err);
        }
    } catch (...) {
        storage_error err;
        err.ec = boost::system::error_code(boost::system::errc::io_error, boost::system::generic_category());
        sha256_hash hash;
        handler(piece, hash, err);
    }
}

// Stub implementations for other required methods
void BaeDiskInterface::async_move_storage(storage_index_t, std::string p, move_flags_t, std::function<void(status_t, std::string const&, storage_error const&)> handler) {
    handler(status_t::no_error, p, storage_error());
}

void BaeDiskInterface::async_rename_file(storage_index_t storage, file_index_t index, std::string name, std::function<void(std::string const&, file_index_t, storage_error const&)> handler) {
    handler(name, index, storage_error());
}

void BaeDiskInterface::async_delete_files(storage_index_t, remove_flags_t, std::function<void(storage_error const&)> handler) {
    handler(storage_error());
}

void BaeDiskInterface::async_set_file_priority(storage_index_t storage, aux::vector<download_priority_t, file_index_t> prio, std::function<void(storage_error const&, aux::vector<download_priority_t, file_index_t>)> handler) {
    handler(storage_error(), std::move(prio));
}

void BaeDiskInterface::async_clear_piece(storage_index_t, piece_index_t index, std::function<void(piece_index_t)> handler) {
    handler(index);
}

void BaeDiskInterface::async_check_files(storage_index_t storage, add_torrent_params const* resume_data, aux::vector<std::string, file_index_t> links, std::function<void(status_t, storage_error const&)> handler) {
    handler(status_t::no_error, storage_error());
}

void BaeDiskInterface::async_stop_torrent(storage_index_t storage, std::function<void()> handler) {
    handler();
}

void BaeDiskInterface::async_release_files(storage_index_t storage, std::function<void()> handler) {
    handler();
}

void BaeDiskInterface::abort(bool wait) {
    // No-op - we don't have threads to abort
}

void BaeDiskInterface::submit_jobs() {
    // No-op - we handle jobs synchronously
}

void BaeDiskInterface::update_stats_counters(counters& c) const {
    // No-op - we don't track stats
}

std::vector<open_file_state> BaeDiskInterface::get_status(storage_index_t storage) const {
    return std::vector<open_file_state>();
}

void BaeDiskInterface::settings_updated() {
    // No-op - we don't need to react to settings changes
}

// Factory function implementation
std::function<std::unique_ptr<disk_interface>(io_context&, settings_interface const&, counters&)> create_bae_disk_io_constructor(
    ReadPieceCallback read_cb,
    WritePieceCallback write_cb,
    HashPieceCallback hash_cb
) {
    // Create a disk_io_constructor (std::function) that returns our custom disk interface
    return [read_cb, write_cb, hash_cb](io_context&, settings_interface const&, counters&) -> std::unique_ptr<disk_interface> {
        return std::make_unique<BaeDiskInterface>(read_cb, write_cb, hash_cb);
    };
}

} // namespace libtorrent

