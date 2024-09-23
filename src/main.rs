use std::panic;
use std::sync::Arc;

use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::{filter, fmt::format::FmtSpan};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

mod agi_server;
mod db;
pub(crate) mod ldap;
pub mod types;
mod web_server;

mod tests;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("Failed to install rustls crypto provider");

    let file_appender = RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix("asterconf.log")
        .build("/var/log/asterconf")
        .expect("Should be able to create file appender");
    let (writer, _guard) = tracing_appender::non_blocking(file_appender);

    let my_crate_filter = EnvFilter::new("asterconf,blazing_agi");
    let subscriber = tracing_subscriber::registry()
        .with(my_crate_filter)
        .with(
            tracing_subscriber::fmt::layer()
                .compact()
                .with_span_events(FmtSpan::NEW | FmtSpan::CLOSE)
                .with_line_number(true)
                .with_filter(filter::LevelFilter::TRACE),
        )
        .with(fmt::Layer::default().with_writer(writer));
    if let Err(e) = tracing::subscriber::set_global_default(subscriber) {
        eprintln!("Error setting global tracing subscriber: {e}");
        Err(e)?;
    };

    let config = types::Config::create().await?;
    let config_capsule = Arc::new(config);

    sqlx::migrate!().run(&config_capsule.pool).await?;

    // start the agi server
    let config_for_agi = config_capsule.clone();
    let agi_handle = tokio::spawn(async move {
        if let Err(e) = agi_server::run_agi_server(config_for_agi).await {
            eprintln!("Could not start the AGI server: {e}");
            panic!("Unable to start AGI server. Unrecoverable");
        };
    });

    // start the web server
    let config_for_web = config_capsule.clone();
    let webserver = web_server::Webserver::new().await?;
    let web_handle = tokio::spawn(async move {
        if let Err(e) = webserver.run_web_server(config_for_web).await {
            eprintln!("Could not start the web server: {e}");
            panic!("Unable to start web server. Unrecoverable");
        };
    });

    let res = tokio::join!(agi_handle, web_handle);
    res.0?;
    res.1?;

    Ok(())
}
