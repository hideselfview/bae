# Cargo Cheat Sheet

## Build Targets

Cargo can build different types of targets:

- **`--lib`** - The library crate (`src/lib.rs`). Contains reusable code that other crates can depend on.
- **`--bin <name>`** - Binary executables. Main binary is in `src/main.rs`, additional binaries in `src/bin/*.rs`.
- **`--test <name>`** - Integration tests in `tests/*.rs`. Each file is a separate test crate.
- **`--example <name>`** - Example programs in `examples/*.rs`. Demonstrate library usage.
- **`--bench <name>`** - Benchmarks in `benches/*.rs`. Performance testing.
- **(no flag)** - All targets: lib + bins + examples + tests + benches.

Most cargo commands (`build`, `run`, `test`, `clippy`, `check`) work with any target flag. Examples:

```bash
# Build everything (lib + bins + examples + tests + benches)
cargo build

# Build just the library
cargo build --lib

# Run a specific binary
cargo run --bin bae

# Run unit tests
cargo test --lib

# Run clippy on a specific integration test
cargo clippy --test test_roundtrip_vinyl

# Run an example
cargo run --example my_example
```

## Testing

### Unit Tests
```bash
# Run all unit tests (in src/ files)
cargo test --lib

# Run unit tests with output
cargo test --lib -- --no-capture

# Run specific unit test
cargo test --lib test_name

# Run unit tests in specific module
cargo test --lib encryption::tests

# Run ignored unit tests
cargo test --lib -- --ignored
```

### Integration Tests
```bash
# Run `test_roundtrip_simple` (requires test-utils feature)
cargo test --test test_roundtrip_simple --features test-utils
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
cargo test --features test-utils

# Run with output
cargo test -- --no-capture
```

## Linting & Formatting

### Clippy
```bash
# Check all targets and treat warnings as errors
cargo clippy --all-targets -- -D warnings

# Fix clippy suggestions
cargo clippy --fix
```

**`-D` options:**
- `-D warnings` - Treat all warnings as errors
- `-D clippy::all` - Enable all clippy lints
- `-D clippy::pedantic` - Enable pedantic lints
- `-D clippy::nursery` - Enable experimental lints
- `-D clippy::cargo` - Enable cargo-specific lints

**Other options:**
- `-A <lint>` - Allow specific lint
- `-W <lint>` - Warn for specific lint
- `-D <lint>` - Deny specific lint
- `--fix` - Apply automatic fixes

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
- `test-utils` - Helper APIs for tests

### Feature Usage
```bash
# Build with specific feature
cargo build --features test-utils

# Run tests with specific feature
cargo test --features test-utils

# Check with specific feature
cargo clippy --features test-utils
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
