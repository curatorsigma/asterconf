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
use axum_messages::{Message, Messages};
use serde::Deserialize;

use crate::ldap::{LDAPBackend, UserCredentials};

pub type AuthSession = axum_login::AuthSession<LDAPBackend>;

#[derive(Template)]
#[template(path = "login.html")]
pub struct LoginTemplate {
    messages: Vec<Message>,
    next: Option<String>,
}

// This allows us to extract the "next" field from the query string. We use this
// to redirect after log in.
#[derive(Debug,Deserialize)]
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
    use super::*;

    pub(super) async fn login(
        mut auth_session: super::AuthSession,
        messages: Messages,
        Form(creds): Form<UserCredentials>,
    ) -> impl IntoResponse {
        "hi".into_response()
        // let user = match auth_session.authenticate(creds.clone()).await {
        //     Ok(Some(user)) => user,
        //     Ok(None) => {
        //         messages.error("Invalid credentials");

        //         let mut login_url = "/login".to_string();
        //         if let Some(next) = creds.next {
        //             login_url = format!("{}?next={}", login_url, next);
        //         };

        //         return Redirect::to(&login_url).into_response();
        //     }
        //     Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        // };

        // if auth_session.login(&user).await.is_err() {
        //     return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        // }

        // messages.success(format!("Successfully logged in as {}", user.username));

        // if let Some(ref next) = creds.next {
        //     Redirect::to(next)
        // } else {
        //     Redirect::to("/")
        // }
        // .into_response()
    }
}

mod get {
    use super::*;

    pub async fn login(
        messages: Messages,
        Query( super::NextUrl { next }): Query<NextUrl>,
    ) -> LoginTemplate {
        LoginTemplate {
            messages: messages.into_iter().collect(),
            next,
        }
    }

    pub async fn logout(mut auth_session: AuthSession) -> impl IntoResponse {
        match auth_session.logout().await {
            Ok(_) => Redirect::to("/login").into_response(),
            Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        }
    }
}

