fn main() {
    println!("cargo:rustc-link-arg-cdylib=/DELAYLOAD:victory-probe-delayed.dll");
    println!("cargo:rustc-link-lib=delayimp");
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        // The loader opens the DLL by path, so it needs neither the import library nor export object.
        println!("cargo:rustc-link-arg-cdylib=/NOIMPLIB");
        println!("cargo:rustc-link-arg-cdylib=/NOEXP");
    }
}
