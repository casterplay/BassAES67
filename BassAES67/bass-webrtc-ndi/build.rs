//! Build script for bass-webrtc-ndi
//!
//! Configures NDI SDK paths for linking.

fn main() {
    // NDI SDK location on Windows
    // Default: "C:\Program Files\NDI\NDI 6 SDK"
    let ndi_sdk_path = std::env::var("NDI_SDK_DIR")
        .unwrap_or_else(|_| r"C:\Program Files\NDI\NDI 6 SDK".to_string());

    // Tell cargo to look for libraries in NDI SDK lib folder
    #[cfg(target_os = "windows")]
    {
        let lib_path = format!(r"{}\Lib\x64", ndi_sdk_path);
        println!("cargo:rustc-link-search=native={}", lib_path);
        println!("cargo:rerun-if-env-changed=NDI_SDK_DIR");
    }

    #[cfg(target_os = "linux")]
    {
        let lib_path = format!("{}/lib/x86_64-linux-gnu", ndi_sdk_path);
        println!("cargo:rustc-link-search=native={}", lib_path);
        println!("cargo:rerun-if-env-changed=NDI_SDK_DIR");
    }

    #[cfg(target_os = "macos")]
    {
        let lib_path = format!("{}/lib/macOS", ndi_sdk_path);
        println!("cargo:rustc-link-search=native={}", lib_path);
        println!("cargo:rerun-if-env-changed=NDI_SDK_DIR");
    }
}
