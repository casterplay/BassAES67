// Build script for bass-opus-web
// Links against libbass and libopus

fn main() {
    #[cfg(target_os = "linux")]
    {
        // Link against BASS library (from bass-aes67 build)
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        let bass_path = std::path::Path::new(&manifest_dir)
            .parent()
            .unwrap()
            .join("bass-aes67/target/release");
        println!("cargo:rustc-link-search=native={}", bass_path.display());

        // libopus is in system path (pkg-config finds it)
    }

    #[cfg(target_os = "windows")]
    {
        // On Windows, expect BASS DLL in same directory
        if let Ok(manifest_dir) = std::env::var("CARGO_MANIFEST_DIR") {
            println!("cargo:rustc-link-search=native={}", manifest_dir);
        }
    }
}
