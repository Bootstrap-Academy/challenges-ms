use std::env;

#[test]
fn test_config() {
    env::set_var(
        "CONFIG_PATH",
        concat!(env!("CARGO_MANIFEST_DIR"), "/../config.toml"),
    );
    let config = lib::config::load().unwrap();
    assert!(config.internal_jwt_secrets.is_empty());

    // in production the per audience secrets are set through the environment
    env::set_var("INTERNAL_JWT_SECRETS__AUTH", "the auth secret");
    let config = lib::config::load().unwrap();
    assert_eq!(
        config.internal_jwt_secrets.get("auth").map(String::as_str),
        Some("the auth secret")
    );
    env::remove_var("INTERNAL_JWT_SECRETS__AUTH");

    env::set_var(
        "CONFIG_PATH",
        concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"),
    );
    assert!(lib::config::load().is_err());
}
