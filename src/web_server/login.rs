use askama_axum::Template;
/// All the routes needed to do auth and the backend for that
// TODO: build a router, the backend and the routes for login
// return the router up to mod.rs
use axum::{
    extract::Query,
    http::StatusCode,
    response::{IntoResponse, Redirect},
    routing::{get, post},
    Form, Router,
};
use serde::Deserialize;

use crate::ldap::{LDAPBackend, UserCredentials};

pub type AuthSession = axum_login::AuthSession<LDAPBackend>;

#[derive(Template)]
#[template(path = "login.html")]
pub struct LoginTemplate {
    next: Option<String>,
}

// This allows us to extract the "next" field from the query string. We use this
// to redirect after log in.
#[derive(Debug, Deserialize)]
pub struct NextUrl {
    next: Option<String>,
}

pub(crate) fn create_login_router() -> Router<()> {
    Router::new()
        .route("/login", get(self::get::login))
        .route("/login", post(self::post::login))
        .route("/logout", get(self::get::logout))
}

mod post {
    use tracing::{info, warn, Level};
    use uuid::Uuid;

    use crate::web_server::InternalServerErrorTemplate;

    use super::*;

    #[tracing::instrument(level=Level::DEBUG,skip_all,ret)]
    pub(super) async fn login(
        mut auth_session: super::AuthSession,
        Form(creds): Form<UserCredentials>,
    ) -> impl IntoResponse {
        let user = match auth_session.authenticate(creds.clone()).await {
            Ok(Some(user)) => {
                info!("New user logged in: {:?}", user);
                user
            }
            Ok(None) => {
                let mut login_url = "/login".to_string();
                if let Some(next) = creds.next {
                    login_url = format!("{}?next={}", login_url, next);
                };

                warn!("Returning redirect, because the user supplied the wrong password");
                return Redirect::to(&login_url).into_response();
            }
            Err(e) => {
                warn!("Returning internal server error, because I could not ldap search a user: {e}");
                let error_uuid = Uuid::new_v4();
                warn!("{error_uuid}");
                return (StatusCode::INTERNAL_SERVER_ERROR, InternalServerErrorTemplate { error_uuid }).into_response();
            }
        };

        if let Err(e) = auth_session.login(&user).await {
            warn!("Returning internal server error, because I could not ldap bind a user: {e}");
            let error_uuid = Uuid::new_v4();
            return (StatusCode::INTERNAL_SERVER_ERROR, InternalServerErrorTemplate { error_uuid }).into_response();
        }

        if let Some(ref next) = creds.next {
            Redirect::to(next)
        } else {
            Redirect::to("/")
        }
        .into_response()
    }
}

mod get {
    use tracing::{warn, Level};
    use uuid::Uuid;

    use crate::web_server::InternalServerErrorTemplate;

    use super::*;

    #[tracing::instrument(level=Level::DEBUG,skip_all)]
    pub async fn login(Query(super::NextUrl { next }): Query<NextUrl>) -> LoginTemplate {
        LoginTemplate { next }
    }

    #[tracing::instrument(level=Level::DEBUG,skip_all)]
    pub async fn logout(mut auth_session: AuthSession) -> impl IntoResponse {
        match auth_session.logout().await {
            Ok(_) => Redirect::to("/login").into_response(),
            Err(e) => {
                warn!("Returning internal server error, because I could not log a user out: {e}");
                let error_uuid = Uuid::new_v4();
                warn!("{error_uuid}");
                return (StatusCode::INTERNAL_SERVER_ERROR, InternalServerErrorTemplate { error_uuid }).into_response();
            },
        }
    }
}
