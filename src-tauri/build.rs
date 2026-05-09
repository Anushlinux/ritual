fn main() {
    println!("cargo:rerun-if-changed=.env");
    println!("cargo:rerun-if-changed=../.env");
    println!("cargo:rerun-if-env-changed=OPENAI_API_KEY");
    tauri_build::build();
}
