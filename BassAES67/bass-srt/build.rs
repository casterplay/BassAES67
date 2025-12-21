// Build script for bass-srt
// Links against libsrt-gnutls on Linux and libbass

fn main() {
    // On Linux, link against libsrt-gnutls (Debian/Ubuntu variant)
    #[cfg(target_os = "linux")]
    {
        println!("cargo:rustc-link-search=native=/usr/lib/x86_64-linux-gnu");
        println!("cargo:rustc-link-lib=dylib=srt-gnutls");

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
