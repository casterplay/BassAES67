//! Build script for bass-aes67 plugin.
//! Configures the linker to find the BASS library.

fn main() {
    // Path to BASS library (relative to project root)
    let bass_lib_path = "../bass24/c/x64";

    // Tell cargo where to find the BASS library
    println!("cargo:rustc-link-search=native={}", bass_lib_path);

    // Re-run build script if bass.lib changes
    println!("cargo:rerun-if-changed={}/bass.lib", bass_lib_path);
}
