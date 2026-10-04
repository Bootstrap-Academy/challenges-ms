//! Probe the same telemetry policy used by the API and the coding worker.
#[path = "../src/telemetry.rs"]
mod telemetry;

fn main() {
    let dsn = std::env::args().nth(1).unwrap();
    let _guard = sentry::init((dsn.as_str(), telemetry::options()));
    telemetry::init_tracing();
    tracing::debug!("DEBUG-PRIVATE-CODE");
    tracing::info!(
        source_code = "PRIVATE-MAIL-TEXT",
        password = "SYNTHETIC-SMTP-PASSWORD",
        confirmation_code = "ABCD-EFGH-IJKL-MNOP",
        request_id = "owned-request",
        "safe-info-control"
    );
    tracing::error!(
        token = "PRIVATE-EVENT-TOKEN",
        request_id = "owned-request",
        "safe-error-control"
    );
    sentry::capture_event(sentry::protocol::Event {
        level: sentry::Level::Debug,
        message: Some("DIRECT-DEBUG-SECRET".into()),
        ..Default::default()
    });
    // The older SDK in this service inverts its flush result. Our receiver
    // checks actual delivery instead of trusting that return value.
    let _ = sentry::Hub::current()
        .client()
        .unwrap()
        .flush(Some(std::time::Duration::from_secs(10)));
}
