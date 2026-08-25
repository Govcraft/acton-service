#![cfg(all(feature = "mssql", feature = "accounts", feature = "auth", feature = "audit"))]

use acton_service::{accounts::{storage::{mssql::MssqlAccountStorage,AccountStorage},types::{Account,AccountId,AccountStatus}},audit::storage::mssql::MssqlAuditStorage,auth::{ApiKey,ApiKeyStorage,KeyRotationStorage,MssqlApiKeyStorage,MssqlKeyRotationStorage,MssqlRefreshStorage,RefreshTokenMetadata,RefreshTokenStorage},config::DatabaseConfig,mssql};
use chrono::{Duration,Utc};
use testcontainers_modules::{mssql_server::MssqlServer,testcontainers::runners::AsyncRunner};

#[tokio::test]
async fn mssql_backends_initialize_and_accounts_round_trip(){
 let container=MssqlServer::default().with_accept_eula().start().await.expect("start SQL Server container");
 let host=container.get_host().await.expect("container host");
 let port=container.get_host_port_ipv4(1433).await.expect("container port");
    let config=DatabaseConfig{url:format!("Server=tcp:{host},{port};Database=master;User Id=sa;Password={};TrustServerCertificate=True;",MssqlServer::DEFAULT_SA_PASSWORD),max_connections:5,min_connections:1,connection_timeout_secs:30,max_retries:10,retry_delay_secs:2,optional:false,lazy_init:false,mssql_auth:acton_service::config::MssqlAuthMode::ConnectionString};
 let pool=mssql::create_pool(&config).await.expect("SQL Server pool");
 mssql::health_check(&pool).await.expect("health query");
 let accounts=MssqlAccountStorage::new(pool.clone()).await.expect("accounts schema");
 let api_keys=MssqlApiKeyStorage::new(pool.clone(),"test").await.expect("api key schema");
 let refresh=MssqlRefreshStorage::new(pool.clone()).await.expect("refresh schema");
 MssqlKeyRotationStorage::new(pool.clone()).initialize().await.expect("key rotation schema");
 MssqlAuditStorage::new(pool.clone()).initialize().await.expect("audit schema");
 let now=Utc::now();
 let account:Account=serde_json::from_value(serde_json::json!({"id":AccountId::new().to_string(),"email":format!("{}@example.test",uuid::Uuid::new_v4()),"username":null,"password_hash":null,"status":AccountStatus::Active,"roles":["user"],"email_verified":true,"email_verified_at":now,"last_login_at":null,"locked_at":null,"locked_reason":null,"disabled_at":null,"disabled_reason":null,"expires_at":null,"password_changed_at":null,"failed_login_count":0,"metadata":{"backend":"mssql"},"created_at":now,"updated_at":now})).expect("account fixture");
 accounts.create(&account).await.expect("create account");
 let loaded=accounts.get_by_id(account.id.as_str()).await.expect("load account").expect("account exists");
 assert_eq!(loaded.email,account.email);
 assert_eq!(loaded.roles,account.roles);
 let key=ApiKey{id:uuid::Uuid::new_v4().to_string(),user_id:account.id.to_string(),name:"integration".to_string(),prefix:format!("test_{}",uuid::Uuid::new_v4()),key_hash:"not-used-for-id-lookup".to_string(),scopes:vec!["read".to_string()],rate_limit:Some(100),is_revoked:false,last_used_at:None,expires_at:None,created_at:now};
 api_keys.create(&key).await.expect("create API key");
 assert_eq!(api_keys.get_by_id(&key.id).await.expect("load API key").expect("API key exists").scopes,key.scopes);
 let token_id=uuid::Uuid::new_v4().to_string();
 let metadata=RefreshTokenMetadata::default();
 refresh.store(&token_id,account.id.as_str(),"family",now+Duration::hours(1),&metadata).await.expect("store refresh token");
 assert_eq!(refresh.get(&token_id).await.expect("load refresh token").expect("refresh token exists").user_id,account.id.as_str());
 assert!(accounts.delete(account.id.as_str()).await.expect("delete account"));
}
