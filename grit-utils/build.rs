fn main() {
    let out_dir = std::env::var("OUT_DIR").unwrap_or_default();
    let opt = std::env::var("OPT_LEVEL").unwrap_or_else(|_| "0".into());
    let profile = if out_dir.contains("/target/dist/") || out_dir.contains("\\target\\dist\\") {
        "dist"
    } else if opt == "0" {
        "dev"
    } else {
        "release"
    };
    println!("cargo:rustc-env=GRIT_BENCH_CARGO_PROFILE={profile}");
}
