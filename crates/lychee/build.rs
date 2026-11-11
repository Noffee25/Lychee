fn main() {
    let config = slint_build::CompilerConfiguration::new().with_style("fluent".into());
    slint_build::compile_with_config("ui/app.slint", config).expect("failed to compile the Slint UI");
    windows_icon();
}

/// Give lychee.exe its icon. `assets/windows/lychee.res` is a compiled resource file (made by
/// `packaging/windows/make-icon.py`); MSVC's linker takes it as an input, so no resource
/// compiler is needed. Set LYCHEE_NO_EXE_ICON=1 to leave it out.
fn windows_icon() {
    println!("cargo:rerun-if-env-changed=LYCHEE_NO_EXE_ICON");
    let windows = std::env::var("CARGO_CFG_TARGET_OS").is_ok_and(|os| os == "windows");
    let msvc = std::env::var("CARGO_CFG_TARGET_ENV").is_ok_and(|env| env == "msvc");
    if !windows || !msvc || std::env::var_os("LYCHEE_NO_EXE_ICON").is_some() {
        return;
    }
    let manifest = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let res = manifest.join("../../assets/windows/lychee.res");
    println!("cargo:rerun-if-changed={}", res.display());
    if res.exists() {
        println!("cargo:rustc-link-arg-bins={}", res.display());
    }
}
