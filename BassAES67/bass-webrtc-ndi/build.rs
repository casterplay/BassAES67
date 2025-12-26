//! Build script for bass-webrtc-ndi
//!
//! Configures library paths for linking:
//! - NDI SDK (for NDI output)
//! - BASS (for audio I/O)
//! - OPUS (for audio codec)
//! - FFmpeg (for video decoding - Phase 2B)

fn main() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let base_path = std::path::Path::new(&manifest_dir).parent().unwrap();

    // NDI SDK location
    let ndi_sdk_path = std::env::var("NDI_SDK_DIR")
        .unwrap_or_else(|_| r"C:\Program Files\NDI\NDI 6 SDK".to_string());

    #[cfg(target_os = "windows")]
    {
        // NDI SDK
        let ndi_lib_path = format!(r"{}\Lib\x64", ndi_sdk_path);
        println!("cargo:rustc-link-search=native={}", ndi_lib_path);

        // BASS library
        let bass_path = base_path.join("bass24/c/x64");
        println!("cargo:rustc-link-search=native={}", bass_path.display());

        // Windows_need_builds folder with native libraries
        let libs_path = base_path.join("Windows_need_builds");

        // OPUS
        let opus_path = libs_path.join("opus-1.6/build/Release");
        println!("cargo:rustc-link-search=native={}", opus_path.display());

        // FFmpeg (for video decoding - Phase 2B)
        let ffmpeg_path = libs_path.join("ffmpeg-gpl-shared/lib");
        println!("cargo:rustc-link-search=native={}", ffmpeg_path.display());

        // Rerun if paths change
        println!("cargo:rerun-if-env-changed=NDI_SDK_DIR");
    }

    #[cfg(target_os = "linux")]
    {
        // NDI SDK
        let ndi_lib_path = format!("{}/lib/x86_64-linux-gnu", ndi_sdk_path);
        println!("cargo:rustc-link-search=native={}", ndi_lib_path);

        // BASS library
        let bass_path = base_path.join("bass24-linux/libs/x86_64");
        println!("cargo:rustc-link-search=native={}", bass_path.display());

        // System libraries
        println!("cargo:rustc-link-search=native=/usr/local/lib");

        println!("cargo:rerun-if-env-changed=NDI_SDK_DIR");
    }

    #[cfg(target_os = "macos")]
    {
        // NDI SDK
        let ndi_lib_path = format!("{}/lib/macOS", ndi_sdk_path);
        println!("cargo:rustc-link-search=native={}", ndi_lib_path);

        // Homebrew locations
        println!("cargo:rustc-link-search=native=/usr/local/lib");
        println!("cargo:rustc-link-search=native=/opt/homebrew/lib");

        println!("cargo:rerun-if-env-changed=NDI_SDK_DIR");
    }
}
