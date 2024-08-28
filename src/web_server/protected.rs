use std::sync::Arc;

/// The routes protected by a login
// TODO: build a router with all the protected routes (potentially in submods)
// return that router into mod.rs
use axum::{routing::get, Extension, Router};

use crate::types::Config;

pub(crate) fn create_protected_router() -> Router {
    Router::new().route("/", get(self::get::root))
}

pub(super) mod get {
    use super::*;

    use askama_axum::IntoResponse;

    #[tracing::instrument(skip_all)]
    pub(super) async fn root(Extension(config): Extension<Arc<Config>>) -> impl IntoResponse {
        format!("root get: {:?}", config.extensions).into_response()
    }
}
