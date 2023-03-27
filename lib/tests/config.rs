use std::env;

#[test]
fn test_config() {
    env::set_var(
        "CONFIG_PATH",
        concat!(env!("CARGO_MANIFEST_DIR"), "/../config.toml"),
    );
    lib::config::load().unwrap();

    env::set_var(
        "CONFIG_PATH",
        concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"),
    );
    assert!(lib::config::load().is_err());
}
