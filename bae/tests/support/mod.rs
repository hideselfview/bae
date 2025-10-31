pub mod do_roundtrip;
// MockCloudStorage moved to bae::test_support

/// Initialize tracing for tests with proper test output handling
pub fn tracing_init() {
    let _ = tracing_subscriber::fmt()
        .with_test_writer()
        .with_line_number(true)
        .with_target(false) // Tests: hide target names for cleaner output
        .with_file(true) // Tests: show file names for debugging
        .try_init();
}
