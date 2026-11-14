// The web app is embedded from `web/dist`. When it has not been built (a
// plain `cargo build`), a placeholder page is embedded instead.
fn main() {
    let dist = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/dist");
    println!("cargo:rerun-if-changed={}", dist.display());
    if !dist.join("index.html").exists() {
        std::fs::create_dir_all(&dist).expect("create web/dist");
        std::fs::write(
            dist.join("index.html"),
            "<!doctype html><title>rupta</title><p>The web app was not built. Run <code>pnpm --dir web build</code> and rebuild the server.</p>\n",
        )
        .expect("write placeholder");
    }
}
