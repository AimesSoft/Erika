use std::env;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && env::var_os("CARGO_FEATURE_LIBASS").is_some()
    {
        println!("cargo:rustc-link-lib=dwrite");
    }
}
