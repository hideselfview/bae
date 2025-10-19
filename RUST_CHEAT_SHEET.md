# Rust Cheat Sheet

## Build Targets

### Library Crate
```bash
# Build library only
cargo build --lib

# Build library with specific features
cargo build --lib --features integration-test-utils

# Build library without default features
cargo build --lib --no-default-features
```

### Binary Crate
```bash
# Build main binary
cargo build --bin bae

# Run main binary
cargo run --bin bae
```

### All Targets
```bash
# Build everything (lib + bins + examples + tests)
cargo build

# Build with specific features
cargo build --features integration-test-utils

# Build without default features
cargo build --no-default-features
```

## Testing

### Unit Tests
```bash
# Run all unit tests (in src/ files)
cargo test --lib

# Run unit tests with output
cargo test --lib -- --nocapture

# Run specific unit test
cargo test --lib test_name

# Run unit tests in specific module
cargo test --lib encryption::tests

# Run ignored unit tests
cargo test --lib -- --ignored
```

### Integration Tests
```bash
# Run all integration tests
cargo test --test import_reassembly_test

# Run specific integration test
cargo test --test import_reassembly_test test_name

# Run integration tests with features
cargo test --test import_reassembly_test --features integration-test-utils
```

### Doc Tests
```bash
# Run only doc tests
cargo test --doc

# Run doc tests for specific module
cargo test --doc album_layout
```

### All Tests
```bash
# Run everything (unit + integration + doc tests)
cargo test

# Run with specific features
cargo test --features integration-test-utils

# Run with output
cargo test -- --nocapture
```

## Linting & Formatting

### Clippy
```bash
# Check library only
cargo clippy --lib

# Check with specific features
cargo clippy --lib --features integration-test-utils

# Check without default features
cargo clippy --lib --no-default-features

# Check all targets
cargo clippy

# Fix clippy suggestions
cargo clippy --fix
```

### Formatting
```bash
# Check formatting
cargo fmt --check

# Format code
cargo fmt

# Format specific files
cargo fmt -- src/file.rs
```

## Features

### Available Features
- `desktop` - Desktop UI features (default)
- `integration-test-utils` - Helper APIs for integration tests

### Feature Usage
```bash
# Build with specific feature
cargo build --features integration-test-utils

# Run tests with specific feature
cargo test --features integration-test-utils

# Check with specific feature
cargo clippy --features integration-test-utils
```

## Common Commands

### Development
```bash
# Full development cycle
cargo check && cargo test && cargo clippy && cargo fmt

# Quick check (fastest)
cargo check

# Run main application
cargo run

# Build release
cargo build --release
```

### Debugging
```bash
# Run with debug output
RUST_LOG=debug cargo run

# Run tests with debug output
RUST_LOG=debug cargo test

# Run with backtrace
RUST_BACKTRACE=1 cargo test
```

## Project Structure

### Conventional Files in `src/`
- **`lib.rs`** - Library crate root (required for library)
  - Contains `pub` functions, structs, etc. that other crates can use
  - Entry point for `cargo test --lib` and `cargo build --lib`
- **`main.rs`** - Binary crate root (required for binary)
  - Contains the `main()` function
  - Entry point for `cargo run` and `cargo build --bin bae`
- **`bin/`** - Additional binaries (optional)
  - Each `.rs` file becomes a separate binary
  - Example: `src/bin/server.rs`, `src/bin/client.rs`
  - Run with: `cargo run --bin server`
- **`examples/`** - Example programs (optional)
  - Each `.rs` file is a standalone example
  - Run with: `cargo run --example example_name`
- **Other `.rs` files** - Library modules (imported by `lib.rs`)

### Test Organization
- **Unit tests**: In `src/` files with `#[cfg(test)]` modules
- **Integration tests**: In `tests/` directory as separate crates
- **Doc tests**: Code examples in documentation comments

### Feature Gating
- Use `#[cfg(feature = "feature-name")]` for conditional compilation
- Configure integration tests with `[[test]]` blocks in `Cargo.toml`
- Use `required-features = ["feature-name"]` for test-specific features

## Troubleshooting

### Common Issues
- **Dead code warnings**: Use feature gates instead of `#[allow(dead_code)]`
- **Doc test failures**: Ensure examples use valid Rust syntax
- **Integration test failures**: Check if features are properly enabled
- **Module visibility**: Use `pub` for public APIs, `pub(crate)` for internal APIs

### Debug Commands
```bash
# Show dependency tree
cargo tree

# Show feature dependencies
cargo tree --features integration-test-utils

# Show build plan
cargo build --dry-run

# Show test plan
cargo test --dry-run
```
