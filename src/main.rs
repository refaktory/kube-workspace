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

    let config = match config::Config::load_from_env() {
        Ok(c) => c,
        Err(err) => {
            tracing::error!(error=?err, "Could not load config");
            std::process::exit(1);
        }
    };

    dbg!(&config);

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()
        .expect("Could not create runtime");

    let res = rt.block_on(async move {
        let op = operator::Operator::launch(config.clone()).await?;
        server::run_server(op).await;
        Result::<_, AnyError>::Ok(())
    });

    if let Err(err) = res {
        tracing::error!(error=?err, "operator failed");
    } else {
        tracing::info!("Orderly shutdown");
    }
}
