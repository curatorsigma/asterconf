use std::sync::Arc;

use askama::Template;
/// The routes protected by a login
use axum::{
    routing::{get, post},
    Extension, Router,
};
use uuid::Uuid;

use crate::types::{CallForward, Config, Context, HasId};

fn error_display(s: &str) -> String {
    // we cannot control hx-swap separately for hx-target and hx-target-error
    // so we swap outer-html and add the surrounding div all the time
    format!("<div class=\"text-red-500 flex justify-center\" id=\"error_display\" _=\"on htmx:beforeSend from elsewhere set my innerHTML to ''\">{}</div>", s)
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
        .route(
            "/web/search-extension/from",
            post(self::post::from_search_extension),
        )
        .route(
            "/web/search-extension/to",
            post(self::post::to_search_extension),
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
        web_server::{login::AuthSession, InternalServerErrorTemplate},
    };

    use super::*;

    use askama::Template;
    use askama_axum::IntoResponse;
    use axum::{extract::Path, http::StatusCode};
    use tracing::{warn, Level};
    use uuid::Uuid;

    #[derive(Template)]
    #[template(path = "landing.html")]
    struct LandingTemplate<'a> {
        username: String,
        existing_forwards: Vec<CallForward<'a, HasId>>,
        contexts: Vec<&'a Context>,
    }

    pub(super) async fn root(
        auth_session: AuthSession,
        Extension(config): Extension<Arc<Config>>,
    ) -> impl IntoResponse {
        let user = if let Some(x) = auth_session.user {
            x
        } else {
            let error_uuid = Uuid::new_v4();
            warn!("Sending internal server error because there is no user in the auth session. uuid: {error_uuid}");
            return (StatusCode::INTERNAL_SERVER_ERROR, InternalServerErrorTemplate { error_uuid }).into_response();
        };
        let call_forward_res = get_all_call_forwards(&config).await;
        match call_forward_res {
            Ok(forwards) => {
                let mut contexts = config.contexts.values().collect::<Vec<_>>();
                contexts.sort_unstable_by(|a, b| a.display_name.cmp(&b.display_name));

                LandingTemplate {
                    username: user.username,
                    existing_forwards: forwards,
                    contexts,
                }
                .into_response()
            }
            Err(e) => {
                let error_uuid = Uuid::new_v4();
                warn!("Sending internal server error because there was a problem getting call forwards.");
                warn!("DBError: {e} Error-UUID: {error_uuid}");
                return (StatusCode::INTERNAL_SERVER_ERROR, InternalServerErrorTemplate { error_uuid }).into_response();
            },
        }
    }

    #[tracing::instrument(level=Level::DEBUG,skip_all)]
    pub(super) async fn single_call_forward(
        Extension(config): Extension<Arc<Config>>,
        Path(fwdid): Path<i32>,
    ) -> impl IntoResponse {
        let fwd_res = get_call_forward_by_id(&config, fwdid).await;
        match fwd_res {
            Ok(fwd) => {
                let mut contexts = config.contexts.values().collect::<Vec<_>>();
                contexts.sort_unstable_by(|a, b| a.display_name.cmp(&b.display_name));
                SingleCallForwardShowTemplate { fwd, contexts }
            }
            .into_response(),
            Err(e) => {
                let error_uuid = Uuid::new_v4();
                warn!("Sending internal server error because there was a problem getting a call forward.");
                warn!("DBError: {e}, Error-UUID: {error_uuid}");
                return (StatusCode::INTERNAL_SERVER_ERROR, InternalServerErrorTemplate { error_uuid }).into_response();
            },
        }
    }

    #[derive(Template)]
    #[template(path = "call_forward_edit.html")]
    struct SingleCallForwardEditTemplate<'a> {
        current_forward: Option<CallForward<'a, HasId>>,
        contexts: Vec<&'a Context>,
    }

    #[tracing::instrument(level=Level::DEBUG,skip_all)]
    pub(super) async fn single_call_forward_edit(
        Extension(config): Extension<Arc<Config>>,
        Path(fwdid): Path<i32>,
    ) -> impl IntoResponse {
        // Get call forward by id
        let fwd_res = get_call_forward_by_id(&config, fwdid).await;
        match fwd_res {
            Ok(current_forward) => {
                let mut contexts = config.contexts.values().collect::<Vec<_>>();
                contexts.sort_unstable_by(|a, b| a.display_name.cmp(&b.display_name));
                SingleCallForwardEditTemplate {
                    current_forward: Some(current_forward),
                    contexts,
                }
            }
            .into_response(),
            Err(e) => {
                let error_uuid = Uuid::new_v4();
                warn!("Sending internal server error because there was a problem getting a call forward.");
                warn!("DBError: {e}, Error-UUID: {error_uuid}");
                return (StatusCode::INTERNAL_SERVER_ERROR, InternalServerErrorTemplate { error_uuid }).into_response();
            },
        }
    }

    #[tracing::instrument(level=Level::DEBUG,skip_all)]
    pub(super) async fn single_call_forward_new(
        Extension(config): Extension<Arc<Config>>,
    ) -> impl IntoResponse {
        // Get call forward by id
        let mut contexts = config.contexts.values().collect::<Vec<_>>();
        contexts.sort_unstable_by(|a, b| a.display_name.cmp(&b.display_name));
        SingleCallForwardEditTemplate {
            current_forward: None,
            contexts,
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
    use tracing::{info, warn, Level};

    use crate::{
        db::{new_call_forward, update_call_forward, DBError}, types::{CallForward, Config, HasId, NoId}, web_server::{login::AuthSession, InternalServerErrorTemplate}
    };

    #[derive(Deserialize, Debug)]
    pub struct ForwardFormData {
        from: String,
        to: String,
        ctx_checkboxes: Option<Vec<String>>,
    }

    #[tracing::instrument(level=Level::DEBUG,skip_all)]
    pub(super) async fn single_call_forward_new(
        Extension(config): Extension<Arc<Config>>,
        Extension(session): Extension<AuthSession>,
        axum_extra::extract::Form(forward_form): axum_extra::extract::Form<ForwardFormData>,
    ) -> impl IntoResponse {
        let Some(from_ext) = config.extensions.get(&forward_form.from) else {
            return (
                StatusCode::BAD_REQUEST,
                error_display("Das 'Anruf für &darr;'-Feld muss eine bekannte Nummer sein."),
            )
                .into_response();
        };
        let to_ext = crate::types::Extension::create_from_name(&config, forward_form.to);

        let mut contexts = vec![];
        let Some(ctx_checkboxes) = forward_form.ctx_checkboxes else {
            return (
                StatusCode::BAD_REQUEST,
                error_display("Eine Weiterleitung muss mindestens einen Kontext enthalten."),
            )
                .into_response();
        };
        for ctx in ctx_checkboxes {
            let Some(this_ctx) = config.contexts.get(&ctx) else {
                return (
                    StatusCode::BAD_REQUEST,
                    error_display(&format!("Konnte den Kontext {ctx} nicht finden.")),
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
            Ok(x) => {
                let mut contexts = config.contexts.values().collect::<Vec<_>>();
                contexts.sort_unstable_by(|a, b| a.display_name.cmp(&b.display_name));
                info!("{} Inserted a new call forward: {}->{}@{:?}", session.user.expect("route should be protected").username, x.from.extension, x.to.extension, x.in_contexts);
                SingleCallForwardShowTemplate { fwd: x, contexts }.into_response()
            }
            Err(DBError::OverlappingCallForwards(x, y)) => (
                StatusCode::BAD_REQUEST,
                error_display(&format!(
                    "Anrufe an die Nummer {x} werden bereits weitergeleitet wenn sie von {y} kommen."
                )),
            )
                .into_response(),
            Err(e) => {
                let error_uuid = Uuid::new_v4();
                warn!("Sending internal server error because there was a problem INSERTing a call forward to the db.");
                warn!("DBError: {e}, Error-UUID: {error_uuid}");
                return (StatusCode::INTERNAL_SERVER_ERROR, InternalServerErrorTemplate { error_uuid }).into_response();
            },
        }
    }

    #[tracing::instrument(level=Level::DEBUG,skip_all)]
    pub(super) async fn single_call_forward_edit(
        Extension(config): Extension<Arc<Config>>,
        axum::Extension(session): axum::Extension<AuthSession>,
        Path(fwdid): Path<i32>,
        axum_extra::extract::Form(forward_form): axum_extra::extract::Form<ForwardFormData>,
    ) -> impl IntoResponse {
        let Some(from_ext) = config.extensions.get(&forward_form.from) else {
            return (
                StatusCode::BAD_REQUEST,
                error_display("Das 'Anruf für &darr;'-Feld muss eine bekannte Nummer sein."),
            )
                .into_response();
        };
        let to_ext = crate::types::Extension::create_from_name(&config, forward_form.to);

        let mut contexts = vec![];
        let Some(ctx_checkboxes) = forward_form.ctx_checkboxes else {
            return (
                StatusCode::BAD_REQUEST,
                error_display("Eine Weiterleitung muss mindestens einen Kontext enthalten dessen Anrufe weitergeleitet werden."),
            )
                .into_response();
        };
        for ctx in ctx_checkboxes {
            let Some(this_ctx) = config.contexts.get(&ctx) else {
                return (StatusCode::BAD_REQUEST, error_display(&format!("Kontext {ctx} konnte nicht gefunden werden.")))
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
            Ok(()) => {
                let mut contexts = config.contexts.values().collect::<Vec<_>>();
                contexts.sort_unstable_by(|a, b| a.display_name.cmp(&b.display_name));
                info!("{} Updated a call forward. Is now: {}->{}@{:?}.", session.user.expect("route should be protected").username, forward.from.extension, forward.to.extension, forward.in_contexts);
                SingleCallForwardShowTemplate {
                    fwd: forward,
                    contexts,
                }
                .into_response()
            }
            Err(DBError::CannotSelectCallForward(_)) => (
                StatusCode::BAD_REQUEST,
                error_display("Diese Weiterleitung existiert nicht mehr. Bitte Seite neu laden und erneut versuchen."),
            )
                .into_response(),
            Err(DBError::CannotSelectContexts(_)) => (
                StatusCode::BAD_REQUEST,
                error_display("Kontext existiert nicht mehr. Bitte Seite neu laden und erneut versuchen."),
            )
                .into_response(),
            Err(e) => {
                let error_uuid = Uuid::new_v4();
                warn!("Sending internal server error because there was a problem UPATEing a call forward to the db.");
                warn!("DBError: {e}, Error-UUID: {error_uuid}");
                return (StatusCode::INTERNAL_SERVER_ERROR, InternalServerErrorTemplate { error_uuid }).into_response();
            }
        }
    }

    #[derive(Deserialize)]
    pub(super) struct FromExtensionSearchForm {
        from: String,
    }
    #[derive(Deserialize)]
    pub(super) struct ToExtensionSearchForm {
        to: String,
    }

    /// Find the characters of `search` in `term`, in order
    /// Returns
    /// - None, if no match
    /// - Some([indices-in-term-where-the-chars-from-search-are]) if match
    fn string_fuzzy_match(search: &str, term: &str) -> Option<Vec<usize>> {
        let our_search = search.to_lowercase();
        let our_term = term.to_lowercase();

        let mut last_used_idx = None;
        let mut pos_vec = vec![];
        for char in our_search.chars() {
            let next_match = match last_used_idx {
                Some(idx) => (idx as usize + 1) + our_term[idx + 1..].find(char)?,
                None => our_term[0..].find(char)?,
            };
            pos_vec.push(next_match);
            last_used_idx = Some(next_match);
        }
        return Some(pos_vec);
    }

    fn mark_string_at_positions(s: &str, positions: Vec<usize>) -> Option<String> {
        let mut res = String::new();
        // the last char of s copied over
        let mut last = None;
        for idx in positions {
            if idx >= s.len() {
                return None;
            };
            match last {
                Some(last_idx) => {
                    // push everything between last and idx as-is
                    res.push_str(&s[last_idx + 1..idx]);
                }
                None => {
                    // push everything to the first index
                    res.push_str(&s[0..idx]);
                }
            };
            // push idx with marking
            res.push_str("<b>");
            res.push(s.chars().nth(idx)?);
            res.push_str("</b>");

            last = Some(idx);
        }
        // push the remainder of s
        match last {
            None => {
                res.push_str(s);
            }
            Some(last_idx) => {
                res.push_str(&s[last_idx + 1..]);
            }
        };
        return Some(res);
    }

    #[derive(Template)]
    #[template(path = "search_results.html")]
    pub(super) struct SearchResultTemplate {
        results: Vec<(String, String)>,
        target: String,
    }

    #[tracing::instrument(level=Level::DEBUG,skip_all)]
    pub(super) async fn from_search_extension(
        Extension(config): Extension<Arc<Config>>,
        axum_extra::extract::Form(search_form): axum_extra::extract::Form<FromExtensionSearchForm>,
    ) -> impl IntoResponse {
        let relevant_extensions = config
            .extensions
            .iter()
            .filter_map(|(ext_name, extension)| {
                let ext_hr_string = extension.to_string();
                let fuzzy_match = string_fuzzy_match(&search_form.from, &ext_hr_string);
                if let Some(y) = fuzzy_match {
                    Some((
                        mark_string_at_positions(&ext_hr_string, y)?,
                        ext_name.to_owned(),
                    ))
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        SearchResultTemplate {
            results: relevant_extensions,
            target: "from".to_owned(),
        }
    }

    #[tracing::instrument(level=Level::DEBUG,skip_all)]
    pub(super) async fn to_search_extension(
        Extension(config): Extension<Arc<Config>>,
        axum_extra::extract::Form(search_form): axum_extra::extract::Form<ToExtensionSearchForm>,
    ) -> impl IntoResponse {
        let relevant_extensions = config
            .extensions
            .iter()
            .filter_map(|(ext_name, extension)| {
                let ext_hr_string = extension.to_string();
                let fuzzy_match = string_fuzzy_match(&search_form.to, &ext_hr_string);
                if let Some(y) = fuzzy_match {
                    Some((
                        mark_string_at_positions(&ext_hr_string, y)?,
                        ext_name.to_owned(),
                    ))
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        SearchResultTemplate {
            results: relevant_extensions,
            target: "to".to_owned(),
        }
    }

    #[cfg(test)]
    mod test {
        use crate::web_server::protected::post::mark_string_at_positions;

        use super::string_fuzzy_match;

        #[test]
        fn fuzzy_match_success() {
            let search = "rs";
            let term = "rust";
            assert_eq!(string_fuzzy_match(search, term), Some(vec![0, 2]));

            let search = "usf";
            let term = "usomef";
            assert_eq!(string_fuzzy_match(search, term).unwrap(), vec![0, 1, 5]);

            let search = "abc";
            let term = "ababcab";
            assert_eq!(string_fuzzy_match(search, term).unwrap(), vec![0, 1, 4]);

            let search = "AbC";
            let term = "aBabcab";
            assert_eq!(string_fuzzy_match(search, term).unwrap(), vec![0, 1, 4]);
        }
        #[test]
        fn fuzzy_match_fail() {
            let search = "no";
            let term = "this is neg a match";
            assert_eq!(string_fuzzy_match(search, term), None);
        }

        #[test]
        fn mark_string() {
            let string = "Hello There";
            let positions = vec![2];
            assert_eq!(
                mark_string_at_positions(string, positions),
                Some("He<b>l</b>lo There".to_string())
            );

            let string = "Hello There";
            let positions = vec![0, 2];
            assert_eq!(
                mark_string_at_positions(string, positions),
                Some("<b>H</b>e<b>l</b>lo There".to_string())
            );

            let string = "Hello There";
            let positions = vec![100];
            assert_eq!(mark_string_at_positions(string, positions), None);
        }
    }
}

pub(super) mod delete {
    use super::*;

    use std::sync::Arc;

    use askama_axum::IntoResponse;
    use axum::{extract::Path, http::StatusCode, Extension};
    use tracing::{info, warn, Level};

    use crate::{db::delete_call_forward_by_id, types::Config, web_server::{login::AuthSession, InternalServerErrorTemplate}};

    #[tracing::instrument(level=Level::DEBUG,skip_all)]
    pub(super) async fn single_call_forward_delete(
        Extension(config): Extension<Arc<Config>>,
        Extension(session): Extension<AuthSession>,
        Path(fwdid): Path<i32>,
    ) -> impl IntoResponse {
        let fwd_res = delete_call_forward_by_id(&config, fwdid).await;
        match fwd_res {
            Ok(()) => {
                info!("{} Deleted a call forward.", session.user.expect("route should be protected").username);
                "".into_response()
            },
            Err(e) => {
                let error_uuid = Uuid::new_v4();
                warn!("Sending internal server error because there was a problem DELETEing a call forward to the db.");
                warn!("DBError: {e}, Error-UUID: {error_uuid}");
                return (StatusCode::INTERNAL_SERVER_ERROR, InternalServerErrorTemplate { error_uuid }).into_response();
            },
        }
    }
}
