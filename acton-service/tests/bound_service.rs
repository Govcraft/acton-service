//! `ActonService::bind` binds without serving, `BoundService::serve` serves
//! exactly those sockets, and `ServiceBuilder::with_shutdown` stops it.
//!
//! Every test runs the real serve path on loopback and talks to it with raw
//! HTTP/1.1 bytes, so what is asserted is what a client on the wire sees.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;

use acton_service::config::Config;
use acton_service::prelude::ServiceBuilder;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;

const LOOPBACK: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);

/// Bounds every wait; a loopback service answers or stops in milliseconds.
const SETTLE: Duration = Duration::from_secs(5);

/// A service configuration on loopback with an OS-assigned port.
fn config(name: &str) -> Config<()> {
    let mut config = Config::<()>::default();
    config.service.name = name.to_string();
    config.service.bind = LOOPBACK;
    config.service.port = 0;
    config
}

/// A shutdown future and the sender that resolves it.
fn shutdown() -> (
    oneshot::Sender<()>,
    impl std::future::Future<Output = ()> + Send + 'static,
) {
    let (tx, rx) = oneshot::channel::<()>();
    (tx, async move {
        let _ = rx.await;
    })
}

/// One HTTP/1.1 GET, the response returned verbatim.
async fn get(addr: SocketAddr, path: &str) -> String {
    let mut stream = TcpStream::connect(addr).await.expect("connect");
    let request = format!("GET {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n");
    stream
        .write_all(request.as_bytes())
        .await
        .expect("write request");
    let mut response = Vec::new();
    tokio::time::timeout(SETTLE, stream.read_to_end(&mut response))
        .await
        .expect("the service answers within the bound")
        .expect("read response");
    String::from_utf8_lossy(&response).into_owned()
}

/// Whether a new connection to `addr` is refused, polled because a closing
/// socket is released asynchronously.
async fn refused(addr: SocketAddr) -> bool {
    for _ in 0..200 {
        if TcpStream::connect(addr).await.is_err() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    false
}

// `build()` refuses on a current-thread runtime.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn port_zero_binds_a_real_port_that_serve_then_answers_on() {
    let (stop, on_stop) = shutdown();
    let bound = ServiceBuilder::new()
        .with_config(config("bound-port-zero"))
        .with_shutdown(on_stop)
        .build()
        .bind()
        .await
        .expect("binds an ephemeral loopback port");
    let addr = bound.local_addr();
    assert_eq!(addr.ip(), LOOPBACK);
    assert_ne!(
        addr.port(),
        0,
        "local_addr reports the port the OS assigned"
    );

    let server = tokio::spawn(bound.serve());
    let response = get(addr, "/health").await;
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");

    stop.send(())
        .expect("serve is waiting on the shutdown future");
    tokio::time::timeout(SETTLE, server)
        .await
        .expect("serve returns once shutdown is requested")
        .expect("the serve task does not panic")
        .expect("a requested shutdown is a clean exit");
    assert!(refused(addr).await, "a stopped service closes its listener");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dropping_a_bound_service_closes_its_socket() {
    let bound = ServiceBuilder::new()
        .with_config(config("bound-drop"))
        .build()
        .bind()
        .await
        .expect("binds");
    let addr = bound.local_addr();
    assert!(
        TcpStream::connect(addr).await.is_ok(),
        "a bound socket queues connections before serve"
    );

    drop(bound);

    assert!(
        refused(addr).await,
        "dropping the BoundService releases the port"
    );
    // Nothing kept the socket alive: the exact address binds again.
    TcpListener::bind(addr)
        .await
        .expect("the port is free once the BoundService is dropped");
}

/// The window this closes: a harness that learned the port and then decided
/// not to serve must not have a connection answered behind its back.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shutdown_requested_between_bind_and_serve_answers_nothing() {
    let (stop, on_stop) = shutdown();
    let bound = ServiceBuilder::new()
        .with_config(config("bound-early-stop"))
        .with_shutdown(on_stop)
        .build()
        .bind()
        .await
        .expect("binds");
    let addr = bound.local_addr();

    // Queued in the kernel backlog, with a complete request waiting.
    let mut early = TcpStream::connect(addr).await.expect("queued connect");
    early
        .write_all(format!("GET /health HTTP/1.1\r\nHost: {addr}\r\n\r\n").as_bytes())
        .await
        .expect("write the queued request");

    stop.send(())
        .expect("the shutdown future is held by the service");
    tokio::time::timeout(SETTLE, bound.serve())
        .await
        .expect("serve returns at once when shutdown already happened")
        .expect("an early shutdown is a clean exit");

    let mut answer = Vec::new();
    let read = tokio::time::timeout(SETTLE, early.read_to_end(&mut answer))
        .await
        .expect("the queued connection is closed, not left hanging");
    assert!(
        read.is_err() || answer.is_empty(),
        "no response was served: {}",
        String::from_utf8_lossy(&answer)
    );
    assert!(refused(addr).await, "the listener is closed");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn serve_without_bind_still_stops_on_the_shutdown_future() {
    let listener = TcpListener::bind(SocketAddr::new(LOOPBACK, 0))
        .await
        .expect("the caller binds");
    let addr = listener.local_addr().expect("addr");
    let (stop, on_stop) = shutdown();
    let server = tokio::spawn(
        ServiceBuilder::new()
            .with_config(config("serve-with-shutdown"))
            .with_listener(listener)
            .with_shutdown(on_stop)
            .build()
            .serve(),
    );

    let response = get(addr, "/health").await;
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");

    stop.send(()).expect("serve is waiting");
    tokio::time::timeout(SETTLE, server)
        .await
        .expect("serve returns")
        .expect("no panic")
        .expect("clean exit");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn with_listener_serves_on_the_callers_socket_not_the_configured_port() {
    let listener = TcpListener::bind(SocketAddr::new(LOOPBACK, 0))
        .await
        .expect("the caller binds");
    let addr = listener.local_addr().expect("addr");
    // A configured port that is certainly not the caller's.
    let taken = std::net::TcpListener::bind(SocketAddr::new(LOOPBACK, 0)).expect("reserve");
    let mut config = config("with-listener");
    config.service.port = taken.local_addr().expect("addr").port();

    let bound = ServiceBuilder::new()
        .with_config(config)
        .with_listener(listener)
        .build()
        .bind()
        .await
        .expect("binding uses the supplied socket, so the taken port does not matter");
    assert_eq!(bound.local_addr(), addr);
}

#[cfg(feature = "prometheus-metrics")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn with_metrics_listener_serves_the_scrape_on_the_callers_socket() {
    let metrics = TcpListener::bind(SocketAddr::new(LOOPBACK, 0))
        .await
        .expect("the caller binds the exporter socket");
    let metrics_addr = metrics.local_addr().expect("addr");
    let (stop, on_stop) = shutdown();

    let bound = ServiceBuilder::new()
        .with_config(config("with-metrics-listener"))
        .with_metrics_listener(metrics)
        .with_shutdown(on_stop)
        .build()
        .bind()
        .await
        .expect("binds");
    assert_eq!(bound.metrics_local_addr(), Some(metrics_addr));

    let server = tokio::spawn(bound.serve());
    let response = get(metrics_addr, "/metrics").await;
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");

    stop.send(()).expect("serve is waiting");
    tokio::time::timeout(SETTLE, server)
        .await
        .expect("serve returns")
        .expect("no panic")
        .expect("clean exit");
    assert!(
        refused(metrics_addr).await,
        "the exporter drains with the service"
    );
}

#[cfg(feature = "prometheus-metrics")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dropping_a_bound_service_closes_its_exporter_socket_too() {
    let metrics = TcpListener::bind(SocketAddr::new(LOOPBACK, 0))
        .await
        .expect("the caller binds the exporter socket");
    let metrics_addr = metrics.local_addr().expect("addr");
    let bound = ServiceBuilder::new()
        .with_config(config("bound-drop-exporter"))
        .with_metrics_listener(metrics)
        .build()
        .bind()
        .await
        .expect("binds");

    drop(bound);

    assert!(
        refused(metrics_addr).await,
        "no exporter task outlives the drop"
    );
}

#[cfg(feature = "grpc")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dual_port_grpc_reports_its_own_bound_port() {
    let mut config = config("bound-dual-port");
    config.grpc = Some(acton_service::config::GrpcConfig {
        enabled: true,
        use_separate_port: true,
        port: 0,
        ..Default::default()
    });
    let routes = tonic::service::Routes::default();

    let bound = ServiceBuilder::new()
        .with_config(config)
        .with_grpc_services(routes)
        .build()
        .bind()
        .await
        .expect("binds both listeners");
    let grpc = bound.grpc_local_addr().expect("dual-port mode binds gRPC");
    assert_ne!(grpc.port(), 0);
    assert_ne!(grpc, bound.local_addr());
}
