use sha2::{Digest, Sha256};
fn main() {
    let mut digest = Sha256::new();
    for path in [
        "build.rs",
        "Cargo.lock",
        "Cargo.toml",
        "src/model.rs",
        "src/engine.rs",
        "src/workflow.rs",
    ] {
        println!("cargo:rerun-if-changed={path}");
        digest.update(path.as_bytes());
        digest.update([0]);
        digest.update(std::fs::read(path).expect("implementation source required"));
        digest.update([0]);
    }
    println!(
        "cargo:rustc-env=ROWTIDE_IMPLEMENTATION_SHA256={:x}",
        digest.finalize()
    );
}
