//! # kube-workspace-operator
//!
//! Kubernetes operator that creates workspace pods for users.
//! Workspaces are started and stopped via API calls exposed at /api/query.

mod client;
mod config;
mod operator;
mod server;

pub(crate) type AnyError = anyhow::Error;

fn main() {
    // Set default logging level to info.
    if std::env::var("RUST_LOG").is_err() {
        std::env::set_var("RUST_LOG", "info");
    }
    // Initialize logging backend.
    tracing_subscriber::fmt::init();

    // Read config file.
    let config = match config::ConfigSource::load_from_env() {
        Ok(c) => c,
        Err(err) => {
            tracing::error!(error=?err, "Could not load config");
            std::process::exit(1);
        }
    };

    // Set up tokio runtime.
    // We only use a single threaded runtime to save resources and because
    // we don't need multiple threads, a single one is sufficient for the
    // expected workloads.
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()
        .expect("Could not create runtime");

    let res = rt.block_on(async move {
        // Launch the operator.
        let op = operator::Operator::launch(config.clone()).await?;
        // Run the webserver.
        server::run_server(op).await;
        Result::<_, AnyError>::Ok(())
    });

    if let Err(err) = res {
        tracing::error!(error=?err, "operator failed");
    } else {
        tracing::info!("Orderly shutdown");
    }
}
