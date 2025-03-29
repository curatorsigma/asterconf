//! Base types used across the codebase.
use std::fs::File;
use std::path::Path;
use std::{collections::HashMap, fmt::Display};

use askama::Template;
use axum_server::tls_rustls::RustlsConfig;
/// Structs used by the other components
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use time::macros::format_description;
use time::{OffsetDateTime, PrimitiveDateTime, Time, UtcDateTime, Weekday};
use tracing::{error, event, Level};

use crate::db::{get_timeframes, DBError};
use crate::web_server::protected::SingleCallForwardShowTemplate;
use crate::web_server::timeframe::once::TimeframeOnceTemplate;
use crate::web_server::timeframe::{TimeframeDailyEditTemplate, TimeframeDailyTemplate, TimeframeMonthlyEditTemplate, TimeframeMonthlyTemplate, TimeframeOnceEditTemplate, TimeframeShow, TimeframeTemplateError, TimeframeWeeklyEditTemplate, TimeframeWeeklyTemplate};

#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct Extension {
    // we may call-forward to external extensions that are not known by name statically
    // in this case, name will be empty
    name: Option<String>,
    // Note: this is usually a number code
    // but we have no guarantee of this, so we make it a raw String instead
    pub(crate) extension: String,
}
impl Display for Extension {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match &self.name {
            None => {
                write!(f, "{}", self.extension)
            }
            Some(x) => {
                write!(f, "{} ({})", x, self.extension)
            }
        }
    }
}
impl Extension {
    pub fn create_from_name(config: &Config, extension: String) -> Extension {
        let exten = config.extensions.get(&extension);
        match exten {
            None => Extension {
                name: None,
                extension,
            },
            Some(x) => x.clone(),
        }
    }
}

#[derive(Deserialize, Debug, PartialEq, Clone)]
pub struct Context {
    pub(crate) display_name: String,
    pub(crate) asterisk_name: String,
}
impl Display for Context {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", self.display_name)
    }
}
impl Context {
    /// get the correct display name from the config
    /// return the object with display_name and asterisk_name set
    ///
    /// Returns None, if the context does not exist in the config
    pub fn create_from_name<S: AsRef<str>>(config: &Config, asterisk_name: S) -> Option<&Context> {
        config.contexts.get(asterisk_name.as_ref())
    }
}

pub trait IdState {}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct NoId {}
impl IdState for NoId {}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct HasId {
    id: i32,
}
impl HasId {
    pub fn new(x: i32) -> Self {
        HasId { id: x }
    }
}
impl std::fmt::Display for HasId {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", self.id)
    }
}
impl From<HasId> for i32 {
    fn from(value: HasId) -> Self {
        value.id
    }
}
impl From<&HasId> for i32 {
    fn from(value: &HasId) -> Self {
        value.id
    }
}
impl From<i32> for HasId {
    fn from(value: i32) -> Self {
        Self { id: value }
    }
}
impl IdState for HasId {}

#[derive(Debug, Clone, PartialEq)]
pub struct CallForward<'a, S: IdState> {
    pub(crate) fwd_id: S,
    pub(crate) from: Extension,
    pub(crate) to: Extension,
    pub(crate) in_contexts: Vec<&'a Context>,
}
impl<'a, S: IdState> CallForward<'a, S> {
    pub fn intersecting_contexts<'b, T: IdState>(
        &'a self,
        other: &'b CallForward<T>,
    ) -> impl Iterator<Item = &'b &'a Context>
    where
        'a: 'b,
    {
        self.in_contexts
            .iter()
            .filter(move |&c| other.in_contexts.contains(c))
    }
}
impl<'a> CallForward<'a, HasId> {
    pub fn new(
        config: &'a Config,
        from: String,
        to: String,
        in_contexts: Vec<String>,
        fwd_id: i32,
    ) -> Result<CallForward<'a, HasId>, DBError> {
        let without_id = CallForward::<NoId>::new(config, from, to, in_contexts)?;
        Ok(without_id.set_id(fwd_id))
    }

    pub(crate) async fn try_into_with_timeframes(
        self,
        pool: PgPool,
    ) -> Result<CallForwardWithTimeframes<'a>, DBError> {
        let timeframes = get_timeframes(pool, self.fwd_id.into()).await?;
        Ok(CallForwardWithTimeframes {
            fwd_id: self.fwd_id,
            from: self.from,
            to: self.to,
            in_contexts: self.in_contexts,
            timeframes,
        })
    }
}
impl<'a> CallForward<'a, NoId> {
    pub fn new(
        config: &'a Config,
        from: String,
        to: String,
        in_contexts: Vec<String>,
    ) -> Result<CallForward<'a, NoId>, DBError> {
        let from_as_exten = Extension::create_from_name(config, from);
        let to_as_exten = Extension::create_from_name(config, to);
        let mut contexts_as_contexts: Vec<&Context> = vec![];
        for ctx in in_contexts.into_iter() {
            match Context::create_from_name(config, &ctx) {
                None => {
                    return Err(DBError::ContextDoesNotExist(ctx));
                }
                Some(x) => {
                    contexts_as_contexts.push(x);
                }
            }
        }
        Ok(CallForward::<NoId> {
            fwd_id: NoId {},
            from: from_as_exten,
            to: to_as_exten,
            in_contexts: contexts_as_contexts,
        })
    }

    pub fn set_id(self, new_id: i32) -> CallForward<'a, HasId> {
        CallForward::<'a, HasId> {
            fwd_id: HasId { id: new_id },
            from: self.from,
            to: self.to,
            in_contexts: self.in_contexts,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CallForwardWithTimeframes<'a> {
    pub(crate) fwd_id: HasId,
    pub(crate) from: Extension,
    pub(crate) to: Extension,
    pub(crate) in_contexts: Vec<&'a Context>,
    pub(crate) timeframes: Vec<Timeframe<HasId>>,
}
impl<'a> CallForwardWithTimeframes<'a> {
    pub(crate) fn most_specific_active_timeframe(&self) -> Option<&Timeframe<HasId>> {
        self.timeframes
            .iter()
            .filter(|t| t.currently_active())
            .min()
    }

    pub(crate) fn show(&self, contexts: &Vec<&'a Context>) -> String {
        SingleCallForwardShowTemplate {
            fwd: self.clone(),
            contexts: contexts.to_vec(),
        }.render().unwrap_or("ERROR".to_owned())
    }
}

/// Timeframe that happens exactly once.
///
/// - Entry (i.e. elements in the GUI) are made in current local time of the server.
/// - Comparison happens in UTC
#[derive(Debug, sqlx::FromRow, PartialEq, Eq, PartialOrd, Ord, Clone)]
pub(crate) struct TimeframeOnce<S>
where
    S: IdState,
{
    pub(crate) once_id: S,
    /// UTC datetime when this timeframe starts
    pub(crate) start_time: OffsetDateTime,
    /// UTC datetime when this timeframe ends
    pub(crate) end_time: OffsetDateTime,
}
impl<S> TimeframeOnce<S>
where
    S: IdState,
{
    pub(crate) fn currently_active(&self) -> bool {
        let now = time::UtcDateTime::now();
        self.start_time <= now && now <= self.end_time
    }
}
impl TimeframeOnce<NoId> {
    pub(crate) fn add_id(self, id: i32) -> TimeframeOnce<HasId> {
        TimeframeOnce {
            once_id: id.into(),
            start_time: self.start_time,
            end_time: self.end_time,
        }
    }
    pub(crate) fn new(start_time: OffsetDateTime, end_time: OffsetDateTime) -> Self {
        Self {
            once_id: NoId {},
            start_time,
            end_time,
        }
    }
}
impl TimeframeOnce<HasId> {
    pub(crate) fn id(&self) -> i32 {
        self.once_id.into()
    }

    /// Display this Timeframe for showing
    pub(crate) fn inner_display(&self) -> Result<String, TimeframeTemplateError> {
        let descr = format_description!("[year]-[month]-[day] [hour]:[minute]");
        let our_offset = time::UtcOffset::current_local_offset()?;
        Ok(TimeframeOnceTemplate {
            start_time: self.start_time.to_offset(our_offset).format(descr)?,
            end_time: self.end_time.to_offset(our_offset).format(descr)?,
        }
        .render()?)
    }

    /// Display this Timeframe for editing
    pub(crate) fn inner_edit_display(&self) -> Result<String, TimeframeTemplateError> {
        let descr = format_description!("[year]-[month]-[day] [hour]:[minute]");
        let our_offset = time::UtcOffset::current_local_offset()?;
        Ok(TimeframeOnceEditTemplate {
            start_time: self.start_time.to_offset(our_offset).format(descr)?,
            end_time: self.end_time.to_offset(our_offset).format(descr)?,
        }
        .render()?)
    }
}

/// Timeframe that is repeated daily.
///
/// - This consists of [`start_time`] and `end_time`.
/// - `start_time < end_time` is not enforced
/// - Both times are stored internally in Timezone-Unaware Time
/// - The user is always presented with times localized to the current server UTC offset
/// - comparison always happens in current server UTC offset
///
/// Take the followin example (german DST):
/// - Local time @ data-entry is UTC+01:00 (A-time)
/// - Local time @ compairson is UTC+02:00 (B-time)
/// - Entered timerange is 00:30-12:30 (J@entry == A == +01:00)
/// - This is store TZ-unaware as 00:30-12:30 (NO-TZ-INFO)
/// - At comparison time 01:00 in J@comparison == B == +02:00 this is compared TZ-unaware, and
///   01:00 is inside the timerange, even though it is now a different local UTC offset from when
///   the timeframe was entered
///
/// The rationale is, that people will probably intend "office hours start at 09:00, independent of
/// current DST status / current local UTC offset"
#[derive(Debug, sqlx::FromRow, PartialEq, Eq, PartialOrd, Ord, Clone)]
pub(crate) struct TimeframeDaily<S>
where
    S: IdState,
{
    /// The DB-ID of this timeframe if already present in the db
    pub(crate) daily_id: S,
    /// when in each day does this timewindow start?
    /// TZ-unaware, always interpreted as "in current local UTC offset"
    pub(crate) start_time: Time,
    /// when in each day does this timewindow end?
    /// TZ-unaware, always interpreted as "in current local UTC offset"
    pub(crate) end_time: Time,
}
impl<S> TimeframeDaily<S>
where
    S: IdState,
{
    pub(crate) fn currently_active(&self) -> bool {
        let now = time::OffsetDateTime::now_local().expect("Error handling from here is very difficult. Should be able to get local offset.");
        self.start_time <= now.time() && now.time() <= self.end_time
    }
}
impl TimeframeDaily<NoId> {
    pub(crate) fn add_id(self, id: i32) -> TimeframeDaily<HasId> {
        TimeframeDaily {
            daily_id: id.into(),
            start_time: self.start_time,
            end_time: self.end_time,
        }
    }
    pub(crate) fn new(start_time: Time, end_time: Time) -> Self {
        Self {
            daily_id: NoId {},
            start_time,
            end_time,
        }
    }
}
impl TimeframeDaily<HasId> {
    pub(crate) fn id(&self) -> i32 {
        self.daily_id.into()
    }

    /// template out the inner part of the display for this timeframe
    pub(crate) fn inner_display(&self) -> Result<String, TimeframeTemplateError> {
        let descr = format_description!("[year]-[month]-[day] [hour]:[minute]");
        Ok(TimeframeDailyTemplate {
            start_time: self.start_time.format(descr)?,
            end_time: self.end_time.format(descr)?,
        }
        .render()?)
    }

    /// template out the inner part of the display for this timeframe
    pub(crate) fn inner_edit_display(&self) -> Result<String, TimeframeTemplateError> {
        let descr = format_description!("[year]-[month]-[day] [hour]:[minute]");
        Ok(TimeframeDailyEditTemplate {
            start_time: self.start_time.format(descr)?,
            end_time: self.end_time.format(descr)?,
        }
        .render()?)
    }
}

#[derive(Clone, Debug, PartialEq, PartialOrd, Eq, Ord, sqlx::Type, Deserialize, Serialize)]
#[sqlx(type_name = "DAYOFWEEK")]
pub(crate) enum DayOfWeek {
    Monday,
    Tuesday,
    Wednesday,
    Thursday,
    Friday,
    Saturday,
    Sunday,
}
impl DayOfWeek {
    fn string_repr(&self) -> &'static str {
        match self {
            DayOfWeek::Monday => "Monday",
            DayOfWeek::Tuesday => "Tuesday",
            DayOfWeek::Wednesday => "Wednesday",
            DayOfWeek::Thursday => "Thursday",
            DayOfWeek::Friday => "Friday",
            DayOfWeek::Saturday => "Saturday",
            DayOfWeek::Sunday => "Sunday",
        }
    }
}
impl From<Weekday> for DayOfWeek {
    fn from(value: Weekday) -> Self {
        match value {
            Weekday::Monday => DayOfWeek::Monday,
            Weekday::Tuesday => DayOfWeek::Tuesday,
            Weekday::Wednesday => DayOfWeek::Wednesday,
            Weekday::Thursday => DayOfWeek::Thursday,
            Weekday::Friday => DayOfWeek::Friday,
            Weekday::Saturday => DayOfWeek::Saturday,
            Weekday::Sunday => DayOfWeek::Sunday,
        }
    }
}

/// Timeframe that repeats weekly. It starts at one weekday at a certain time and ends at another
/// weekday at another time.
///
/// Comparison happens in current server local UTC offset.
/// All times are intereted TZ-unaware (as though given in the current server local UTC offset).
///
/// see [`TimeframeDaily`] for more discussion of TZ-handling and DST here. [`TimeframeWeekly`] behaves
/// analogously.
#[derive(Debug, sqlx::FromRow, PartialEq, Eq, PartialOrd, Ord, Clone)]
pub(crate) struct TimeframeWeekly<S>
where
    S: IdState,
{
    pub(crate) weekly_id: S,
    pub(crate) start_dow: DayOfWeek,
    /// when on the start_dow does this timewindow start?
    /// TZ-unaware, always interpreted as "in current local UTC offset"
    /// see [`TimeframeDaily`] for more discussion of TZ-handling here. [`TimeframeWeekly`] behaves
    /// analogously.
    pub(crate) start_time: Time,
    pub(crate) end_dow: DayOfWeek,
    /// when on the end_dow does this timewindow end?
    /// TZ-unaware, always interpreted as "in current local UTC offset"
    /// see [`TimeframeDaily`] for more discussion of TZ-handling here. [`TimeframeWeekly`] behaves
    /// analogously.
    pub(crate) end_time: Time,
}
impl<S> TimeframeWeekly<S>
where
    S: IdState,
{
    pub(crate) fn currently_active(&self) -> bool {
        let now = time::OffsetDateTime::now_local().expect("Error handling from here is very difficult. Should be able to get local offset.");
        let now_dow: DayOfWeek = now.date().weekday().into();
        if self.start_dow < now_dow && now_dow < self.end_dow {
            true
        } else if self.start_dow == now_dow && self.start_time <= now.time() {
            true
        } else if self.end_dow == now_dow && self.end_time >= now.time() {
            true
        } else {
            false
        }
    }
}
impl TimeframeWeekly<NoId> {
    pub(crate) fn add_id(self, id: i32) -> TimeframeWeekly<HasId> {
        TimeframeWeekly {
            weekly_id: id.into(),
            start_dow: self.start_dow,
            start_time: self.start_time,
            end_dow: self.end_dow,
            end_time: self.end_time,
        }
    }
    pub(crate) fn new(
        start_dow: DayOfWeek,
        start_time: Time,
        end_dow: DayOfWeek,
        end_time: Time,
    ) -> Self {
        Self {
            weekly_id: NoId {},
            start_dow,
            start_time,
            end_dow,
            end_time,
        }
    }
}
impl TimeframeWeekly<HasId> {
    pub(crate) fn id(&self) -> i32 {
        self.weekly_id.into()
    }

    /// template out the inner part of the display for this timeframe
    pub(crate) fn inner_display(&self) -> Result<String, TimeframeTemplateError> {
        let descr = format_description!("[year]-[month]-[day] [hour]:[minute]");
        Ok(TimeframeWeeklyTemplate {
            start_dow: self.start_dow.string_repr(),
            start_time: self.start_time.format(descr)?,
            end_dow: self.end_dow.string_repr(),
            end_time: self.end_time.format(descr)?,
        }
        .render()?)
    }

    /// template out the inner part of the display for this timeframe
    pub(crate) fn inner_edit_display(&self) -> Result<String, TimeframeTemplateError> {
        let descr = format_description!("[year]-[month]-[day] [hour]:[minute]");
        Ok(TimeframeWeeklyEditTemplate {
            start_dow: self.start_dow.string_repr(),
            start_time: self.start_time.format(descr)?,
            end_dow: self.end_dow.string_repr(),
            end_time: self.end_time.format(descr)?,
        }
        .render()?)
    }
}

/// Timeframe that repeats every month.
///
/// It starts on a specific day-of-month at a specific time and ends on another day-of-month at
/// another time.
///
/// Times are TZ-unaware, relative to the current server local UTC offset.
/// see [`TimeframeDaily`] for more discussion of TZ-handling and DST here. [`TimeframeMonthly`] behaves
/// analogously.
#[derive(Debug, sqlx::FromRow, PartialEq, Eq, PartialOrd, Ord, Clone)]
pub(crate) struct TimeframeMonthly<S>
where
    S: IdState,
{
    pub(crate) monthly_id: S,
    /// Note: DOM being viable (an actual day of month) is not enforced, since we do not know how
    /// long an individual month is anyways. so setting `end_dom = 60` is the same as setting it to
    /// `28` in february.
    pub(crate) start_dom: i16,
    /// see [`TimeframeDaily`] for more discussion of TZ-handling and DST here. [`TimeframeMonthly`] behaves
    /// analogously.
    pub(crate) start_time: Time,
    pub(crate) end_dom: i16,
    /// see [`TimeframeDaily`] for more discussion of TZ-handling and DST here. [`TimeframeMonthly`] behaves
    /// analogously.
    pub(crate) end_time: Time,
}
impl<S> TimeframeMonthly<S>
where
    S: IdState,
{
    /// The current timestamp is between start_dom,start_time and end_dom,end_time
    pub(crate) fn currently_active(&self) -> bool {
        let now = time::OffsetDateTime::now_local().expect("Error handling from here is very difficult. Should be able to get local offset.");
        // now.day() returns in 1-31, which safely casts to i16
        if self.start_dom < (now.day() as i16) && (now.day() as i16) < self.end_dom {
            true
        } else if self.start_dom == (now.day() as i16) && self.start_time <= now.time() {
            true
        } else if self.end_dom == (now.day() as i16) && self.end_time >= now.time() {
            true
        } else {
            false
        }
    }
}
impl TimeframeMonthly<NoId> {
    pub(crate) fn add_id(self, id: i32) -> TimeframeMonthly<HasId> {
        TimeframeMonthly {
            monthly_id: id.into(),
            start_dom: self.start_dom,
            start_time: self.start_time,
            end_dom: self.end_dom,
            end_time: self.end_time,
        }
    }
    pub(crate) fn new(start_dom: i16, start_time: Time, end_dom: i16, end_time: Time) -> Self {
        Self {
            monthly_id: NoId {},
            start_dom,
            start_time,
            end_dom,
            end_time,
        }
    }
}
impl TimeframeMonthly<HasId> {
    pub(crate) fn id(&self) -> i32 {
        self.monthly_id.into()
    }

    /// template out the inner part of the display for this timeframe
    pub(crate) fn inner_display(&self) -> Result<String, TimeframeTemplateError> {
        let descr = format_description!("[year]-[month]-[day] [hour]:[minute]");
        Ok(TimeframeMonthlyTemplate {
            start_dom: self.start_dom,
            start_time: self.start_time.format(descr)?,
            end_dom: self.end_dom,
            end_time: self.end_time.format(descr)?,
        }
        .render()?)
    }

    /// template out the inner part of the display for this timeframe
    pub(crate) fn inner_edit_display(&self) -> Result<String, TimeframeTemplateError> {
        let descr = format_description!("[year]-[month]-[day] [hour]:[minute]");
        Ok(TimeframeMonthlyEditTemplate {
            start_dom: self.start_dom,
            start_time: self.start_time.format(descr)?,
            end_dom: self.end_dom,
            end_time: self.end_time.format(descr)?,
        }
        .render()?)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Timeframe<S>
where
    S: IdState,
{
    Once(TimeframeOnce<S>),
    Daily(TimeframeDaily<S>),
    Weekly(TimeframeWeekly<S>),
    Monthly(TimeframeMonthly<S>),
}
impl<S> Timeframe<S>
where
    S: IdState,
{
    /// The current time is in this timeframe.
    ///
    /// All times in Timeframe are interpreted as UTC.
    pub(crate) fn currently_active(&self) -> bool {
        match self {
            Self::Once(x) => x.currently_active(),
            Self::Daily(x) => x.currently_active(),
            Self::Weekly(x) => x.currently_active(),
            Self::Monthly(x) => x.currently_active(),
        }
    }
}
impl Timeframe<HasId> {
    pub(crate) fn id(&self) -> i32 {
        match self {
            Self::Once(x) => x.id(),
            Self::Daily(x) => x.id(),
            Self::Weekly(x) => x.id(),
            Self::Monthly(x) => x.id(),
        }
    }

    pub(crate) fn type_name(&self) -> &'static str {
        match self {
            Self::Once(_) => "once",
            Self::Daily(_) => "daily",
            Self::Weekly(_) => "weekly",
            Self::Monthly(_) => "monthly",
        }
    }

    pub(crate) fn inner_display(&self) -> String {
        let res = match self {
            Self::Once(x) => x.inner_display(),
            Self::Daily(x) => x.inner_display(),
            Self::Weekly(x) => x.inner_display(),
            Self::Monthly(x) => x.inner_display(),
        };
        match res {
            Ok(x) => x,
            Err(_) => "ERROR".to_owned(),
        }
    }

    pub(crate) fn display(&self) -> String {
        TimeframeShow {
            timeframe: self.clone(),
        }.render().unwrap_or("ERROR".to_owned())
    }

    pub(crate) fn inner_edit_display(&self) -> String {
        let res = match self {
            Self::Once(x) => x.inner_edit_display(),
            Self::Daily(x) => x.inner_edit_display(),
            Self::Weekly(x) => x.inner_edit_display(),
            Self::Monthly(x) => x.inner_edit_display(),
        };
        match res {
            Ok(x) => x,
            Err(_) => "????".to_owned(),
        }
    }
}

#[derive(Deserialize)]
struct ConfigFileData {
    extensions: Vec<Extension>,
    contexts: Vec<Context>,
    db_user: String,
    db_password: String,
    db_port: u16,
    db_host: String,
    db_database: String,
    tls_cert_file: String,
    tls_key_file: String,
    web_bind_addr: String,
    web_bind_port: u16,
    web_bind_port_tls: u16,
    agi_bind_addr: String,
    agi_bind_port: String,
    agi_digest_secret: String,
    ldap: LDAPConfigData,
}
impl std::fmt::Debug for ConfigFileData {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.debug_struct("ConfigFileData")
            .field("extensions", &self.extensions)
            .field("contexts", &self.contexts)
            .field("db_user", &self.db_user)
            .field("db_password", &"[redacted]")
            .field("db_port", &self.db_port)
            .field("db_host", &self.db_host)
            .field("db_database", &self.db_database)
            .field("tls_cert_file", &self.tls_cert_file)
            .field("tls_key_file", &self.tls_key_file)
            .field("web_bind_addr", &self.web_bind_addr)
            .field("web_bind_port", &self.web_bind_port)
            .field("web_bind_port_tls", &self.web_bind_port_tls)
            .field("agi_bind_addr", &self.agi_bind_addr)
            .field("agi_bind_port", &self.agi_bind_port)
            .field("agi_digest_secret", &self.agi_digest_secret)
            .field("ldap", &self.ldap)
            .finish()
    }
}

#[derive(Deserialize)]
struct LDAPConfigData {
    hostname: String,
    port: u16,
    bind_user: String,
    bind_password: String,
    base_dn: String,
    user_filter: String,
}
impl std::fmt::Debug for LDAPConfigData {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.debug_struct("LDAPConfigData")
            .field("hostname", &self.hostname)
            .field("port", &self.port)
            .field("bind_user", &self.bind_user)
            .field("bind_password", &"[redacted]")
            .field("base_dn", &self.base_dn)
            .field("user_filter", &self.user_filter)
            .finish()
    }
}

#[derive(Clone)]
pub struct Config {
    // extension name to Extension
    pub(crate) extensions: HashMap<String, Extension>,
    // context name to Context
    pub(crate) contexts: HashMap<String, Context>,
    // db connection pool
    pub(crate) pool: PgPool,
    // addr:port to bind the webserver to
    pub(crate) web_bind_string: String,
    // the port (we need it as u16 later)
    pub(crate) web_bind_port: u16,
    // the same for TLS
    pub(crate) web_bind_string_tls: String,
    pub(crate) web_bind_port_tls: u16,
    // addr:port to bind agi server to
    pub(crate) agi_bind_string: String,
    // the secret used in the SHA digest
    pub(crate) agi_digest_secret: String,
    /// config for the TLS layer
    pub(crate) rustls_config: RustlsConfig,
    pub(crate) ldap_config: crate::ldap::LDAPBackend,
}
impl std::fmt::Debug for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.debug_struct("Config")
            .field("extensions", &self.extensions)
            .field("contexts", &self.contexts)
            .field("web_bind_string", &self.web_bind_string)
            .field("web_bind_port", &self.web_bind_port)
            .field("web_bind_string_tls", &self.web_bind_string_tls)
            .field("web_bind_port_tls", &self.web_bind_port_tls)
            .field("agi_bind_string", &self.agi_bind_string)
            .field("agi_digest_secret", &"[redacted]")
            .field("rustls_config", &self.rustls_config)
            .field("ldap_config", &self.ldap_config)
            .finish()
    }
}
impl Config {
    // this will never be called inside the actual application (only during setup)
    // so I don't care about proper error handling
    // TODO: this needs to log its own errors, because it is called in lazy_static
    pub async fn create() -> Result<Config, Box<dyn std::error::Error>> {
        let config_path = Path::new("/etc/asterconf/config.yaml");
        let f = match File::open(config_path) {
            Ok(x) => x,
            Err(e) => {
                event!(
                    Level::ERROR,
                    "config file /etc/asterconf/config.yaml not readable: {e}"
                );
                return Err(Box::new(e));
            }
        };
        let config_data: ConfigFileData = match serde_yaml::from_reader(f) {
            Ok(x) => x,
            Err(e) => {
                event!(Level::ERROR, "config file had syntax errors: {e}");
                return Err(Box::new(e));
            }
        };
        // static extensions and contexts
        let extensions: HashMap<String, Extension> = config_data
            .extensions
            .into_iter()
            .map(|exten| (exten.extension.clone(), exten))
            .collect();
        let contexts: HashMap<String, Context> = config_data
            .contexts
            .into_iter()
            .map(|ctx| (ctx.asterisk_name.clone(), ctx))
            .collect();
        // postgres settings
        let url = format!(
            "postgres://{}:{}@{}:{}/{}",
            config_data.db_user,
            config_data.db_password,
            config_data.db_host,
            config_data.db_port,
            config_data.db_database
        );
        let pool = match sqlx::postgres::PgPool::connect(&url).await {
            Ok(x) => x,
            Err(e) => {
                event!(Level::ERROR, "Could not connect to postgres: {e}");
                return Err(Box::new(e));
            }
        };
        // webserver settings
        let web_bind_string = format!(
            "{}:{}",
            config_data.web_bind_addr, config_data.web_bind_port
        );
        let web_bind_string_tls = format!(
            "{}:{}",
            config_data.web_bind_addr, config_data.web_bind_port_tls
        );
        let agi_bind_string = format!(
            "{}:{}",
            config_data.agi_bind_addr, config_data.agi_bind_port
        );
        let rustls_config =
            match RustlsConfig::from_pem_file(config_data.tls_cert_file, config_data.tls_key_file)
                .await
            {
                Ok(x) => x,
                Err(e) => {
                    event!(
                        Level::ERROR,
                        "There was a problem reading the TLS cert/key: {e}"
                    );
                    return Err(Box::new(e));
                }
            };
        let ldap_config = match crate::ldap::LDAPBackend::new(
            &config_data.ldap.hostname,
            config_data.ldap.port,
            &config_data.ldap.bind_user,
            &config_data.ldap.bind_password,
            &config_data.ldap.user_filter,
            &config_data.ldap.base_dn,
        )
        .await
        {
            Ok(x) => x,
            Err(e) => {
                event!(
                    Level::ERROR,
                    "LDAP connection could not be established: {e}"
                );
                return Err(Box::new(e));
            }
        };
        Ok(Config {
            extensions,
            contexts,
            pool,
            web_bind_string,
            web_bind_string_tls,
            web_bind_port: config_data.web_bind_port,
            web_bind_port_tls: config_data.web_bind_port_tls,
            agi_bind_string,
            agi_digest_secret: config_data.agi_digest_secret,
            rustls_config,
            ldap_config,
        })
    }
}
