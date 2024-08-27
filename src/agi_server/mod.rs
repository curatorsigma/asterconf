use std::{fmt::Display, sync::Arc};

use async_trait::async_trait;
use blazing_agi::{
    command::AGICommand,
    connection::Connection,
    handler::{AGIHandler, AndThenHandler},
    router::Router,
    serve::serve,
    AGIError, AGIRequest,
};
use blazing_agi_macros::{create_handler, layer_before};
use rand::Rng;
use sha1::{Digest, Sha1};
use tokio::net::TcpListener;
use tracing::{event, Level};

use crate::{
    db::get_call_forwards_from_startpoint,
    types::{Config, Extension},
};

#[derive(Debug, Clone)]
enum SHA1DigestError {
    DecodeError,
    WrongDigest,
}
impl Display for SHA1DigestError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Self::DecodeError => {
                write!(f, "The returned digest was not decodable as u8")
            }
            Self::WrongDigest => {
                write!(f, "The returned digest is false")
            }
        }
    }
}
impl std::error::Error for SHA1DigestError {}

fn create_nonce() -> String {
    let mut raw_bytes = [0_u8; 20];
    // let mut raw_bytes: Vec<u8> = Vec::with_capacity(20);
    let now_in_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("Should be after the epoch");
    // 8 bytes against reuse
    raw_bytes[0..=7].clone_from_slice(&now_in_secs.as_secs().to_le_bytes());
    // 4 bytes against reuse
    raw_bytes[8..=11].clone_from_slice(&now_in_secs.subsec_millis().to_le_bytes());
    // 8 bytes against predictability
    rand::rngs::ThreadRng::default().fill(&mut raw_bytes[12..=19]);
    return hex::encode(raw_bytes);
}

#[derive(Clone)]
struct SHA1DigestOverAGI {
    secret: String,
}
impl SHA1DigestOverAGI {
    pub fn new<S: AsRef<str>>(secret: S) -> Self {
        Self {
            secret: secret.as_ref().to_string(),
        }
    }
}
#[async_trait]
impl AGIHandler for SHA1DigestOverAGI {
    // Note: this handler does not care about the request.
    // It simply ignores it and does the AGI digest.
    // This handler effectively works as a layer later)
    async fn handle(&self, connection: &mut Connection, _: &AGIRequest) -> Result<(), AGIError> {
        let nonce = create_nonce();
        let mut hasher = Sha1::new();
        hasher.update(self.secret.as_bytes());
        hasher.update(":".as_bytes());
        hasher.update(&nonce.as_bytes());
        let expected_digest: [u8; 20] = hasher.finalize().into();
        let digest_response = connection
            .send_command(AGICommand::GetFullVariable(
                format!("${{SHA1(${{BLAZING_AGI_DIGEST_SECRET}}:{})}}", nonce),
                None,
            ))
            .await?;
        if digest_response.code != 200 {
            return Err(AGIError::Not200(digest_response.code));
        };
        if let Some(x) = digest_response.operational_data {
            let digest_as_str = x.trim_matches(|c| c == '(' || c == ')');
            if expected_digest
                != *hex::decode(digest_as_str)
                    .map_err(|_| AGIError::InnerError(Box::new(SHA1DigestError::DecodeError)))?
            {
                event!(Level::WARN, "Expected Digest {}, got {}", hex::encode(expected_digest), digest_as_str);
                connection
                    .send_command(AGICommand::Verbose(
                        "Unauthenticated: Wrong Digest.".to_string(),
                    ))
                    .await?;
                Err(AGIError::InnerError(Box::new(SHA1DigestError::WrongDigest)))
            } else {
                Ok(())
            }
        } else {
            Err(AGIError::NoOperationalData(digest_response))
        }
    }
}

/// The route handler for call_forward

struct HandleCallForward {
    config: Arc<Config>,
}
impl HandleCallForward {
    pub fn new(config: Arc<Config>) -> Self {
        HandleCallForward { config }
    }
}
#[async_trait::async_trait]
impl AGIHandler for HandleCallForward {
    async fn handle(
        &self,
        connection: &mut Connection,
        request: &AGIRequest,
    ) -> Result<(), AGIError> {
        let dump = &request.variables;
        let initial_dest = dump
            .custom_args
            .get(&1)
            .ok_or(AGIError::NotEnoughCustomVariables(0, 2))?;
        let context_name = dump
            .custom_args
            .get(&2)
            .ok_or(AGIError::NotEnoughCustomVariables(1, 2))?;

        let call_forwards_from_src = get_call_forwards_from_startpoint(
            &self.config,
            &Extension::create_from_name(&self.config, initial_dest.to_string()),
        )
        .await
        .map_err(|e| AGIError::InnerError(Box::new(e)))?;

        // select the first call_forward which has the relevant context set
        // and use its destination
        for fwd in call_forwards_from_src.iter() {
            if fwd
                .in_contexts
                .iter()
                .any(|&x| x.asterisk_name == *context_name)
            {
                event!(
                    Level::INFO,
                    "Call to {initial_dest} forwarded to {}",
                    fwd.to.extension
                );
                connection
                    .send_command(AGICommand::SetVariable(
                        "CALL_FORWARDED_TO".to_string(),
                        fwd.to.extension.to_string(),
                    ))
                    .await?;
                return Ok(());
            } else {
                continue;
            };
        }
        // do not set a call forward, since no context matches
        // instead repeat the initial destination as the final destination
        event!(
            Level::INFO,
            "Call to {initial_dest} did not need forwarding."
        );
        connection
            .send_command(AGICommand::SetVariable(
                "CALL_FORWARDED_TO".to_string(),
                initial_dest.to_string(),
            ))
            .await?;
        return Ok(());
    }
}

pub async fn run_agi_server(config: Arc<Config>) -> Result<(), Box<dyn std::error::Error>> {
    let agi_listener = TcpListener::bind(config.agi_bind_string.clone()).await?;
    let router = Router::new()
        .route("/call_forward", HandleCallForward::new(config.clone()))
        .layer(layer_before!(SHA1DigestOverAGI::new(
            config.agi_digest_secret.clone()
        )));

    event!(Level::INFO, "AGI Server started listening on {}", config.agi_bind_string);
    serve(agi_listener, router).await?;
    Ok(())
}
