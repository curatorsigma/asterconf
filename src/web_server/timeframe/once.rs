use std::sync::Arc;

use askama::Template;
use askama_axum::IntoResponse;
use axum::{
    extract::{Path, Query},
    http::StatusCode,
    Extension,
};
use axum_extra::extract::Form;
use serde::Deserialize;
use time::{macros::format_description, PrimitiveDateTime};
use tracing::warn;
use uuid::Uuid;

use crate::{
    db::{
        get_timeframe_once, insert_timeframe, link_timeframe, unlink_timeframe_once,
        update_timeframe_once,
    },
    types::{Config, Timeframe, TimeframeOnce},
    web_server::{protected::error_display, InternalServerErrorTemplate},
};

use super::{FwdIdQuery, TimeframeEditBase, TimeframeShow};

#[derive(Template)]
#[template(path = "timeframe/timeframe-inner-once.html")]
pub(crate) struct TimeframeOnceTemplate {
    pub start_time: String,
    pub end_time: String,
}

#[derive(Deserialize)]
pub(crate) struct OnceNewFormData {
    start_time: String,
    end_time: String,
    fwd_id: i32,
}

#[derive(Template)]
#[template(path = "timeframe_new/timeframe-new-once.html")]
pub(crate) struct TimeframeOnceNewTemplate {
    /// What time is it now? Used as default in time fields
    now_timestamp: String,
    /// ID of the forward to attach this timeframe to on POST
    fwd_id: i32,
}

pub(crate) async fn once_new_template(Query(query): Query<FwdIdQuery>) -> impl IntoResponse {
    let now = time::UtcDateTime::now();
    let descr = format_description!("[year]-[month]-[day]T[hour]:[minute]");
    let our_offset = match time::UtcOffset::current_local_offset() {
        Ok(x) => x,
        Err(e) => {
            let error_uuid = Uuid::new_v4();
            warn!("Sending internal server error because I cannot get the local UTC offset: {e}. uuid: {error_uuid}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                InternalServerErrorTemplate { error_uuid },
            )
                .into_response();
        }
    };
    let offset_time = now.to_offset(our_offset);
    match offset_time.format(&descr) {
        Ok(x) => TimeframeOnceNewTemplate {
            now_timestamp: x,
            fwd_id: query.fwd_id,
        }
        .into_response(),
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

pub(crate) async fn once_new_post(
    Extension(config): Extension<Arc<Config>>,
    Form(data): Form<OnceNewFormData>,
) -> impl IntoResponse {
    let descr = format_description!("[year]-[month]-[day]T[hour]:[minute]");
    // the user supplies data in assumed server local time
    let start_time_parsed = match PrimitiveDateTime::parse(&data.start_time, descr) {
        Ok(x) => {
            let our_offset = match time::UtcOffset::current_local_offset() {
                Ok(x) => x,
                Err(e) => {
                    let error_uuid = Uuid::new_v4();
                    warn!("Sending internal server error because I cannot get the local UTC offset: {e}. uuid: {error_uuid}");
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        InternalServerErrorTemplate { error_uuid },
                    )
                        .into_response();
                }
            };
            x.assume_offset(our_offset)
        }
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                error_display(&format!(
                    "Der Startzeitpunkt war nicht im Format YYYY-mm-ddTHH-MM: {e}"
                )),
            )
                .into_response();
        }
    };
    let end_time_parsed = match PrimitiveDateTime::parse(&data.end_time, descr) {
        Ok(x) => {
            let our_offset = match time::UtcOffset::current_local_offset() {
                Ok(x) => x,
                Err(e) => {
                    let error_uuid = Uuid::new_v4();
                    warn!("Sending internal server error because I cannot get the local UTC offset: {e}. uuid: {error_uuid}");
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        InternalServerErrorTemplate { error_uuid },
                    )
                        .into_response();
                }
            };
            x.assume_offset(our_offset)
        }
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                error_display(&format!(
                    "Der Endzeitpunkt war nicht im Format YYYY-mm-ddTHH-MM: {e}"
                )),
            )
                .into_response();
        }
    };
    // both times were given in local time - we now need to add the offset to make them Utc

    let timeframe = Timeframe::Once(TimeframeOnce::new(start_time_parsed, end_time_parsed));

    let mut con = match config.pool.clone().acquire().await {
        Ok(x) => x,
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
        Ok(()) => TimeframeShow {
            timeframe: inserted,
        }
        .into_response(),
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
        Ok(x) => x,
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
        Ok(Some(x)) => x,
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
    }
    .into_response()
}

pub(crate) async fn once_edit_template(
    Extension(config): Extension<Arc<Config>>,
    Path(timeframeid): Path<i32>,
) -> impl IntoResponse {
    let mut con = match config.pool.clone().acquire().await {
        Ok(x) => x,
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
        Ok(Some(x)) => x,
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
    }
    .into_response()
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
        Ok(x) => x,
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
            let our_offset = match time::UtcOffset::current_local_offset() {
                Ok(x) => x,
                Err(e) => {
                    let error_uuid = Uuid::new_v4();
                    warn!("Sending internal server error because I cannot get the local UTC offset: {e}. uuid: {error_uuid}");
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        InternalServerErrorTemplate { error_uuid },
                    )
                        .into_response();
                }
            };
            x.assume_offset(our_offset)
        }
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                error_display(&format!(
                    "Der Startzeitpunkt war nicht im Format YYYY-mm-ddTHH-MM: {e}"
                )),
            )
                .into_response();
        }
    };
    timeframe.end_time = match PrimitiveDateTime::parse(&data.end_time, descr) {
        Ok(x) => {
            let our_offset = match time::UtcOffset::current_local_offset() {
                Ok(x) => x,
                Err(e) => {
                    let error_uuid = Uuid::new_v4();
                    warn!("Sending internal server error because I cannot get the local UTC offset: {e}. uuid: {error_uuid}");
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        InternalServerErrorTemplate { error_uuid },
                    )
                        .into_response();
                }
            };
            x.assume_offset(our_offset)
        }
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                error_display(&format!(
                    "Der Endzeitpunkt war nicht im Format YYYY-mm-ddTHH-MM: {e}"
                )),
            )
                .into_response();
        }
    };

    match update_timeframe_once(&mut con, &timeframe).await {
        Ok(()) => TimeframeShow {
            timeframe: Timeframe::Once(timeframe),
        }
        .into_response(),
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
