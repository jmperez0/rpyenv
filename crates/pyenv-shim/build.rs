//! Embeds `pyenv-shim.manifest` (spec §5.3, M5a): with `consoleAllocationPolicy=detached`,
//! Windows 11 24H2 and later give the shim no console when its caller has none, and the
//! shim asks for one itself (EAGER) or when the program prints (LAZY). Older Windows
//! ignores the setting.
fn main() {
    println!("cargo::rerun-if-changed=pyenv-shim.manifest");
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        let dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        println!("cargo::rustc-link-arg-bins=/MANIFEST:EMBED");
        println!("cargo::rustc-link-arg-bins=/MANIFESTINPUT:{dir}\\pyenv-shim.manifest");
    }
}
