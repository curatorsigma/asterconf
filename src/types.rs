use std::fs::File;
use std::path::Path;
use std::{collections::HashMap, fmt::Display};

/// Structs used by the other components
use serde::Deserialize;
use sqlx::PgPool;

use crate::db::DBError;

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

#[derive(Debug, PartialEq)]
pub(crate) struct NoId {}
impl IdState for NoId {}

#[derive(Debug, PartialEq)]
pub(crate) struct HasId {
    id: i32,
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
}
impl<'a> CallForward<'a, NoId> {
    pub fn new(
        config: &'a Config,
        from: String,
        to: String,
        in_contexts: Vec<String>,
    ) -> Result<CallForward<'a, NoId>, DBError> {
        let from_as_exten = Extension::create_from_name(&config, from);
        let to_as_exten = Extension::create_from_name(&config, to);
        let mut contexts_as_contexts: Vec<&Context> = vec![];
        for ctx in in_contexts.into_iter() {
            match Context::create_from_name(&config, &ctx) {
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

#[derive(Deserialize, Debug)]
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
    agi_bind_addr: String,
    agi_bind_port: String,
    agi_digest_secret: String,
}

#[derive(Debug, Clone)]
pub struct Config {
    // extension name to Extension
    pub(crate) extensions: HashMap<String, Extension>,
    // context name to Context
    pub(crate) contexts: HashMap<String, Context>,
    // db connection pool
    pub(crate) pool: PgPool,
    // addr:port to bind the webserver to
    pub(crate) web_bind_string: String,
    // addr:port to bind agi server to
    pub(crate) agi_bind_string: String,
    // the secret used in the SHA digest
    pub(crate) agi_digest_secret: String,
}
impl Config {
    // this will never be called inside the actual application (only during setup)
    // so I don't care about proper error handling
    // TODO: this needs to log its own errors, because it is called in lazy_static
    pub async fn create() -> Result<Config, Box<dyn std::error::Error>> {
        let config_path = Path::new("config.yaml");
        let f = File::open(config_path)?;
        let config_data: ConfigFileData = serde_yaml::from_reader(f)?;
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
        let pool = sqlx::postgres::PgPool::connect(&url).await?;
        // webserver settings
        // TODO: tls settings
        let web_bind_string = format!(
            "{}:{}",
            config_data.web_bind_addr, config_data.web_bind_port
        );
        let agi_bind_string = format!(
            "{}:{}",
            config_data.agi_bind_addr, config_data.agi_bind_port
        );
        Ok(Config {
            extensions,
            contexts,
            pool,
            web_bind_string,
            agi_bind_string,
            agi_digest_secret: config_data.agi_digest_secret,
        })
    }
}
