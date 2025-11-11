#ifndef BAE_STORAGE_H
#define BAE_STORAGE_H

#include <libtorrent/storage_defs.hpp>
#include <libtorrent/disk_interface.hpp>
#include <libtorrent/disk_buffer_holder.hpp>
#include <libtorrent/io_context.hpp>
#include <libtorrent/settings_pack.hpp>
#include <libtorrent/aux_/session_settings.hpp>
#include <functional>
#include <memory>

namespace libtorrent {

// Forward declarations
struct disk_io_constructor;

// Callback types for Rust FFI
// These will be called from C++ to Rust
// storage_index identifies which torrent's storage to use
using ReadPieceCallback = std::function<std::vector<uint8_t>(int32_t storage_index, int32_t piece_index, int32_t offset, int32_t size)>;
using WritePieceCallback = std::function<bool(int32_t storage_index, int32_t piece_index, int32_t offset, const std::vector<uint8_t>& data)>;
using HashPieceCallback = std::function<bool(int32_t storage_index, int32_t piece_index, const std::vector<uint8_t>& hash)>;

// Simple buffer allocator for disk_buffer_holder
class BaeBufferAllocator : public buffer_allocator_interface {
public:
    void free_disk_buffer(char* b) override;
};

// Custom disk interface implementation for BAE storage
class BaeDiskInterface : public disk_interface {
public:
    BaeDiskInterface(
        ReadPieceCallback read_cb,
        WritePieceCallback write_cb,
        HashPieceCallback hash_cb
    );
    
    ~BaeDiskInterface() override;

    // Required disk_interface methods
    storage_holder new_torrent(storage_params const& params, std::shared_ptr<void> const&) override;
    void remove_torrent(storage_index_t) override;
    
    void async_read(storage_index_t storage, peer_request const& r, std::function<void(disk_buffer_holder, storage_error const&)> handler, disk_job_flags_t flags = {}) override;
    bool async_write(storage_index_t storage, peer_request const& r, char const* buf, std::shared_ptr<disk_observer> o, std::function<void(storage_error const&)> handler, disk_job_flags_t flags = {}) override;
    
    void async_hash(storage_index_t storage, piece_index_t piece, span<sha256_hash> v2, disk_job_flags_t flags, std::function<void(piece_index_t, sha1_hash const&, storage_error const&)> handler) override;
    void async_hash2(storage_index_t storage, piece_index_t piece, int offset, disk_job_flags_t flags, std::function<void(piece_index_t, sha256_hash const&, storage_error const&)> handler) override;
    
    void async_move_storage(storage_index_t storage, std::string p, move_flags_t flags, std::function<void(status_t, std::string const&, storage_error const&)> handler) override;
    void async_rename_file(storage_index_t storage, file_index_t index, std::string name, std::function<void(std::string const&, file_index_t, storage_error const&)> handler) override;
    void async_delete_files(storage_index_t storage, remove_flags_t options, std::function<void(storage_error const&)> handler) override;
    void async_set_file_priority(storage_index_t storage, aux::vector<download_priority_t, file_index_t> prio, std::function<void(storage_error const&, aux::vector<download_priority_t, file_index_t>)> handler) override;
    void async_clear_piece(storage_index_t, piece_index_t index, std::function<void(piece_index_t)> handler) override;
    
    void async_check_files(storage_index_t storage, add_torrent_params const* resume_data, aux::vector<std::string, file_index_t> links, std::function<void(status_t, storage_error const&)> handler) override;
    
    void async_stop_torrent(storage_index_t storage, std::function<void()> handler) override;
    
    void async_release_files(storage_index_t storage, std::function<void()> handler) override;
    
    void abort(bool wait) override;
    void submit_jobs() override;
    
    void update_stats_counters(counters& c) const override;
    
    std::vector<open_file_state> get_status(storage_index_t storage) const override;
    
    void settings_updated() override;

private:
    ReadPieceCallback read_callback_;
    WritePieceCallback write_callback_;
    HashPieceCallback hash_callback_;
    BaeBufferAllocator buffer_allocator_;
};

// Factory function to create disk_io_constructor (std::function)
// This will be called from Rust via FFI
// Note: Uses types from libtorrent namespace (io_context, settings_interface, counters, disk_interface)
std::function<std::unique_ptr<disk_interface>(io_context&, settings_interface const&, counters&)> create_bae_disk_io_constructor(
    ReadPieceCallback read_cb,
    WritePieceCallback write_cb,
    HashPieceCallback hash_cb
);

} // namespace libtorrent

#endif // BAE_STORAGE_H

