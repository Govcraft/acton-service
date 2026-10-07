//! Exercise normal config loading in child processes to isolate cwd/environment.
use std::process::Command;

#[test]
fn malformed_revocation_configuration_never_becomes_anonymous_defaults() {
    for config in [
        Some("[revocation]\nbackend='postgress'\nnamespace='test'\n"),
        Some("[revocation]\nbackend='postgres'\nnamespace='test'\nmisspelled=true\n"),
        Some("[revocation]\nbackend='postgres'\n"),
        None,
    ] {
        let directory = tempfile::tempdir().unwrap();
        if let Some(config) = config {
            std::fs::write(directory.path().join("config.toml"), config).unwrap();
        }
        let output = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "normal_loading_child", "--nocapture"])
            .current_dir(directory.path())
            .env("REVOCATION_CONFIG_CHILD", "1")
            .env(
                "CONFIG_FAILURE_EXPECTED",
                if config.is_some() { "yes" } else { "no" },
            )
            .env("XDG_CONFIG_HOME", directory.path())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "config={config:?}\n{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn normal_loading_child() {
    if std::env::var_os("REVOCATION_CONFIG_CHILD").is_none() {
        return;
    }
    let expected = std::env::var("CONFIG_FAILURE_EXPECTED").unwrap() == "yes";
    let result = acton_service::prelude::ServiceBuilder::<()>::new().try_build();
    assert_eq!(
        result.is_err(),
        expected,
        "try_build must preserve config-load failures"
    );
    if expected {
        assert!(
            acton_service::prelude::ServiceBuilder::<()>::new()
                .build()
                .bind()
                .await
                .is_err(),
            "infallible build must refuse to bind after malformed config"
        );
    }
}
