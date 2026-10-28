fn main() {
    let out_dir = std::env::var("OUT_DIR").unwrap_or_default();
    let opt = std::env::var("OPT_LEVEL").unwrap_or_else(|_| "0".into());
    // `OUT_DIR` is `<target-dir>[/<triple>]/<profile>/build/<pkg>-<hash>/out`, and Cargo's
    // `PROFILE` only says `release` for profiles that inherit from it.
    let profile_dir = std::path::Path::new(&out_dir)
        .ancestors()
        .nth(3)
        .and_then(|dir| dir.file_name())
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    let profile = if profile_dir == "dist" {
        "dist"
    } else if opt == "0" {
        "dev"
    } else {
        "release"
    };
    println!("cargo:rustc-env=GRIT_BENCH_CARGO_PROFILE={profile}");
}
