// Build script for bass-srt
// Links against libsrt on Linux and libbass

fn main() {
    // On Linux, link against libsrt (SRT 1.5.4 from local install)
    #[cfg(target_os = "linux")]
    {
        // Use local SRT 1.5.4 installation
        let home = std::env::var("HOME").unwrap_or_else(|_| "/home/kennet".to_string());
        let srt_path = format!("{}/local/srt-1.5.4/lib", home);
        println!("cargo:rustc-link-search=native={}", srt_path);
        println!("cargo:rustc-link-lib=dylib=srt");

        // Link against BASS library (from bass-aes67 build)
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        let bass_path = std::path::Path::new(&manifest_dir)
            .parent()
            .unwrap()
            .join("bass-aes67/target/release");
        println!("cargo:rustc-link-search=native={}", bass_path.display());
    }

    #[cfg(target_os = "windows")]
    {
        // On Windows, expect SRT DLL in same directory or standard location
        if let Ok(manifest_dir) = std::env::var("CARGO_MANIFEST_DIR") {
            println!("cargo:rustc-link-search=native={}", manifest_dir);
        }
        println!("cargo:rustc-link-lib=dylib=srt");
    }

    #[cfg(target_os = "macos")]
    {
        // On macOS, try Homebrew location
        println!("cargo:rustc-link-search=native=/usr/local/lib");
        println!("cargo:rustc-link-search=native=/opt/homebrew/lib");
        println!("cargo:rustc-link-lib=dylib=srt");
    }
}
