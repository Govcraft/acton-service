//! Basic tracing uses the same ownership policy as the full service path.

use acton_service::observability::{init_basic_tracing, init_tracing};

#[test]
fn basic_tracing_preserves_an_application_subscriber() {
    tracing::subscriber::set_global_default(tracing_subscriber::registry())
        .expect("application subscriber installs first");
    init_basic_tracing();
    init_tracing(&acton_service::config::Config::<()>::default())
        .expect("subsequent configured tracing is also harmless");
    tracing::dispatcher::get_default(|dispatch| {
        assert!(dispatch.is::<tracing_subscriber::Registry>());
    });
}
