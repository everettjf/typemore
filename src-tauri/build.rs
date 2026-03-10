fn main() {
    #[cfg(target_os = "macos")]
    {
        println!(
            "cargo:rustc-link-arg-bin=TypeMore=-Wl,-rpath,@executable_path/../Frameworks"
        );
    }

    tauri_build::build()
}
