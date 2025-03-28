//! Everything to do with rendering timeframes for the frond-end and handling the routes for that
//! Note that some functions are defined in impl blocks in [`crate::types`] and use structs from
//! here.
use std::sync::Arc;

use askama::Template;
use askama_axum::IntoResponse;
use axum::{extract::{Path, Query}, http::StatusCode, Extension};
use axum_extra::extract::Form;
use serde::Deserialize;
use time::{macros::format_description, PrimitiveDateTime, UtcDateTime};
use tracing::warn;
use uuid::Uuid;

use crate::{db::{get_timeframe_once, insert_timeframe, link_timeframe, unlink_timeframe_once, update_timeframe_once}, types::{Config, HasId, Timeframe, TimeframeOnce}, web_server::{protected::error_display, InternalServerErrorTemplate}};

use super::login::AuthSession;

#[derive(Debug)]
pub(crate) enum TimeframeTemplateError {
    TimeFormat(time::error::Format),
    Render(askama::Error),
}
impl core::fmt::Display for TimeframeTemplateError {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        match self {
            Self::TimeFormat(e) => { write!(f, "Unable to format a known-good instance of time.") }
            Self::Render(e) => { write!(f, "Unable to render askama template.") }
        }
    }
}
impl std::error::Error for TimeframeTemplateError {}
impl From<time::error::Format> for TimeframeTemplateError {
    fn from(value: time::error::Format) -> Self {
        Self::TimeFormat(value)
    }
}
impl From<askama::Error> for TimeframeTemplateError {
    fn from(value: askama::Error) -> Self {
        Self::Render(value)
    }
}

#[derive(Template)]
#[template(path="timeframe/base.html", escape="none")]
pub(crate) struct TimeframeShow {
    /// timeframe to show
    pub(crate) timeframe: Timeframe<HasId>,
}

#[derive(Template)]
#[template(path="timeframe/timeframe-inner-once.html")]
pub(crate) struct TimeframeOnceTemplate {
    pub start_time: String,
    pub end_time: String,
}

#[derive(Template)]
#[template(path="timeframe/timeframe-inner-daily.html")]
pub(crate) struct TimeframeDailyTemplate {
    pub start_time: String,
    pub end_time: String,
}

#[derive(Template)]
#[template(path="timeframe/timeframe-inner-weekly.html")]
pub(crate) struct TimeframeWeeklyTemplate {
    pub start_dow: &'static str,
    pub start_time: String,
    pub end_dow: &'static str,
    pub end_time: String,
}

#[derive(Template)]
#[template(path="timeframe/timeframe-inner-monthly.html")]
pub(crate) struct TimeframeMonthlyTemplate {
    pub start_dom: i16,
    pub start_time: String,
    pub end_dom: i16,
    pub end_time: String,
}

#[derive(Template)]
#[template(path="timeframe_edit/base.html", escape="none")]
pub(crate) struct TimeframeEditBase {
    pub current: Timeframe<HasId>,
}

#[derive(Template)]
#[template(path="timeframe_edit/timeframe-inner-once.html")]
pub(crate) struct TimeframeOnceEditTemplate {
    pub start_time: String,
    pub end_time: String,
}

#[derive(Template)]
#[template(path="timeframe_edit/timeframe-inner-daily.html")]
pub(crate) struct TimeframeDailyEditTemplate {
    pub start_time: String,
    pub end_time: String,
}

#[derive(Template)]
#[template(path="timeframe_edit/timeframe-inner-weekly.html")]
pub(crate) struct TimeframeWeeklyEditTemplate {
    pub start_dow: &'static str,
    pub start_time: String,
    pub end_dow: &'static str,
    pub end_time: String,
}

#[derive(Template)]
#[template(path="timeframe_edit/timeframe-inner-monthly.html")]
pub(crate) struct TimeframeMonthlyEditTemplate {
    pub start_dom: i16,
    pub start_time: String,
    pub end_dom: i16,
    pub end_time: String,
}

#[derive(Template)]
#[template(path="timeframe_new/base.html")]
pub(crate) struct TimeframeNewBase {
    fwd_id: i32,
}

#[derive(Deserialize)]
pub(crate) struct FwdIdQuery {
    fwd_id: i32,
}

pub(crate) async fn new_template(
        Query(query): Query<FwdIdQuery>,
    ) -> impl IntoResponse {
    TimeframeNewBase { fwd_id: query.fwd_id, }.into_response()
}

#[derive(Template)]
#[template(path="timeframe_new/timeframe-new-once.html")]
pub(crate) struct TimeframeOnceNewTemplate {
    /// What time is it now? Used as default in time fields
    now_timestamp: String,
    /// ID of the forward to attach this timeframe to on POST
    fwd_id: i32,
}

#[derive(Template)]
#[template(path="timeframe_new/timeframe-new-daily.html")]
pub(crate) struct TimeframeDailyNewTemplate {
    /// What time is it now? Used as default in time fields
    now_timestamp: String,
    /// ID of the forward to attach this timeframe to on POST
    fwd_id: i32,
}

#[derive(Template)]
#[template(path="timeframe_new/timeframe-new-weekly.html")]
pub(crate) struct TimeframeWeeklyNewTemplate {
    /// What time is it now? Used as default in time fields
    now_timestamp: String,
    /// ID of the forward to attach this timeframe to on POST
    fwd_id: i32,
}

#[derive(Template)]
#[template(path="timeframe_new/timeframe-new-monthly.html")]
pub(crate) struct TimeframeMonthlyNewTemplate {
    /// What time is it now? Used as default in time fields
    now_timestamp: String,
    /// ID of the forward to attach this timeframe to on POST
    fwd_id: i32,
}

pub(crate) async fn once_new_template(
        Query(query): Query<FwdIdQuery>,
    ) -> impl IntoResponse {
    let now = time::UtcDateTime::now();
    let descr = format_description!("[year]-[month]-[day]T[hour]:[minute]");
    match now.format(&descr) {
        Ok(x) => {
            TimeframeOnceNewTemplate { now_timestamp: x, fwd_id: query.fwd_id, }.into_response()
        },
        Err(e) => {
            let error_uuid = Uuid::new_v4();
            warn!("Sending internal server error because I cannot format a timestamp: {e}. uuid: {error_uuid}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                InternalServerErrorTemplate { error_uuid },
            )
                .into_response();
        }
    }
}

#[derive(Deserialize)]
pub(crate) struct OnceNewFormData {
    start_time: String,
    end_time: String,
    fwd_id: i32,
}

pub(crate) async fn once_new_post(
        Extension(config): Extension<Arc<Config>>,
        Extension(session): Extension<AuthSession>,
        Form(data): Form<OnceNewFormData>,
    ) -> impl IntoResponse {
    let descr = format_description!("[year]-[month]-[day]T[hour]:[minute]");
    let start_time_parsed = match PrimitiveDateTime::parse(&data.start_time, descr) {
        Ok(x) => {
            x
        }
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                error_display("Der Startzeitpunkt war nicht im Format YYYY-mm-ddTHH-MM: {e}"),
            )
                .into_response();
        }
    };
    let end_time_parsed = match PrimitiveDateTime::parse(&data.end_time, descr) {
        Ok(x) => {
            x
        }
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                error_display("Der Endzeitpunkt war nicht im Format YYYY-mm-ddTHH-MM: {e}"),
            )
                .into_response();
        }
    };

    let timeframe = Timeframe::Once(TimeframeOnce::new(start_time_parsed, end_time_parsed));

    let mut con = match config.pool.clone().acquire().await {
        Ok(x) => {
            x
        }
        Err(e) => {
            let error_uuid = Uuid::new_v4();
            warn!("Sending internal server error because I cannot acquire a DB connection: {e}. uuid: {error_uuid}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                InternalServerErrorTemplate { error_uuid },
            )
                .into_response();
        }
    };
    let inserted = match insert_timeframe(&mut con, timeframe).await {
        Ok(x) => x,
        Err(e) => {
            let error_uuid = Uuid::new_v4();
            warn!("Sending internal server error because I cannot insert a new timeframe: {e}. uuid: {error_uuid}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                InternalServerErrorTemplate { error_uuid },
            )
                .into_response();
        }
    };
    match link_timeframe(&mut con, data.fwd_id, &inserted).await {
        Ok(x) => {
            TimeframeShow {
                timeframe: inserted,
            }.into_response()
        }
        Err(e) => {
            let error_uuid = Uuid::new_v4();
            warn!("Sending internal server error because I cannot link a new timeframe: {e}. uuid: {error_uuid}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                InternalServerErrorTemplate { error_uuid },
            )
                .into_response();
        }
    }
}

/// Handle a POST to the edit endpoint for an existing timeframe
pub(crate) async fn once_show_template(
        Extension(config): Extension<Arc<Config>>,
        Path(timeframeid): Path<i32>,
    ) -> impl IntoResponse {
    let mut con = match config.pool.clone().acquire().await {
        Ok(x) => {
            x
        }
        Err(e) => {
            let error_uuid = Uuid::new_v4();
            warn!("Sending internal server error because I cannot acquire a DB connection: {e}. uuid: {error_uuid}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                InternalServerErrorTemplate { error_uuid },
            )
                .into_response();
        }
    };
    let current = match get_timeframe_once(&mut con, timeframeid).await {
        Ok(Some(x)) => {
            x
        }
        Ok(None) => {
            warn!("Timeframe once {timeframeid} was requested but not found.");
            return StatusCode::NOT_FOUND.into_response();
        }
        Err(e) => {
            let error_uuid = Uuid::new_v4();
            warn!("Sending internal server error because I cannot get timeframe/once {timeframeid}: {e}. uuid: {error_uuid}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                InternalServerErrorTemplate { error_uuid },
            )
                .into_response();
        }
    };
    TimeframeShow {
        timeframe: Timeframe::Once(current),
    }.into_response()
}


pub(crate) async fn once_edit_template(
        Extension(config): Extension<Arc<Config>>,
        Path(timeframeid): Path<i32>,
    ) -> impl IntoResponse {
    let mut con = match config.pool.clone().acquire().await {
        Ok(x) => {
            x
        }
        Err(e) => {
            let error_uuid = Uuid::new_v4();
            warn!("Sending internal server error because I cannot acquire a DB connection: {e}. uuid: {error_uuid}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                InternalServerErrorTemplate { error_uuid },
            )
                .into_response();
        }
    };
    let current = match get_timeframe_once(&mut con, timeframeid).await {
        Ok(Some(x)) => {
            x
        }
        Ok(None) => {
            warn!("Timeframe once {timeframeid} was requested but not found.");
            return StatusCode::NOT_FOUND.into_response();
        }
        Err(e) => {
            let error_uuid = Uuid::new_v4();
            warn!("Sending internal server error because I cannot get timeframe/once {timeframeid}: {e}. uuid: {error_uuid}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                InternalServerErrorTemplate { error_uuid },
            )
                .into_response();
        }
    };
    TimeframeEditBase {
        current: Timeframe::Once(current),
    }.into_response()
}

#[derive(Deserialize)]
pub(crate) struct TimeframeOnceEditForm {
    start_time: String,
    end_time: String,
}

/// Handle a POST to the edit endpoint for an existing timeframe
pub(crate) async fn once_edit_post(
        Extension(config): Extension<Arc<Config>>,
        Path(timeframeid): Path<i32>,
        Form(data): Form<TimeframeOnceEditForm>,
    ) -> impl IntoResponse {
    // get the timeframe in question
    let mut con = match config.pool.clone().acquire().await {
        Ok(x) => {
            x
        }
        Err(e) => {
            let error_uuid = Uuid::new_v4();
            warn!("Sending internal server error because I cannot acquire a DB connection: {e}. uuid: {error_uuid}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                InternalServerErrorTemplate { error_uuid },
            )
                .into_response();
        }
    };
    let mut timeframe = match get_timeframe_once(&mut con, timeframeid).await {
        Ok(Some(x)) => x,
        Ok(None) => {
            return StatusCode::NOT_FOUND.into_response();
        }
        Err(e) => {
            let error_uuid = Uuid::new_v4();
            warn!("Sending internal server error because I cannot get the timeframe to edit: {e}. uuid: {error_uuid}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                InternalServerErrorTemplate { error_uuid },
            )
                .into_response();
        }
    };

    // parse the form data
    let descr = format_description!("[year]-[month]-[day]T[hour]:[minute]");
    timeframe.start_time = match PrimitiveDateTime::parse(&data.start_time, descr) {
        Ok(x) => {
            x
        }
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                error_display("Der Startzeitpunkt war nicht im Format YYYY-mm-ddTHH-MM: {e}"),
            )
                .into_response();
        }
    };
    timeframe.end_time = match PrimitiveDateTime::parse(&data.end_time, descr) {
        Ok(x) => {
            x
        }
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                error_display("Der Endzeitpunkt war nicht im Format YYYY-mm-ddTHH-MM: {e}"),
            )
                .into_response();
        }
    };

    match update_timeframe_once(&mut con, &timeframe).await {
        Ok(x) => {
            TimeframeShow {
                timeframe: Timeframe::Once(timeframe),
            }
            .into_response()
        }
        Err(e) => {
            let error_uuid = Uuid::new_v4();
            warn!("Sending internal server error because I cannot update the timeframe during edit: {e}. uuid: {error_uuid}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                InternalServerErrorTemplate { error_uuid },
            )
                .into_response()
        }
    }
}

/// Handle a DELETE to the delete endpoint for an existing timeframe
pub(crate) async fn once_delete(
        Extension(config): Extension<Arc<Config>>,
        Path(timeframeid): Path<i32>,
    ) -> impl IntoResponse {
    // unlink the timeframe
    if let Err(e) = unlink_timeframe_once(config.pool.clone(), timeframeid).await {
        let error_uuid = Uuid::new_v4();
        warn!("Sending internal server error because I cannot unlink timeframe/once {timeframeid}: {e}. uuid: {error_uuid}");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            InternalServerErrorTemplate { error_uuid },
        )
            .into_response();
    }
    "".into_response()
}
