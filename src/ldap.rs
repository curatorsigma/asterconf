use axum_login::{AuthUser, AuthnBackend, UserId};
use ldap3::{Ldap, LdapConnAsync, Scope, SearchEntry};
use serde::Deserialize;
use tracing::{warn, Level};

/// escape parameter such that it may be used in a search filter
/// uses RFC2254 Section 4 and RFC4514 Section 2.4
///
/// NOTE: ldap3 also has ldap3::ldap_escape,
/// but that does not escape potentially dangerous characters, and only
/// does the escape for the RFC2254-mandated chars: ()*\NUL
fn escape_ldap_search_filter_parameter(parameter: &str) -> String {
    let mut res = String::new();
    for c in parameter.chars() {
        match c {
            // mandated
            '(' => res.push_str("\\28"),
            ')' => res.push_str("\\29"),
            '*' => res.push_str("\\2a"),
            '\\' => res.push_str("\\5c"),
            '\0' => res.push_str("\\00"),
            // for safety against LDAP injections - these are
            // the characters to be encoded by RFC4514
            '"' => res.push_str("\\22"),
            '#' => res.push_str("\\23"),
            '+' => res.push_str("\\2b"),
            ',' => res.push_str("\\2c"),
            ';' => res.push_str("\\3b"),
            '<' => res.push_str("\\3c"),
            '=' => res.push_str("\\3d"),
            '>' => res.push_str("\\3e"),
            '|' => res.push_str("\\7c"),
            ' ' => res.push_str("\\20"),
            x => res.push(x),
        };
    }
    return res;
}

/// Functions for accessing LDAP
#[derive(Clone)]
pub(crate) struct User {
    /// the full dn used in LDAP
    ldap_dn: String,
    /// the uid in LDAP
    pub(crate) username: String,
    /// The password hash as stored in LDAP
    password_hash: String,
}
impl std::fmt::Debug for User {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.debug_struct("User")
            .field("username", &self.username)
            .field("password_hash", &"[redacted]")
            .finish()
    }
}

impl AuthUser for User {
    type Id = String;
    fn id(&self) -> Self::Id {
        self.username.clone()
    }
    fn session_auth_hash(&self) -> &[u8] {
        self.password_hash.as_bytes()
    }
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct UserCredentials {
    pub username: String,
    pub password: String,
}

#[derive(Clone)]
pub(crate) struct LDAPBackend {
    /// String defining the ldaps server to bind against
    bind_string: String,
    /// filter to search for users. Contains {username} which will be replaced
    pub(crate) user_filter: String,
    /// the base dn under which users lie
    pub(crate) base_dn: String,
    /// dn and password of the search user
    bind_dn: String,
    bind_pw: String,
}
impl std::fmt::Debug for LDAPBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.debug_struct("LDAPBackend")
            .field("bind_string", &self.bind_string)
            .field("bind_dn", &self.bind_dn)
            .field("base_dn", &self.base_dn)
            .field("user_filter", &self.user_filter)
            .field("bind_pw", &"[redacted]")
            .finish()
    }
}
impl LDAPBackend {
    #[tracing::instrument(level=Level::DEBUG,skip_all,err)]
    pub async fn new(
        hostname: &str,
        port: u16,
        bind_dn: &str,
        bind_pw: &str,
        user_filter: &str,
        base_dn: &str,
    ) -> Result<Self, LDAPError> {
        let bind_string = format!("ldaps://{hostname}:{port}");
        Ok(LDAPBackend {
            bind_string,
            user_filter: user_filter.to_string(),
            base_dn: base_dn.to_string(),
            bind_dn: bind_dn.to_string(),
            bind_pw: bind_pw.to_string(),
        })
    }

    async fn new_bound_connection(&self) -> Result<Ldap, LDAPError> {
        let (conn, mut ldap) = LdapConnAsync::new(&self.bind_string)
            .await
            .map_err(|_| LDAPError::CannotConnect)?;
        // spawn a task that drives the connection until ldap is dropped
        ldap3::drive!(conn);
        // LDAP-bind the handle
        ldap.simple_bind(&self.bind_dn, &self.bind_pw)
            .await
            .map_err(|_| LDAPError::CannotBind)?
            .success()
            .map_err(|e| LDAPError::UserError(e))?;
        Ok(ldap)
    }

    /// Bind, get a user (potentially) and DO NOT UNBIND, returning the (still live and bound)
    /// connection on success
    async fn get_user_no_unbind(&self, id: &str) -> Result<(Ldap, Option<User>), LDAPError> {
        let mut our_handle = self.new_bound_connection().await?;
        let (rs, _res) = our_handle
            .search(
                &self.base_dn,
                Scope::OneLevel,
                &self
                    .user_filter
                    .replace("{username}", &escape_ldap_search_filter_parameter(id)),
                vec!["uid", "userPassword"],
            )
            .await
            .map_err(|_| LDAPError::CannotSearch)?
            .success()
            .map_err(|x| LDAPError::UserError(x))?;
        if rs.len() == 0 {
            return Ok((our_handle, None));
        }
        if rs.len() != 1 {
            return Err(LDAPError::MultipleUsersWithSameUid(id.to_string()));
        }
        let user_obj = SearchEntry::construct(
            rs.into_iter()
                .next()
                .expect("Should have checked that we got a user"),
        );

        let uids = user_obj
            .attrs
            .get("uid")
            .ok_or(LDAPError::AttributeMissing("uid".to_string()))?;
        let uid = if uids.len() != 1 {
            return Err(LDAPError::NotExactlyOneOfAttribute("uid".to_string()));
        } else {
            uids.into_iter()
                .next()
                .expect("In else if if len() != 1")
                .to_string()
        };

        let password_hashes = user_obj
            .attrs
            .get("userPassword")
            .ok_or(LDAPError::AttributeMissing("userPassword".to_string()))?;
        let password_hash = if uids.len() != 1 {
            return Err(LDAPError::NotExactlyOneOfAttribute(
                "userPassword".to_string(),
            ));
        } else {
            password_hashes
                .into_iter()
                .next()
                .expect("In else of if len() != 1")
                .to_string()
        };

        let user = User {
            ldap_dn: user_obj.dn,
            username: uid,
            password_hash,
        };
        Ok((our_handle, Some(user)))
    }
}

#[async_trait::async_trait]
impl AuthnBackend for LDAPBackend {
    type User = User;
    type Credentials = UserCredentials;
    type Error = LDAPError;
    #[tracing::instrument(level=Level::DEBUG,skip_all,err)]
    async fn authenticate(&self, creds: UserCredentials) -> Result<Option<User>, LDAPError> {
        let (mut handle, user) = self.get_user_no_unbind(&creds.username).await?;
        let user = match user {
            Some(x) => x,
            None => {
                warn!(
                    "User {} tried logging in but was not found via the search filter {}",
                    creds.username, self.user_filter
                );
                return Ok(None);
            }
        };
        // we now know that the user exists.
        // try to bind as that user
        // get a new handle and re-bind
        // we need to rebind as the search user
        let res = handle
            .simple_bind(&user.ldap_dn, &creds.password)
            // on a connection error, return Err(_)
            .await
            .map_err(|_| LDAPError::CannotBind)?
            .success()
            // if the password is wrong, return Ok(None), else Ok(Some(the-user))
            .map_or(Ok(None), |_| Ok(Some(user)))?;
        // unbind to cleanly exit the ldap session
        handle.unbind().await.map_err(|_| LDAPError::CannotUnbind)?;
        Ok(res)
    }

    #[tracing::instrument(level=Level::DEBUG,skip_all,err)]
    async fn get_user(&self, id: &UserId<Self>) -> Result<Option<User>, LDAPError> {
        let (mut handle, res) = self.get_user_no_unbind(id).await?;
        // unbind to cleanly exit the ldap session
        handle.unbind().await.map_err(|_| LDAPError::CannotUnbind)?;
        Ok(res)
    }
}
#[derive(Debug)]
pub enum LDAPError {
    CannotConnect,
    CannotUnbind,
    CannotBind,
    CannotSearch,
    UserError(ldap3::LdapError),
    MultipleUsersWithSameUid(String),
    AttributeMissing(String),
    NotExactlyOneOfAttribute(String),
}
impl std::fmt::Display for LDAPError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Self::CannotConnect => {
                write!(f, "Cannot connect to the LDAPS host")
            }
            Self::CannotUnbind => {
                write!(f, "Cannot unbind from LDAP")
            }
            Self::CannotBind => {
                write!(f, "Cannot bind to LDAP")
            }
            Self::CannotSearch => {
                write!(f, "Cannot search in the LDAP directory")
            }
            Self::UserError(x) => {
                write!(f, "Error while executing command: {x}")
            }
            Self::MultipleUsersWithSameUid(x) => {
                write!(f, "There were multiple users with the uid {x}")
            }
            Self::AttributeMissing(x) => {
                write!(f, "The attribute {x} is missing")
            }
            Self::NotExactlyOneOfAttribute(x) => {
                write!(f, "There is not exactly one value for attribute {x}")
            }
        }
    }
}
impl std::error::Error for LDAPError {}

/// Note: we assume that testuser is present in the LDAP server here.
/// The password has to be added as ASTERCONF_TESTUSER_PASSWORD in .env
///
/// If you cannot/do not want this, simply do not run these tests (they are ignored by default)
#[cfg(test)]
mod ldap_test {
    use axum_login::AuthnBackend;
    use dotenv::dotenv;

    use super::*;
    use crate::types::Config;

    /// Ensure that your config.yaml has the correct credentials for your LDAP databse
    #[tokio::test]
    #[ignore]
    async fn ldap_bind() {
        let backend = Config::create().await.unwrap().ldap_config;
        let mut handle = backend.new_bound_connection().await.unwrap();
        handle.unbind().await.unwrap();
    }

    #[tokio::test]
    #[ignore]
    async fn ldap_get_user() {
        let backend = Config::create().await.unwrap().ldap_config;
        let res = backend.get_user(&"testuser".to_string()).await.unwrap();
        res.unwrap();
    }

    #[tokio::test]
    #[ignore]
    async fn ldap_get_user_does_not_exist() {
        let backend = Config::create().await.unwrap().ldap_config;
        let res = backend
            .get_user(&"DOES NOT EXIST EVEN REMOTELY".to_string())
            .await
            .unwrap();
        assert!(res.is_none());
    }

    #[tokio::test]
    #[ignore]
    async fn ldap_authenticate_user() {
        let backend = Config::create().await.unwrap().ldap_config;
        dotenv().ok();
        let res = backend
            .authenticate(UserCredentials {
                username: "testuser".to_string(),
                password: std::env::var("ASTERCONF_TESTUSER_PASSWORD").unwrap(),
            })
            .await
            .unwrap();
        assert!(res.is_some());
    }

    #[tokio::test]
    #[ignore]
    async fn ldap_authenticate_user_password_wrong() {
        let backend = Config::create().await.unwrap().ldap_config;
        let res = backend
            .authenticate(UserCredentials {
                username: "testuser".to_string(),
                password: "THIS IS NOT THE PASSWORD".to_string(),
            })
            .await
            .unwrap();
        assert!(res.is_none());
    }

    #[tokio::test]
    #[ignore]
    async fn ldap_auth_user_twice() {
        let backend = Config::create().await.unwrap().ldap_config;
        let res = backend
            .authenticate(UserCredentials {
                username: "testuser".to_string(),
                password: "THIS IS NOT THE PASSWORD".to_string(),
            })
            .await
            .unwrap();
        assert!(res.is_none());
        let res = backend
            .authenticate(UserCredentials {
                username: "testuser".to_string(),
                password: "THIS IS NOT THE PASSWORD".to_string(),
            })
            .await
            .unwrap();
        assert!(res.is_none());
    }
}
