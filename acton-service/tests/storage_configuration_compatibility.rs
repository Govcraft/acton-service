use acton_service::config::ClickHouseConfig;

#[test]
fn clickhouse_configuration_is_available_without_the_driver_feature() {
    let config: ClickHouseConfig = serde_json::from_value(serde_json::json!({
        "url": "http://localhost:8123"
    }))
    .unwrap();
    assert_eq!(config.database, "default");
    assert_eq!(config.max_retries, 5);
    assert!(config.lazy_init);
    assert!(!config.optional);
}
