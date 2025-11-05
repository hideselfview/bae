#!/bin/bash

# Clean up old test data
rm -rf /tmp/bae_test_import
mkdir -p /tmp/bae_test_import

# Set environment to use test directory
export BAE_HOME=/tmp/bae_test_import
export BAE_BUCKET=bae-test-import
export RUST_LOG=debug

echo "=== Starting bae with test environment ==="
echo "BAE_HOME=$BAE_HOME"
echo "BAE_BUCKET=$BAE_BUCKET"
echo ""
echo "Import a CUE/FLAC album and check the logs for:"
echo "  - 'Time Xms: estimated byte Y'"
echo "  - 'Time Xms: found frame at byte Z'"
echo ""
echo "Press Ctrl+C when done"
echo ""

cd /Users/dima/dev/bae/bae
cargo run --release

