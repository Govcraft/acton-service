use acton_service::middleware::{Claims, TokenRevocation};

pub async fn contract(storage: &dyn TokenRevocation, other: &dyn TokenRevocation) {
    storage.initialize().await.expect("initialize revocation");
    storage
        .initialize()
        .await
        .expect("repeat revocation migration");
    let mut claims: Claims = serde_json::from_value(serde_json::json!({
        "sub":"user:revocation", "exp":chrono::Utc::now().timestamp()+3600,
        "iat":100, "jti":"revocation-token"
    }))
    .expect("claims fixture");
    assert!(!storage
        .check_claims(&claims)
        .await
        .expect("initially valid"));
    storage
        .revoke("revocation-token", 3600)
        .await
        .expect("revoke token");
    storage
        .revoke("revocation-token", 0)
        .await
        .expect("do not shorten expiry");
    assert!(storage.check_claims(&claims).await.expect("token denied"));
    assert!(!other
        .is_revoked("revocation-token")
        .await
        .expect("namespace isolation"));
    storage
        .revoke("exact-token", 3600)
        .await
        .expect("exact token identifier");
    assert!(!storage
        .is_revoked("exact-token ")
        .await
        .expect("trailing token space differs"));
    storage
        .revoke_subject("exact-subject", 100)
        .await
        .expect("exact subject");
    assert_eq!(
        storage
            .subject_not_before("exact-subject ")
            .await
            .expect("trailing subject space differs"),
        None
    );
    storage
        .revoke("already-expired", 0)
        .await
        .expect("expired record");
    assert!(!storage
        .is_revoked("already-expired")
        .await
        .expect("expired revocation ignored"));
    storage
        .revoke_subject(&claims.sub, 100)
        .await
        .expect("revoke subject");
    storage
        .revoke_subject(&claims.sub, 99)
        .await
        .expect("cutoff cannot decrease");
    assert_eq!(
        storage
            .subject_not_before(&claims.sub)
            .await
            .expect("cutoff"),
        Some(100)
    );
    claims.jti = None;
    assert!(storage
        .check_claims(&claims)
        .await
        .expect("same-second subject denial without jti"));
    claims.iat = Some(101);
    assert!(!storage
        .check_claims(&claims)
        .await
        .expect("newer token allowed"));
    claims.iat = None;
    assert!(storage
        .check_claims(&claims)
        .await
        .expect("missing iat fails closed"));
    storage
        .cleanup_expired()
        .await
        .expect("cleanup expired token records");
    assert_eq!(
        storage
            .subject_not_before(&claims.sub)
            .await
            .expect("persistent cutoff"),
        Some(100)
    );
}
