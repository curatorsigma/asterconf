use std::sync::Arc;

use askama::Template;
use axum::{
    extract::Path,
    http::StatusCode,
    response::{Html, IntoResponse},
    routing::{get, post},
    Router,
};
use tracing::{event, Level};

use crate::types::Config;

/// Run the web server (frontend)
pub async fn run_web_server(config: Arc<Config>) -> Result<(), Box<dyn std::error::Error>> {
    // build our application with a route
    let app = Router::new()
        .route("/scripts/htmx@1.9.12.js", get(htmx_script))
        .fallback(fallback);

    // run it
    let listener = tokio::net::TcpListener::bind(&config.web_bind_string)
        .await
        .unwrap();
    event!(
        Level::INFO,
        "Webserver started listening on {}",
        &config.web_bind_string
    );

    // TODO: add TLS support for web server
    axum::serve(listener, app).await.unwrap();
    Ok(())
}

async fn htmx_script() -> &'static str {
    include_str!("scripts/htmx@1.9.12.js")
}

async fn fallback() -> impl IntoResponse {
    (
        StatusCode::NOT_FOUND,
        Html(include_str!("templates/404.html")),
    )
}
