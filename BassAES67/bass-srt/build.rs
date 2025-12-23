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
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        let base_path = std::path::Path::new(&manifest_dir).parent().unwrap();

        // BASS library
        let bass_path = base_path.join("bass24/c/x64");
        println!("cargo:rustc-link-search=native={}", bass_path.display());

        // Windows_need_builds folder with native libraries
        let libs_path = base_path.join("Windows_need_builds");

        // SRT (newly built)
        let srt_path = libs_path.join("srt/srt-1.5.4/build/Release");
        println!("cargo:rustc-link-search=native={}", srt_path.display());

        // OPUS (newly built)
        let opus_path = libs_path.join("opus-1.6/build/Release");
        println!("cargo:rustc-link-search=native={}", opus_path.display());

        // TwoLame
        let twolame_path = libs_path.join("twolame-main");
        println!("cargo:rustc-link-search=native={}", twolame_path.display());

        // FLAC (newly built) - lib is in src/libFLAC/Release, DLL is in objs/Release
        let flac_lib_path = libs_path.join("flac-master/build/src/libFLAC/Release");
        println!("cargo:rustc-link-search=native={}", flac_lib_path.display());

        // mpg123 (pre-built x64 binaries)
        let mpg123_path = libs_path.join("mpg123-1.32.10/mpg123-1.32.10-x86-64");
        println!("cargo:rustc-link-search=native={}", mpg123_path.display());

        // Link libraries
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
