use std::path::Path;
use std::process::Command;

fn main() {
    // Compile C++ custom storage backend with cxx bridge
    compile_cpp_storage();
    // Run tailwindcss to generate CSS (using locally installed version)
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let tailwind_input = Path::new(manifest_dir).join("tailwind.css");
    let tailwind_output = Path::new(manifest_dir).join("assets/tailwind.css");

    println!("cargo:rerun-if-changed={}", tailwind_input.display());
    println!(
        "cargo:rerun-if-changed={}",
        Path::new(manifest_dir).join("tailwind.config.js").display()
    );

    let output = Command::new("npx")
        .arg("tailwindcss")
        .args([
            "-i",
            tailwind_input.to_str().unwrap(),
            "-o",
            tailwind_output.to_str().unwrap(),
        ])
        .current_dir(manifest_dir)
        .output();

    match output {
        Ok(output) => {
            if !output.status.success() {
                println!("cargo:error=Failed to generate Tailwind CSS");
                println!(
                    "cargo:error=STDERR: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
                println!(
                    "cargo:error=STDOUT: {}",
                    String::from_utf8_lossy(&output.stdout)
                );
            } else {
                println!("cargo:warning=Tailwind CSS generated successfully");
            }
        }
        Err(e) => {
            println!("cargo:error=Failed to run tailwindcss: {}", e);
        }
    }

    // Apply dioxus-html patch for drag-and-drop fix
    apply_dioxus_patch();
}

fn apply_dioxus_patch() {
    // Get patch file path (next to build.rs)
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let patch_file = Path::new(manifest_dir).join("patches/dioxus-html-0.7.1.patch");

    if !patch_file.exists() {
        panic!(
            "dioxus-html patch file not found at: {}",
            patch_file.display()
        );
    }

    let home = match std::env::var("HOME") {
        Ok(h) => h,
        Err(_) => return,
    };

    // Find dioxus-html in cargo registry
    let registry_base = Path::new(&home).join(".cargo/registry/src");
    if !registry_base.exists() {
        return;
    }

    // Look for dioxus-html-0.7.1 in the registry
    let entries = match std::fs::read_dir(&registry_base) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let dioxus_html_dir = path.join("dioxus-html-0.7.1");
            if dioxus_html_dir.exists() {
                apply_patch_file(&dioxus_html_dir, &patch_file);
                return;
            }
        }
    }
}

fn apply_patch_file(target_dir: &Path, patch_file: &Path) {
    // Check if patch is already applied by looking for the fix
    let data_transfer_rs = target_dir.join("src/data_transfer.rs");
    if let Ok(content) = std::fs::read_to_string(&data_transfer_rs) {
        if content.contains("#[serde(rename = \"type\")]") {
            // Patch already applied
            return;
        }
    }

    // Apply patch using the `patch` command
    let output = Command::new("patch")
        .arg("-p1")
        .arg("-d")
        .arg(target_dir)
        .arg("-i")
        .arg(patch_file)
        .arg("--quiet")
        .arg("--forward")
        .output();

    match output {
        Ok(output) => {
            if output.status.success() {
                println!(
                    "cargo:warning=Applied patch: {}",
                    patch_file.file_name().unwrap_or_default().to_string_lossy()
                );
            } else {
                // Patch might already be applied or failed - check stderr
                let stderr = String::from_utf8_lossy(&output.stderr);
                let stdout = String::from_utf8_lossy(&output.stdout);
                // Ignore "already applied" or "Skipping" messages
                if !stderr.contains("already applied")
                    && !stderr.contains("Skipping")
                    && !stdout.contains("already applied")
                    && !stdout.contains("Skipping")
                {
                    // Only warn if it's a real error
                    if !stderr.is_empty() {
                        println!(
                            "cargo:warning=Failed to apply patch {}: {}",
                            patch_file.file_name().unwrap_or_default().to_string_lossy(),
                            stderr
                        );
                    }
                }
            }
        }
        Err(e) => {
            // `patch` command not available - silently fail
            // This is expected on some systems or if patch isn't installed
            if e.kind() != std::io::ErrorKind::NotFound {
                println!(
                    "cargo:warning=Could not apply patch {}: {}",
                    patch_file.file_name().unwrap_or_default().to_string_lossy(),
                    e
                );
            }
        }
    }
}

fn compile_cpp_storage() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let cpp_dir = Path::new(manifest_dir).join("cpp");

    if !cpp_dir.exists() {
        println!("cargo:warning=CPP directory not found, skipping C++ compilation");
        return;
    }

    let header = cpp_dir.join("bae_storage.h");
    let source = cpp_dir.join("bae_storage.cpp");
    let helpers_header = cpp_dir.join("bae_storage_helpers.h");
    let helpers_source = cpp_dir.join("bae_storage_helpers.cpp");
    let ffi_rs = Path::new(manifest_dir).join("src/torrent/ffi.rs");

    if !header.exists() || !source.exists() || !helpers_header.exists() || !helpers_source.exists()
    {
        println!("cargo:warning=Custom storage C++ files not found, skipping compilation");
        return;
    }

    // Use cxx_build to compile C++ code with bridge code generation
    // This ensures cxx bridge code is generated and compiled together with our C++ code
    // Note: This requires libtorrent development headers to be installed
    let wrappers_source = cpp_dir.join("bae_storage_cxx_wrappers.cpp");
    cxx_build::bridge(&ffi_rs)
        .file(&source)
        .file(&helpers_source)
        .file(&wrappers_source)
        .include(&cpp_dir)
        .include("/opt/homebrew/include") // macOS Homebrew
        .include("/usr/local/include") // Standard Unix
        .flag("-std=c++17")
        .compile("bae_storage");

    println!("cargo:rerun-if-changed={}", ffi_rs.display());
    println!("cargo:rerun-if-changed={}", source.display());
    println!("cargo:rerun-if-changed={}", helpers_source.display());
    println!("cargo:rerun-if-changed={}", header.display());
    println!("cargo:rerun-if-changed={}", helpers_header.display());
    println!("cargo:rerun-if-changed={}", wrappers_source.display());

    // Link directives for the library and tests.
    // Note: These directives DON'T propagate to binaries automatically - binaries must
    // use #[link] attributes (see src/main.rs) to link these native libraries.
    let out_dir = std::env::var("OUT_DIR").unwrap();
    println!("cargo:rustc-link-search=native={}", out_dir);
    println!("cargo:rustc-link-lib=static=bae_storage");
    println!("cargo:rustc-link-lib=torrent-rasterbar");
    println!("cargo:rustc-link-search=native=/opt/homebrew/lib"); // macOS Homebrew
    println!("cargo:rustc-link-search=native=/usr/local/lib"); // Standard Unix
}
