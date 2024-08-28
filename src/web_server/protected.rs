/// The routes protected by a login
// TODO: build a router with all the protected routes (potentially in submods)
// return that router into mod.rs
use axum::{routing::get, Router};

pub(crate) fn create_protected_router() -> Router {
    Router::new()
        .route("/", get(self::get::root))
}

pub(super) mod get {
    use askama_axum::IntoResponse;

    pub(super) async fn root() -> impl IntoResponse {
        "hi".into_response()
    }
}
