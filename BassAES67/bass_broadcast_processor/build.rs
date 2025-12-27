//! Build script for bass-broadcast-processor.
//! Configures the linker to find the BASS library.

fn main() {
    // Path to BASS library (relative to project root)
    #[cfg(target_os = "windows")]
    let bass_lib_path = "../bass24/c/x64";

    #[cfg(target_os = "linux")]
    let bass_lib_path = "../bass24-linux/libs/x86_64";

    #[cfg(target_os = "macos")]
    let bass_lib_path = "../bass24/c";

    // Tell cargo where to find the BASS library
    println!("cargo:rustc-link-search=native={}", bass_lib_path);

    // Re-run build script if library changes
    #[cfg(target_os = "windows")]
    println!("cargo:rerun-if-changed={}/bass.lib", bass_lib_path);

    #[cfg(not(target_os = "windows"))]
    println!("cargo:rerun-if-changed={}/libbass.so", bass_lib_path);
}
