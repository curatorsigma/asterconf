use std::sync::Arc;

/// The routes protected by a login
// TODO: build a router with all the protected routes (potentially in submods)
// return that router into mod.rs
use axum::{routing::get, Extension, Router};

use crate::types::Config;

pub(crate) fn create_protected_router() -> Router {
    Router::new().route("/", get(self::get::root))
    // .layer(Extension(our_config))
}

pub(super) mod get {
    use super::*;

    use askama_axum::IntoResponse;

    #[tracing::instrument(skip_all)]
    pub(super) async fn root() -> impl IntoResponse {
        "hi there root get".into_response()
        // format!("root get: {:?}", config.extensions).into_response()
    }
}
