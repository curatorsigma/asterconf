//! Everything to do with rendering timeframes for the frond-end and handling the routes for that
//! Note that some functions are defined in impl blocks in [`crate::types`] and use structs from
//! here.

use askama::Template;
use askama_axum::IntoResponse;
use axum::extract::Query;
use serde::Deserialize;
use time::{macros::format_description, OffsetDateTime, PrimitiveDateTime, UtcOffset};

use crate::types::{HasId, Timeframe};

pub mod daily;
pub mod monthly;
pub mod once;
pub mod weekly;

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
impl core::error::Error for TimeframeTemplateError {}
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
#[template(path = "timeframe_new/base.html")]
pub(crate) struct TimeframeNewBase {
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

#[derive(Debug)]
enum ParseDatetimeError {
    Offset(time::error::IndeterminateOffset),
    Format(time::error::Parse),
}
impl core::fmt::Display for ParseDatetimeError {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        match self {
            Self::Offset(e) => {
                write!(f, "A server UTC offset could not be determined: {e}")
            }
            Self::Format(e) => {
                write!(f, "The input format was not YYYY-mm-ddTHH:MM: {e}")
            }
        }
    }
}
impl core::error::Error for ParseDatetimeError {}
impl From<time::error::IndeterminateOffset> for ParseDatetimeError {
    fn from(value: time::error::IndeterminateOffset) -> Self {
        Self::Offset(value)
    }
}
impl From<time::error::Parse> for ParseDatetimeError {
    fn from(value: time::error::Parse) -> Self {
        Self::Format(value)
    }
}

/// Given a Datetime passed to us by a user, convert it to the best aproximate [`OffsetDateTime`].
fn parse_datetime(time_str: &str) -> Result<OffsetDateTime, ParseDatetimeError> {
    let descr = format_description!("[year]-[month]-[day]T[hour]:[minute]");
    let time_parsed_primitive = PrimitiveDateTime::parse(time_str, descr)?;
    // the time, assuming it was given in UTC
    let time_as_if_utc = time_parsed_primitive.assume_utc();
    // the local server UTC offset active at the time interpreted as UTC
    let offset_at_time_interpreted_as_utc = UtcOffset::local_offset_at(time_as_if_utc)?;
    Ok(time_parsed_primitive.assume_offset(offset_at_time_interpreted_as_utc))
}
