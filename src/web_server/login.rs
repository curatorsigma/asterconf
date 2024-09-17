use crate::ldap::{LDAPBackend, UserCredentials};
use askama_axum::Template;
/// All the routes needed to do auth and the backend for that
// TODO: build a router, the backend and the routes for login
// return the router up to mod.rs
use axum::{
    http::StatusCode,
    response::{IntoResponse, Redirect},
    routing::{get, post},
    Form, Router,
};

pub type AuthSession = axum_login::AuthSession<LDAPBackend>;

#[derive(Template)]
#[template(path = "login.html")]
pub struct LoginTemplate {}

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
                warn!("Returning redirect, because the user supplied the wrong password or was not found via the user filter.");
                return Redirect::to("/login").into_response();
            }
            Err(e) => {
                warn!(
                    "Returning internal server error, because I could not ldap search a user: {e}"
                );
                let error_uuid = Uuid::new_v4();
                warn!("{error_uuid}");
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    InternalServerErrorTemplate { error_uuid },
                )
                    .into_response();
            }
        };

        if let Err(e) = auth_session.login(&user).await {
            warn!("Returning internal server error, because I could not ldap bind a user: {e}");
            let error_uuid = Uuid::new_v4();
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                InternalServerErrorTemplate { error_uuid },
            )
                .into_response();
        }
        Redirect::to("/").into_response()
    }
}

mod get {
    use tracing::{warn, Level};
    use uuid::Uuid;

    use crate::web_server::InternalServerErrorTemplate;

    use super::*;

    #[tracing::instrument(level=Level::DEBUG,skip_all)]
    pub async fn login() -> LoginTemplate {
        LoginTemplate {}
    }

    #[tracing::instrument(level=Level::DEBUG,skip_all)]
    pub async fn logout(mut auth_session: AuthSession) -> impl IntoResponse {
        match auth_session.logout().await {
            Ok(_) => Redirect::to("/login").into_response(),
            Err(e) => {
                warn!("Returning internal server error, because I could not log a user out: {e}");
                let error_uuid = Uuid::new_v4();
                warn!("{error_uuid}");
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    InternalServerErrorTemplate { error_uuid },
                )
                    .into_response();
            }
        }
    }
}
