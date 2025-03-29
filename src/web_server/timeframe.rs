//! Everything to do with rendering timeframes for the frond-end and handling the routes for that
//! Note that some functions are defined in impl blocks in [`crate::types`] and use structs from
//! here.

use askama::Template;
use askama_axum::IntoResponse;
use axum::extract::Query;
use serde::Deserialize;

use crate::types::{HasId, Timeframe};

pub mod daily;
pub mod once;

#[derive(Debug)]
pub(crate) enum TimeframeTemplateError {
    /// Formatting time for output failed
    TimeFormat(time::error::Format),
    /// Rendering itself failed
    Render(askama::Error),
    /// time conversion failed
    TimeConversion(time::error::IndeterminateOffset),
}
impl core::fmt::Display for TimeframeTemplateError {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        match self {
            Self::TimeFormat(e) => {
                write!(f, "Unable to format a known-good instance of time: {e}.")
            }
            Self::Render(e) => {
                write!(f, "Unable to render askama template: {e}.")
            }
            Self::TimeConversion(e) => {
                write!(f, "Unable to convert time to local offset: {e}.")
            }
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
impl From<time::error::IndeterminateOffset> for TimeframeTemplateError {
    fn from(value: time::error::IndeterminateOffset) -> Self {
        Self::TimeConversion(value)
    }
}

#[derive(Template)]
#[template(path = "timeframe/base.html", escape = "none")]
pub(crate) struct TimeframeShow {
    /// timeframe to show
    pub(crate) timeframe: Timeframe<HasId>,
}

#[derive(Template)]
#[template(path = "timeframe/timeframe-inner-daily.html")]
pub(crate) struct TimeframeDailyTemplate {
    pub start_time: String,
    pub end_time: String,
}

#[derive(Template)]
#[template(path = "timeframe/timeframe-inner-weekly.html")]
pub(crate) struct TimeframeWeeklyTemplate {
    pub start_dow: &'static str,
    pub start_time: String,
    pub end_dow: &'static str,
    pub end_time: String,
}

#[derive(Template)]
#[template(path = "timeframe/timeframe-inner-monthly.html")]
pub(crate) struct TimeframeMonthlyTemplate {
    pub start_dom: i16,
    pub start_time: String,
    pub end_dom: i16,
    pub end_time: String,
}

#[derive(Template)]
#[template(path = "timeframe_edit/base.html", escape = "none")]
pub(crate) struct TimeframeEditBase {
    pub current: Timeframe<HasId>,
}

#[derive(Template)]
#[template(path = "timeframe_edit/timeframe-inner-once.html")]
pub(crate) struct TimeframeOnceEditTemplate {
    pub start_time: String,
    pub end_time: String,
}

#[derive(Template)]
#[template(path = "timeframe_edit/timeframe-inner-daily.html")]
pub(crate) struct TimeframeDailyEditTemplate {
    pub start_time: String,
    pub end_time: String,
}

#[derive(Template)]
#[template(path = "timeframe_edit/timeframe-inner-weekly.html")]
pub(crate) struct TimeframeWeeklyEditTemplate {
    pub start_dow: &'static str,
    pub start_time: String,
    pub end_dow: &'static str,
    pub end_time: String,
}

#[derive(Template)]
#[template(path = "timeframe_edit/timeframe-inner-monthly.html")]
pub(crate) struct TimeframeMonthlyEditTemplate {
    pub start_dom: i16,
    pub start_time: String,
    pub end_dom: i16,
    pub end_time: String,
}

#[derive(Template)]
#[template(path = "timeframe_new/base.html")]
pub(crate) struct TimeframeNewBase {
    fwd_id: i32,
}

#[derive(Template)]
#[template(path = "timeframe_new/timeframe-new-weekly.html")]
pub(crate) struct TimeframeWeeklyNewTemplate {
    /// What time is it now? Used as default in time fields
    now_timestamp: String,
    /// ID of the forward to attach this timeframe to on POST
    fwd_id: i32,
}

#[derive(Template)]
#[template(path = "timeframe_new/timeframe-new-monthly.html")]
pub(crate) struct TimeframeMonthlyNewTemplate {
    /// What time is it now? Used as default in time fields
    now_timestamp: String,
    /// ID of the forward to attach this timeframe to on POST
    fwd_id: i32,
}

#[derive(Deserialize)]
pub(crate) struct FwdIdQuery {
    fwd_id: i32,
}

pub(crate) async fn new_template(Query(query): Query<FwdIdQuery>) -> impl IntoResponse {
    TimeframeNewBase {
        fwd_id: query.fwd_id,
    }
    .into_response()
}
