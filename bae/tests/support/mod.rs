pub mod do_roundtrip;
pub mod mock_cloud_storage;

pub use do_roundtrip::*;
pub use mock_cloud_storage::MockCloudStorage;

/// Initialize tracing for tests with proper test output handling
pub fn tracing_init() {
    let _ = tracing_subscriber::fmt().with_test_writer().try_init();
}
