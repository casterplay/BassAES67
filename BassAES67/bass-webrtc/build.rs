//! Build script for bass-webrtc plugin.
//! Configures the linker to find the BASS library and OPUS library.

fn main() {
    #[cfg(target_os = "windows")]
    {
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        let base_path = std::path::Path::new(&manifest_dir).parent().unwrap();

        // BASS library
        let bass_path = base_path.join("bass24/c/x64");
        println!("cargo:rustc-link-search=native={}", bass_path.display());

        // Windows_need_builds folder with native libraries
        let libs_path = base_path.join("Windows_need_builds");

        // OPUS
        let opus_path = libs_path.join("opus-1.6/build/Release");
        println!("cargo:rustc-link-search=native={}", opus_path.display());
    }

    #[cfg(target_os = "linux")]
    {
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        let base_path = std::path::Path::new(&manifest_dir).parent().unwrap();

        // BASS library
        let bass_path = base_path.join("bass24-linux/libs/x86_64");
        println!("cargo:rustc-link-search=native={}", bass_path.display());

        // System libraries (assume installed via package manager)
        println!("cargo:rustc-link-search=native=/usr/local/lib");
    }

    #[cfg(target_os = "macos")]
    {
        // Homebrew locations
        println!("cargo:rustc-link-search=native=/usr/local/lib");
        println!("cargo:rustc-link-search=native=/opt/homebrew/lib");
    }
}
