use std::path::Path;
use std::process::Command;

fn main() {
    // Run tailwindcss to generate CSS (using locally installed version)
    let output = Command::new("npx")
        .arg("tailwindcss")
        .args(["-i", "tailwind.css", "-o", "assets/tailwind.css"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
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

fn configure_libtorrent_paths() {
    #[cfg(target_os = "macos")]
    {
        // Check for Homebrew installation
        let homebrew_prefix = "/opt/homebrew";
        let pkgconfig_path = format!("{}/lib/pkgconfig", homebrew_prefix);
        let lib_path = format!("{}/lib", homebrew_prefix);

        // Check if pkgconfig directory exists
        if std::path::Path::new(&pkgconfig_path).exists() {
            // Set PKG_CONFIG_PATH so libtorrent-sys can find libtorrent-rasterbar.pc
            let existing_pkg_config = std::env::var("PKG_CONFIG_PATH").unwrap_or_default();
            let new_pkg_config = if existing_pkg_config.is_empty() {
                pkgconfig_path.clone()
            } else {
                format!("{}:{}", existing_pkg_config, pkgconfig_path)
            };
            std::env::set_var("PKG_CONFIG_PATH", &new_pkg_config);
            println!("cargo:warning=Set PKG_CONFIG_PATH={}", new_pkg_config);

            // Set LIBRARY_PATH for linking
            let existing_lib = std::env::var("LIBRARY_PATH").unwrap_or_default();
            let new_lib = if existing_lib.is_empty() {
                lib_path.clone()
            } else {
                format!("{}:{}", existing_lib, lib_path)
            };
            std::env::set_var("LIBRARY_PATH", &new_lib);
            println!("cargo:warning=Set LIBRARY_PATH={}", new_lib);
        }
    }
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
