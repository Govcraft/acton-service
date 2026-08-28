//! SP-initiated SAML round trip against an in-process identity provider.
//!
//! `saml-rs` ships an IdP role, so the whole exchange runs without a network:
//! the SP under test issues a signed `AuthnRequest`, the IdP consumes it and
//! signs a `Response`, and the SP validates that at its ACS. The negative
//! cases each corrupt exactly one property the acceptance criteria name
//! (signature, validity window, audience, replay) and assert the SP rejects
//! the response for that reason and no other.

#![cfg(feature = "saml")]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use acton_service::auth::saml::{
    InMemorySamlStore, SamlAuthError, SamlConfig, SamlLoginDelivery, SamlRejection,
    SamlServiceProvider,
};
use saml_rs::binding::{base64_decode, base64_encode};
use saml_rs::{
    AuthnRequest, BrowserInput, CertificatePem, Credentials, EntityId, Idp, IdpConfig,
    IdpValidationPolicy, MetadataTrustPolicy, NameId, NameIdFormat, PrivateKeyPem, ReplayPolicy,
    RespondSso, Saml, SamlValidationContext, SpDescriptor, SsoEndpoint, Subject,
    XmlEncryptionPolicy, XmlPolicy,
};

const SP_ENTITY_ID: &str = "https://sp.example.test/saml/metadata";
const ACS_URL: &str = "https://sp.example.test/saml/acs";
const IDP_ENTITY_ID: &str = "https://idp.example.test/metadata";
const IDP_SSO_URL: &str = "https://idp.example.test/sso";

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/saml")
        .join(name)
}

fn pem(name: &str) -> String {
    std::fs::read_to_string(fixture(name)).expect("fixture exists")
}

/// An identity provider whose metadata has been written to disk for the SP.
struct TestIdp {
    idp: Saml<Idp>,
    metadata_file: tempfile::NamedTempFile,
}

impl TestIdp {
    fn new(issuance_lifetime: Duration) -> Self {
        Self::build(issuance_lifetime, false)
    }

    /// An identity provider that encrypts assertions to the SP's certificate.
    fn encrypting() -> Self {
        Self::build(Duration::from_secs(300), true)
    }

    fn build(issuance_lifetime: Duration, encrypt_assertions: bool) -> Self {
        let xml = if encrypt_assertions {
            XmlPolicy {
                encryption: XmlEncryptionPolicy::encrypt_assertions(),
                ..XmlPolicy::default()
            }
        } else {
            XmlPolicy::default()
        };
        let config = IdpConfig::builder(EntityId::try_new(IDP_ENTITY_ID).unwrap())
            .sso_endpoint(SsoEndpoint::redirect(IDP_SSO_URL).unwrap())
            .credentials(Credentials {
                signing_key: Some(PrivateKeyPem::new(pem("idp.key.pem"))),
                signing_certificate: Some(CertificatePem::new(pem("idp.cert.pem"))),
                ..Credentials::default()
            })
            .validation(IdpValidationPolicy::strict())
            .issuance_lifetime(issuance_lifetime)
            .xml(xml)
            .build()
            .expect("idp config");
        let idp = Saml::idp(config).expect("idp");
        let metadata_file = tempfile::NamedTempFile::new().expect("tempfile");
        std::fs::write(metadata_file.path(), idp.metadata_xml()).expect("write metadata");
        Self { idp, metadata_file }
    }

    /// Act on the SP's redirect and produce the form the browser would post
    /// to the ACS.
    fn respond(
        &self,
        sp: &SamlServiceProvider,
        redirect_url: &str,
        name_id: &str,
    ) -> Vec<(String, String)> {
        let descriptor = SpDescriptor::from_metadata_xml(
            sp.metadata_xml(),
            MetadataTrustPolicy::UnsignedForCompatibility,
        )
        .expect("sp metadata");
        let query = redirect_url
            .split_once('?')
            .map(|(_, q)| q)
            .expect("query string");
        let received = self
            .idp
            .receive_sso(
                &descriptor,
                BrowserInput::<AuthnRequest>::redirect(query),
                SamlValidationContext::new(
                    std::time::SystemTime::now(),
                    ReplayPolicy::DisabledForCompatibility,
                ),
            )
            .expect("idp accepts the request");
        let subject = Subject::new(
            NameId::new(name_id, Some(NameIdFormat::EmailAddress)),
            vec![],
        );
        let outbound = self
            .idp
            .respond_sso(&descriptor, &received, subject, RespondSso::post())
            .expect("idp responds");
        outbound
            .post_form()
            .expect("post form")
            .fields()
            .iter()
            .map(|field| (field.name().to_owned(), field.value().to_owned()))
            .collect()
    }
}

fn sp_config(idp: &TestIdp, entity_id: &str, clock_skew_secs: u64) -> SamlConfig {
    sp_config_with(idp, entity_id, clock_skew_secs, "")
}

/// SP configured to decrypt assertions with its own key. The software-RSA
/// opt-in only matters on `crypto-ring` builds and is inert elsewhere.
fn decrypting_sp_config(idp: &TestIdp) -> SamlConfig {
    sp_config_with(
        idp,
        SP_ENTITY_ID,
        60,
        &format!(
            "decryption_key_path = \"{}\"\nallow_software_rsa_decryption = true",
            fixture("sp.key.pem").display()
        ),
    )
}

fn sp_config_with(idp: &TestIdp, entity_id: &str, clock_skew_secs: u64, extra: &str) -> SamlConfig {
    toml::from_str(&format!(
        r#"
        entity_id = "{entity_id}"
        acs_url = "{ACS_URL}"
        signing_key_path = "{key}"
        certificate_path = "{cert}"
        name_id_format = "email-address"
        clock_skew_secs = {clock_skew_secs}
        {extra}
        [idp]
        entity_id = "{IDP_ENTITY_ID}"
        metadata_path = "{metadata}"
        [attributes]
        default_roles = ["employee"]
        "#,
        key = fixture("sp.key.pem").display(),
        cert = fixture("sp.cert.pem").display(),
        metadata = idp.metadata_file.path().display(),
    ))
    .expect("config parses")
}

async fn redirect_url(sp: &SamlServiceProvider, relay_state: Option<&str>) -> String {
    let request = sp.begin_login(relay_state).await.expect("login starts");
    match request.delivery {
        SamlLoginDelivery::Redirect { url } => url,
        SamlLoginDelivery::PostForm { .. } => panic!("expected redirect binding"),
    }
}

fn rejection(error: SamlAuthError) -> SamlRejection {
    match error {
        SamlAuthError::Rejected { reason, .. } => reason,
        other => panic!("expected a rejection, got {other}"),
    }
}

fn response_xml(fields: &[(String, String)]) -> String {
    let encoded = fields
        .iter()
        .find(|(name, _)| name == "SAMLResponse")
        .map(|(_, value)| value.as_str())
        .expect("SAMLResponse");
    String::from_utf8(base64_decode(encoded).expect("base64")).expect("utf8")
}

fn with_response_xml(mut fields: Vec<(String, String)>, xml: &str) -> Vec<(String, String)> {
    for (name, value) in &mut fields {
        if name == "SAMLResponse" {
            *value = base64_encode(xml.as_bytes());
        }
    }
    fields
}

#[tokio::test]
async fn signed_response_round_trips_into_claims() {
    let idp = TestIdp::new(Duration::from_secs(300));
    let sp =
        SamlServiceProvider::from_config_in_memory(&sp_config(&idp, SP_ENTITY_ID, 60)).expect("sp");

    assert!(sp.metadata_xml().contains(SP_ENTITY_ID));
    assert!(sp.metadata_xml().contains("AuthnRequestsSigned=\"true\""));
    assert_eq!(sp.idp_entity_id(), IDP_ENTITY_ID);

    let url = redirect_url(&sp, Some("/dashboard")).await;
    assert!(url.starts_with(IDP_SSO_URL));
    assert!(url.contains("SAMLRequest="));
    assert!(url.contains("Signature="));

    let fields = idp.respond(&sp, &url, "alice@example.test");
    let login = sp.finish_login(fields).await.expect("login completes");

    assert_eq!(login.name_id, "alice@example.test");
    assert_eq!(login.issuer, IDP_ENTITY_ID);
    assert_eq!(login.relay_state.as_deref(), Some("/dashboard"));
    assert_eq!(login.claims.sub, "saml:alice@example.test");
    assert_eq!(login.claims.email.as_deref(), Some("alice@example.test"));
    assert_eq!(login.claims.roles, ["employee"]);
    assert_eq!(login.claims.iss.as_deref(), Some(IDP_ENTITY_ID));
    assert!(login.claims.exp > chrono::Utc::now().timestamp());
    assert_eq!(
        login
            .claims
            .custom_claim("authentication_method")
            .and_then(serde_json::Value::as_str),
        Some("saml")
    );
}

#[tokio::test]
async fn encrypted_assertion_is_decrypted_and_validated() {
    let idp = TestIdp::encrypting();
    let sp = SamlServiceProvider::from_config_in_memory(&decrypting_sp_config(&idp)).expect("sp");
    assert!(sp.metadata_xml().contains("use=\"encryption\""));

    let url = redirect_url(&sp, None).await;
    let fields = idp.respond(&sp, &url, "alice@example.test");
    let xml = response_xml(&fields);
    assert!(
        xml.contains("EncryptedAssertion"),
        "IdP must have encrypted the assertion"
    );
    assert!(
        !xml.contains("alice@example.test"),
        "NameID must not travel in the clear"
    );

    let login = sp
        .finish_login(fields)
        .await
        .expect("encrypted login completes");
    assert_eq!(login.name_id, "alice@example.test");
    assert_eq!(login.claims.sub, "saml:alice@example.test");
}

#[tokio::test]
async fn tampered_response_fails_signature_verification() {
    let idp = TestIdp::new(Duration::from_secs(300));
    let sp =
        SamlServiceProvider::from_config_in_memory(&sp_config(&idp, SP_ENTITY_ID, 60)).expect("sp");

    let url = redirect_url(&sp, None).await;
    let fields = idp.respond(&sp, &url, "alice@example.test");
    let xml = response_xml(&fields);
    assert!(xml.contains("alice@example.test"));
    let forged = with_response_xml(
        fields,
        &xml.replace("alice@example.test", "mallory@example.test"),
    );

    let error = sp
        .finish_login(forged)
        .await
        .expect_err("forged response must fail");
    assert_eq!(rejection(error), SamlRejection::InvalidSignature);
}

#[tokio::test]
async fn expired_assertion_is_rejected() {
    // A one-second issuance window and no clock skew: by the time the
    // response reaches the ACS its Conditions and bearer window have passed.
    let idp = TestIdp::new(Duration::from_secs(1));
    let sp =
        SamlServiceProvider::from_config_in_memory(&sp_config(&idp, SP_ENTITY_ID, 0)).expect("sp");

    let url = redirect_url(&sp, None).await;
    let fields = idp.respond(&sp, &url, "alice@example.test");
    tokio::time::sleep(Duration::from_millis(2100)).await;

    let error = sp
        .finish_login(fields)
        .await
        .expect_err("expired response must fail");
    assert_eq!(rejection(error), SamlRejection::Expired);
}

#[tokio::test]
async fn assertion_for_another_audience_is_rejected() {
    // Two SPs share one pending store, so the second can look up a login the
    // first started. The IdP addressed the assertion to the first SP's
    // entityID; the second must refuse it on AudienceRestriction alone.
    let idp = TestIdp::new(Duration::from_secs(300));
    let store = Arc::new(InMemorySamlStore::new());
    let intended = SamlServiceProvider::from_config(
        &sp_config(&idp, SP_ENTITY_ID, 60),
        store.clone(),
        store.clone(),
    )
    .expect("intended sp");
    let other = SamlServiceProvider::from_config(
        &sp_config(&idp, "https://other.example.test/saml/metadata", 60),
        store.clone(),
        store,
    )
    .expect("other sp");

    let url = redirect_url(&intended, None).await;
    let fields = idp.respond(&intended, &url, "alice@example.test");

    let error = other
        .finish_login(fields)
        .await
        .expect_err("wrong audience must fail");
    assert_eq!(rejection(error), SamlRejection::AudienceMismatch);
}

#[tokio::test]
async fn replayed_response_is_rejected() {
    use acton_service::auth::saml::SamlPendingStore;

    let idp = TestIdp::new(Duration::from_secs(300));
    let store = Arc::new(InMemorySamlStore::new());
    let sp = SamlServiceProvider::from_config(
        &sp_config(&idp, SP_ENTITY_ID, 60),
        store.clone(),
        store.clone(),
    )
    .expect("sp");

    let url = redirect_url(&sp, None).await;
    let fields = idp.respond(&sp, &url, "alice@example.test");
    let request_id = {
        let xml = response_xml(&fields);
        let start = xml.find("InResponseTo=\"").unwrap() + "InResponseTo=\"".len();
        xml[start..start + xml[start..].find('"').unwrap()].to_owned()
    };

    // Capture the pending login before the first (successful) consumption so
    // the replay reaches the assertion checks rather than failing on a
    // missing request.
    let pending = store
        .take(&request_id)
        .await
        .unwrap()
        .expect("pending login exists");
    store.put(&pending, Duration::from_secs(60)).await.unwrap();

    sp.finish_login(fields.clone())
        .await
        .expect("first presentation succeeds");

    // Presenting it a second time with no pending login is refused outright.
    let error = sp
        .finish_login(fields.clone())
        .await
        .expect_err("consumed request");
    assert!(matches!(error, SamlAuthError::UnknownRequest(_)));

    // Even if the pending login were somehow reinstated, the assertion and
    // response identifiers are already in the replay store.
    store.put(&pending, Duration::from_secs(60)).await.unwrap();
    let error = sp.finish_login(fields).await.expect_err("replay must fail");
    assert_eq!(rejection(error), SamlRejection::Replayed);
}

#[tokio::test]
async fn unsolicited_response_is_rejected() {
    let idp = TestIdp::new(Duration::from_secs(300));
    let sp =
        SamlServiceProvider::from_config_in_memory(&sp_config(&idp, SP_ENTITY_ID, 60)).expect("sp");

    let error = sp
        .finish_login(vec![(
            "SAMLResponse".to_owned(),
            base64_encode(b"<Response ID=\"_x\"/>"),
        )])
        .await
        .expect_err("unsolicited response must fail");
    assert_eq!(rejection(error), SamlRejection::RequestMismatch);
}

#[test]
fn metadata_with_the_wrong_entity_id_is_refused_at_startup() {
    let idp = TestIdp::new(Duration::from_secs(300));
    let mut config = sp_config(&idp, SP_ENTITY_ID, 60);
    config.idp.entity_id = "https://impostor.example.test/metadata".to_owned();
    let error =
        SamlServiceProvider::from_config_in_memory(&config).expect_err("entity id mismatch");
    assert!(error.to_string().contains("idp.metadata_path"), "{error}");
}

/// Sanity check the fixtures are what the tests assume: an RSA pair per role.
#[test]
fn fixtures_are_rsa_pems() {
    for name in ["sp.key.pem", "idp.key.pem"] {
        assert!(
            pem(name).starts_with("-----BEGIN PRIVATE KEY-----"),
            "{name}"
        );
    }
    for name in ["sp.cert.pem", "idp.cert.pem"] {
        assert!(
            pem(name).starts_with("-----BEGIN CERTIFICATE-----"),
            "{name}"
        );
    }
}
