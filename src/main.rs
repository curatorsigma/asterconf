use std::sync::Arc;

use tracing::level_filters::LevelFilter;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::{filter, fmt::format::FmtSpan};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

mod agi_server;
mod db;
pub mod types;
mod web_server;

mod tests;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
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
                .with_filter(filter::LevelFilter::DEBUG),
        )
        .with(fmt::Layer::default().with_writer(writer));
    tracing::subscriber::set_global_default(subscriber).unwrap();

    let config = types::Config::create().await?;
    let config_capsule = Arc::new(config);

    sqlx::migrate!().run(&config_capsule.pool).await?;

    // start the agi server
    let config_for_agi = config_capsule.clone();
    let agi_handle = tokio::spawn(async move {
        agi_server::run_agi_server(config_for_agi).await.unwrap();
    });

    // start the web server
    let config_for_web = config_capsule.clone();
    let web_handle = tokio::spawn(async move {
        web_server::run_web_server(config_for_web).await.unwrap();
    });

    let res = tokio::join!(agi_handle, web_handle);
    res.0?;
    res.1?;

    Ok(())
}
