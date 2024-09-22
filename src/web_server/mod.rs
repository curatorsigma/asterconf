use askama::Template;
use axum_login::{
    login_required,
    tower_sessions::{ExpiredDeletion, Expiry, SessionManagerLayer},
    AuthManagerLayerBuilder,
};
use sqlx::SqlitePool;
use time::Duration;
use tower_sessions::cookie::Key;
use tower_sessions_sqlx_store::SqliteStore;
use uuid::Uuid;

use std::{str::FromStr, sync::Arc};

use axum::{
    extract::Host,
    handler::HandlerWithoutStateExt,
    http::{header, HeaderMap, StatusCode, Uri},
    response::{Html, IntoResponse, Redirect},
    routing::get,
    Extension, Router,
};
use tracing::{event, Level};

use crate::{ldap::LDAPBackend, types::Config};
pub(crate) mod login;
mod protected;

#[derive(Template)]
#[template(path = "500.html")]
struct InternalServerErrorTemplate {
    error_uuid: Uuid,
}

/// App State that simply holds a user session store
pub struct Webserver {
    db: SqlitePool,
}
impl Webserver {
    pub async fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let connect_options = sqlx::sqlite::SqliteConnectOptions::new()
            .filename(".session_data.db")
            .create_if_missing(true);
        let db = SqlitePool::connect_with(connect_options).await?;
        sqlx::migrate!().run(&db).await?;

        Ok(Self { db })
    }

    /// Run the web server
    pub async fn run_web_server(
        &self,
        config: Arc<Config>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Session layer.
        //
        // This uses `tower-sessions` to establish a layer that will provide the session
        // as a request extension.
        let session_store = SqliteStore::new(self.db.clone());
        session_store.migrate().await?;

        tokio::task::spawn(
            session_store
                .clone()
                .continuously_delete_expired(tokio::time::Duration::from_secs(60)),
        );
        // Generate a cryptographic key to sign the session cookie.
        let key = Key::generate();

        let session_layer = SessionManagerLayer::new(session_store)
            .with_secure(false)
            .with_expiry(Expiry::OnInactivity(Duration::hours(12)))
            .with_signed(key);

        // Auth service.
        //
        // This combines the session layer with our backend to establish the auth
        // service which will provide the auth session as a request extension.
        let auth_backend = config.ldap_config.clone();
        let auth_layer = AuthManagerLayerBuilder::new(auth_backend, session_layer).build();

        let our_config = config.clone();
        let app = Router::new()
            .merge(protected::create_protected_router())
            .route_layer(login_required!(LDAPBackend, login_url = "/login"))
            .merge(login::create_login_router())
            .layer(auth_layer)
            .layer(Extension(our_config))
            .route("/scripts/htmx@2.0.2.js", get(htmx_script))
            .route(
                "/scripts/hyperscript.org@0.9.12.js",
                get(hyperscript_script),
            )
            .route(
                "/scripts/htmx@2.0.2_response_targets.js",
                get(htmx_script_response_targets),
            )
            .route("/style.css", get(css_style))
            .fallback(fallback);

        // run it
        let addr = std::net::SocketAddr::from_str(&config.web_bind_string_tls)
            .expect("Should be able to parse socket addr");
        event!(Level::INFO, "Webserver (HTTPS) listening on {}", addr);

        // run the redirect service HTTPS -> HTTP on its own port
        tokio::spawn(redirect_http_to_https(config.clone()));

        // serve the main app on HTTPS
        axum_server::bind_rustls(addr, config.rustls_config.clone())
            .serve(app.into_make_service())
            .await
            .expect("Should be able to start service");

        Ok(())
    }
}

fn make_https(
    host: String,
    uri: Uri,
    http_port: u16,
    https_port: u16,
) -> Result<Uri, Box<dyn std::error::Error>> {
    let mut parts = uri.into_parts();

    parts.scheme = Some(axum::http::uri::Scheme::HTTPS);

    if parts.path_and_query.is_none() {
        parts.path_and_query = Some("/".parse().expect("Path should be statically save."));
    }

    let https_host = host.replace(&http_port.to_string(), &https_port.to_string());
    parts.authority = Some(https_host.parse()?);

    Ok(Uri::from_parts(parts)?)
}

async fn redirect_http_to_https(config: Arc<Config>) {
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

    let listener = match tokio::net::TcpListener::bind(config.web_bind_string.clone()).await {
        Ok(x) => x,
        Err(e) => {
            tracing::error!("Could not bind a TcP socket for the http -> https redirect service: {e}");
            panic!("Unable to start http -> https server. Unrecoverable.");
        },
    };
    tracing::info!(
        "Webserver (HTTP) listening on {}",
        listener.local_addr().expect("Local address of bound http -> https should be readable.")
    );
    if let Err(e) = axum::serve(listener, redirect.into_make_service())
        .await {
        tracing::error!("Could not start the http -> https redirect server: {e}");
        panic!("Unable to start http -> https server. Unrecoverable.");
    };
}

async fn htmx_script() -> impl IntoResponse {
    let mut headers = HeaderMap::new();
    headers.insert(header::SERVER, "axum".parse().expect("static string"));
    headers.insert(
        header::CONTENT_TYPE,
        "text/javascript".parse().expect("static string"),
    );
    (
        headers,
        include_str!("../../templates/static/htmx@2.0.2.js"),
    )
}

async fn htmx_script_response_targets() -> impl IntoResponse {
    let mut headers = HeaderMap::new();
    headers.insert(header::SERVER, "axum".parse().expect("static string"));
    headers.insert(
        header::CONTENT_TYPE,
        "text/javascript".parse().expect("static string"),
    );
    (
        headers,
        include_str!("../../templates/static/htmx@2.0.2_response_targets.js"),
    )
}

async fn hyperscript_script() -> impl IntoResponse {
    let mut headers = HeaderMap::new();
    headers.insert(header::SERVER, "axum".parse().expect("static string"));
    headers.insert(
        header::CONTENT_TYPE,
        "text/javascript".parse().expect("static string"),
    );
    (
        headers,
        include_str!("../../templates/static/hyperscript.org@0.9.12.js"),
    )
}

async fn css_style() -> impl IntoResponse {
    let mut headers = HeaderMap::new();
    headers.insert(header::SERVER, "axum".parse().expect("static string"));
    headers.insert(
        header::CONTENT_TYPE,
        "text/css".parse().expect("static string"),
    );
    (headers, include_str!("../../templates/static/style.css"))
}

async fn fallback() -> impl IntoResponse {
    (
        StatusCode::NOT_FOUND,
        Html(include_str!("templates/404.html")),
    )
}
