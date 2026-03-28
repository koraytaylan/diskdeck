use std::fs;
use std::path::Path;

/// The macOS app menu displays the binary name as the application name.
/// This test ensures the [[bin]] name in Cargo.toml matches the productName
/// in tauri.conf.json so the menu reads "DiskDeck", not "diskdeck-backend".
#[test]
fn binary_name_matches_product_name() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));

    let cargo_toml = fs::read_to_string(manifest_dir.join("Cargo.toml"))
        .expect("Cargo.toml should be readable");
    let tauri_conf = fs::read_to_string(manifest_dir.join("tauri.conf.json"))
        .expect("tauri.conf.json should be readable");

    let conf: serde_json::Value =
        serde_json::from_str(&tauri_conf).expect("tauri.conf.json should be valid JSON");
    let product_name = conf["productName"]
        .as_str()
        .expect("productName should be a string in tauri.conf.json");

    // Parse [[bin]] name from Cargo.toml
    let cargo: toml::Value =
        cargo_toml.parse().expect("Cargo.toml should be valid TOML");
    let bins = cargo["bin"]
        .as_array()
        .expect("Cargo.toml should have a [[bin]] section");
    let bin_name = bins
        .iter()
        .find_map(|b| b["name"].as_str())
        .expect("[[bin]] should have a name field");

    assert_eq!(
        bin_name, product_name,
        "[[bin]] name in Cargo.toml ({bin_name:?}) must match productName in tauri.conf.json ({product_name:?}) \
         so macOS app menu shows the correct name"
    );
}
