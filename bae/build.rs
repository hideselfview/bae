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
                println!("cargo:warning=Failed to generate Tailwind CSS");
                println!(
                    "cargo:warning=STDERR: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
                println!(
                    "cargo:warning=STDOUT: {}",
                    String::from_utf8_lossy(&output.stdout)
                );
            } else {
                println!("cargo:warning=Tailwind CSS generated successfully");
            }
        }
        Err(e) => {
            println!("cargo:warning=Failed to run tailwindcss: {}", e);
        }
    }
}
