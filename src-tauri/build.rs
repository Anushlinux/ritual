fn main() {
    println!("cargo:rerun-if-changed=.env");
    println!("cargo:rerun-if-changed=../.env");
    println!("cargo:rerun-if-env-changed=ANTHROPIC_API_KEY");
    println!("cargo:rerun-if-env-changed=ANTHROPIC_MODEL");
    println!("cargo:rerun-if-env-changed=ANTHROPIC_VERSION");
    tauri_build::build();
}
