use std::{str::FromStr, sync::Arc};

use axum::{
    extract::Host, http::{StatusCode, Uri}, response::{Html, IntoResponse, Redirect}, routing::get, Router, handler::HandlerWithoutStateExt
};
use tracing::{event, warn, Level};

use crate::types::Config;

/// Run the web server (frontend)
pub async fn run_web_server(config: Arc<Config>) -> Result<(), Box<dyn std::error::Error>> {
    // build our application with a route
    let app = Router::new()
        .route("/scripts/htmx@1.9.12.js", get(htmx_script))
        .fallback(fallback);

    // run it
    let addr = std::net::SocketAddr::from_str(&config.web_bind_string_tls).expect("Should be able to parse socket addr");
    event!(
        Level::INFO,
        "Webserver (HTTPS) listening on {}",
        addr
    );

    // run the redirect service
    tokio::spawn(redirect_http_to_https(config.clone()));

    axum_server::bind_rustls(addr, config.rustls_config.clone())
        .serve(app.into_make_service())
        .await.expect("Should be able to start service");

    Ok(())
}

async fn redirect_http_to_https(config: Arc<Config>) {
    // todo: good error
    fn make_https(host: String, uri: Uri, http_port: u16, https_port: u16) -> Result<Uri, Box<dyn std::error::Error>> {
        let mut parts = uri.into_parts();

        parts.scheme = Some(axum::http::uri::Scheme::HTTPS);

        if parts.path_and_query.is_none() {
            parts.path_and_query = Some("/".parse().unwrap());
        }

        let https_host = host.replace(&http_port.to_string(), &https_port.to_string());
        parts.authority = Some(https_host.parse()?);

        Ok(Uri::from_parts(parts)?)
    }

    let redir_web_bind_port = config.web_bind_port;
    let redir_web_bind_port_tls = config.web_bind_port_tls;
    let redirect = move |Host(host): Host, uri: Uri| async move {
        match make_https(host, uri, redir_web_bind_port, redir_web_bind_port_tls) {
            Ok(uri) => Ok(Redirect::permanent(&uri.to_string())),
            Err(error) => {
                tracing::warn!(%error, "failed to convert URI to HTTPS");
                Err(StatusCode::BAD_REQUEST)
            }
        }
    };

    let listener = tokio::net::TcpListener::bind(config.web_bind_string.clone()).await.unwrap();
    tracing::info!("Webserver (HTTP) listening on {}", listener.local_addr().unwrap());
    axum::serve(listener, redirect.into_make_service())
        .await
        .unwrap();
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
