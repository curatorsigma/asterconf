use std::sync::Arc;

use askama::Template;
/// The routes protected by a login
use axum::{
    routing::{delete, get, post},
    Extension, Router,
};

use crate::types::{CallForward, Config, Context, HasId};

fn error_display(s: &str) -> String {
    format!("<div id=\"error_display\" _=\"on htmx:beforeSend from elsewhere set my innerHTML to ''\">{}</div>", s)
}

pub(crate) fn create_protected_router() -> Router {
    Router::new()
        .route("/", get(self::get::root))
        .route(
            "/web/call-forward/:fwdid",
            get(self::get::single_call_forward).delete(self::delete::single_call_forward_delete),
        )
        .route(
            "/web/call-forward/:fwdid/edit",
            get(self::get::single_call_forward_edit).post(self::post::single_call_forward_edit),
        )
        .route(
            "/web/call-forward/new",
            get(self::get::single_call_forward_new).post(self::post::single_call_forward_new),
        )
}

#[derive(Template)]
#[template(path = "call_forward_show.html")]
struct SingleCallForwardShowTemplate<'a> {
    fwd: CallForward<'a, HasId>,
    contexts: Vec<&'a Context>,
}

pub(super) mod get {
    use crate::{
        db::{get_all_call_forwards, get_call_forward_by_id},
        types::{CallForward, Context, HasId},
        web_server::login::AuthSession,
    };

    use super::*;

    use askama::Template;
    use askama_axum::IntoResponse;
    use axum::{extract::Path, http::StatusCode};
    use tracing::{event, Level};

    #[derive(Template)]
    #[template(path = "new_call_forward_button.html")]
    struct NewCallForwardButtonTemplate {}

    #[tracing::instrument(skip_all)]
    pub(super) async fn new_call_forward_button() -> impl IntoResponse {
        NewCallForwardButtonTemplate {}
    }

    #[derive(Template)]
    #[template(path = "landing.html")]
    struct LandingTemplate<'a> {
        username: String,
        existing_forwards: Vec<CallForward<'a, HasId>>,
        contexts: Vec<&'a Context>,
    }

    #[tracing::instrument(skip_all)]
    pub(super) async fn root(
        auth_session: AuthSession,
        Extension(config): Extension<Arc<Config>>,
    ) -> impl IntoResponse {
        let user = if let Some(x) = auth_session.user {
            x
        } else {
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        };
        let call_forward_res = get_all_call_forwards(&config).await;
        match call_forward_res {
            Ok(forwards) => {
                event!(Level::INFO, "{forwards:?}");
                LandingTemplate {
                    username: user.username,
                    existing_forwards: forwards,
                    contexts: config.contexts.values().collect::<Vec<_>>(),
                }
                .into_response()
            }
            Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        }
    }

    #[tracing::instrument(skip_all)]
    pub(super) async fn single_call_forward(
        Extension(config): Extension<Arc<Config>>,
        Path(fwdid): Path<i32>,
    ) -> impl IntoResponse {
        let fwd_res = get_call_forward_by_id(&config, fwdid).await;
        match fwd_res {
            Ok(fwd) => SingleCallForwardShowTemplate {
                fwd,
                contexts: config.contexts.values().collect::<Vec<_>>(),
            }
            .into_response(),
            Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        }
    }

    #[derive(Template)]
    #[template(path = "call_forward_edit.html")]
    struct SingleCallForwardEditTemplate<'a> {
        current_forward: Option<CallForward<'a, HasId>>,
        contexts: Vec<&'a Context>,
    }

    #[tracing::instrument(skip_all)]
    pub(super) async fn single_call_forward_edit(
        Extension(config): Extension<Arc<Config>>,
        Path(fwdid): Path<i32>,
    ) -> impl IntoResponse {
        // Get call forward by id
        let fwd_res = get_call_forward_by_id(&config, fwdid).await;
        match fwd_res {
            Ok(current_forward) => SingleCallForwardEditTemplate {
                current_forward: Some(current_forward),
                contexts: config.contexts.values().collect::<Vec<_>>(),
            }
            .into_response(),
            Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        }
    }

    #[tracing::instrument(skip_all)]
    pub(super) async fn single_call_forward_new(
        Extension(config): Extension<Arc<Config>>,
    ) -> impl IntoResponse {
        // Get call forward by id
        SingleCallForwardEditTemplate {
            current_forward: None,
            contexts: config.contexts.values().collect::<Vec<_>>(),
        }
        .into_response()
    }
}

pub(super) mod post {
    use super::*;

    use std::sync::Arc;

    use askama_axum::IntoResponse;
    use axum::{extract::Path, http::StatusCode, Extension};
    use serde::Deserialize;
    use tracing::info;

    use crate::{
        db::{new_call_forward, update_call_forward, DBError},
        types::{CallForward, Config, HasId, NoId},
    };

    #[derive(Deserialize, Debug)]
    pub struct ForwardFormData {
        from: String,
        to: String,
        ctx_checkboxes: Option<Vec<String>>,
    }

    #[tracing::instrument(skip_all)]
    pub(super) async fn single_call_forward_new(
        Extension(config): Extension<Arc<Config>>,
        axum_extra::extract::Form(forward_form): axum_extra::extract::Form<ForwardFormData>,
    ) -> impl IntoResponse {
        let Some(from_ext) = config.extensions.get(&forward_form.from) else {
            return (
                StatusCode::BAD_REQUEST,
                error_display("Please set a From-Extension that is known."),
            )
                .into_response();
        };
        let to_ext = crate::types::Extension::create_from_name(&config, forward_form.to);

        let mut contexts = vec![];
        let Some(ctx_checkboxes) = forward_form.ctx_checkboxes else {
            return (
                StatusCode::BAD_REQUEST,
                error_display("Please select at least one context"),
            )
                .into_response();
        };
        for ctx in ctx_checkboxes {
            let Some(this_ctx) = config.contexts.get(&ctx) else {
                return (
                    StatusCode::BAD_REQUEST,
                    error_display(&format!("Context Not Found: {ctx}.")),
                )
                    .into_response();
            };
            contexts.push(this_ctx);
        }

        let forward = CallForward {
            fwd_id: NoId {},
            from: from_ext.clone(),
            to: to_ext.clone(),
            in_contexts: contexts,
        };

        let res = new_call_forward(&config, forward).await;

        match res {
            Ok(x) => SingleCallForwardShowTemplate {
                fwd: x,
                contexts: config.contexts.values().collect::<Vec<_>>(),
            }
            .into_response(),
            Err(DBError::OverlappingCallForwards(x, y)) => (
                StatusCode::BAD_REQUEST,
                error_display(&format!(
                    "The Extension {x} already has a forward set in Context {y}."
                )),
            )
                .into_response(),
            Err(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                error_display("Internal Server Error. Please reload and try again."),
            )
                .into_response(),
        }
    }

    #[tracing::instrument(skip_all)]
    pub(super) async fn single_call_forward_edit(
        Extension(config): Extension<Arc<Config>>,
        Path(fwdid): Path<i32>,
        axum_extra::extract::Form(forward_form): axum_extra::extract::Form<ForwardFormData>,
    ) -> impl IntoResponse {
        let Some(from_ext) = config.extensions.get(&forward_form.from) else {
            return (
                StatusCode::BAD_REQUEST,
                error_display("Please set a From-Extension that is known."),
            )
                .into_response();
        };
        let to_ext = crate::types::Extension::create_from_name(&config, forward_form.to);

        let mut contexts = vec![];
        let Some(ctx_checkboxes) = forward_form.ctx_checkboxes else {
            return (
                StatusCode::BAD_REQUEST,
                error_display("Please select at least one context"),
            )
                .into_response();
        };
        for ctx in ctx_checkboxes {
            let Some(this_ctx) = config.contexts.get(&ctx) else {
                return (StatusCode::BAD_REQUEST, error_display("Context Not Found."))
                    .into_response();
            };
            contexts.push(this_ctx);
        }

        let forward = CallForward {
            fwd_id: HasId::new(fwdid),
            from: from_ext.clone(),
            to: to_ext.clone(),
            in_contexts: contexts,
        };
        let update_res = update_call_forward(&config, &forward).await;

        match update_res {
            Ok(()) => SingleCallForwardShowTemplate {
                fwd: forward,
                contexts: config.contexts.values().collect::<Vec<_>>(),
            }
            .into_response(),
            Err(DBError::CannotSelectCallForward(fwdid)) => (
                StatusCode::BAD_REQUEST,
                error_display("Call Forward no longer exists. Please reload and try again."),
            )
                .into_response(),
            Err(DBError::CannotSelectContexts(_)) => (
                StatusCode::BAD_REQUEST,
                error_display("Context not found.. Please reload and try again."),
            )
                .into_response(),
            Err(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                error_display("Internal Server Error. Please reload and try again."),
            )
                .into_response(),
        }
    }
}

pub(super) mod delete {
    use std::sync::Arc;

    use askama_axum::IntoResponse;
    use axum::{extract::Path, http::StatusCode, Extension};

    use crate::{db::delete_call_forward_by_id, types::Config};

    #[tracing::instrument(skip_all)]
    pub(super) async fn single_call_forward_delete(
        Extension(config): Extension<Arc<Config>>,
        Path(fwdid): Path<i32>,
    ) -> impl IntoResponse {
        let fwd_res = delete_call_forward_by_id(&config, fwdid).await;
        match fwd_res {
            Ok(()) => { "".into_response() }.into_response(),
            Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        }
    }
}
