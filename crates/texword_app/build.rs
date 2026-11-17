fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc")
    {
        // Windows defaults to a 1 MiB main thread stack, which GPUI + Taffy layout
        // can exceed in debug builds when rendering long chapters.
        println!("cargo:rustc-link-arg=/STACK:8388608");
    }
}
