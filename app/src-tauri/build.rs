fn main() {
    // Embed Info.plist into the dev binary so the macOS System Audio Recording
    // TCC prompt can attribute the request (tauri dev runs a bare binary; the
    // bundler merges the same file into the .app for real builds).
    #[cfg(target_os = "macos")]
    {
        let dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        println!("cargo:rustc-link-arg=-Wl,-sectcreate,__TEXT,__info_plist,{dir}/Info.plist");
        println!("cargo:rerun-if-changed=Info.plist");
        // Frameworks pulled in by cidre reference the Swift runtime via @rpath.
        println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
    }
    tauri_build::build()
}
