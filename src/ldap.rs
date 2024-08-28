use axum_login::{AuthUser, AuthnBackend, UserId};
use ldap3::{LdapConnAsync, Scope, SearchEntry};
use serde::Deserialize;

/// Functions for accessing LDAP
#[derive(Clone)]
pub(crate) struct User {
    ldap_dn: String,
    pub(crate) username: String,
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
    // a hack: we add the redirect url we want to set here
    pub(crate) next: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct LDAPBackend {
    bound_handle: ldap3::Ldap,
    pub(crate) user_filter: String,
    pub(crate) base_dn: String,
    bind_dn: String,
    bind_pw: String,
}
impl LDAPBackend {
    pub async fn new(
        bind_string: &str,
        bind_dn: &str,
        bind_pw: &str,
        user_filter: &str,
        base_dn: &str,
    ) -> Result<Self, LDAPError> {
        let (conn, mut ldap) = LdapConnAsync::new(bind_string)
            .await
            .map_err(|_| LDAPError::CannotConnect)?;
        // spawn a task that drives the connection until ldap is dropped
        ldap3::drive!(conn);
        // LDAP-bind the handle
        ldap.simple_bind(bind_dn, bind_pw)
            .await
            .map_err(|_| LDAPError::CannotBind)?
            .success()
            .map_err(|e| LDAPError::UserError(e))?;
        Ok(LDAPBackend {
            bound_handle: ldap,
            user_filter: user_filter.to_string(),
            base_dn: base_dn.to_string(),
            bind_dn: bind_dn.to_string(),
            bind_pw: bind_pw.to_string(),
        })
    }

    // rebind as the search user
    pub async fn rebind(&self) -> Result<(), LDAPError> {
        let mut our_handle = self.bound_handle.clone();
        our_handle
            .simple_bind(&self.bind_dn, &self.bind_pw)
            .await
            .map_err(|_| LDAPError::CannotBind)?
            .success()
            .map_err(|e| LDAPError::UserError(e))?;
        Ok(())
    }
}

#[async_trait::async_trait]
impl AuthnBackend for LDAPBackend {
    type User = User;
    type Credentials = UserCredentials;
    type Error = LDAPError;
    async fn authenticate(&self, creds: UserCredentials) -> Result<Option<User>, LDAPError> {
        let user = match self.get_user(&creds.username).await? {
            Some(x) => x,
            None => {
                return Ok(None);
            }
        };
        // we now know that the user exists.
        // try to bind as that user
        // get a new handle and re-bind
        let mut rebind_handle = self.bound_handle.clone();
        let res = rebind_handle
            .simple_bind(&user.ldap_dn, &creds.password)
            // on a connection error, return Err(_)
            .await
            .map_err(|_| LDAPError::CannotBind)?
            .success()
            // if the password is wrong, return Ok(None), else Ok(Some(the-user))
            .map_or(Ok(None), |_| Ok(Some(user)))?;
        // we need to rebind as the search user
        self.rebind().await?;
        Ok(res)
    }

    async fn get_user(&self, id: &UserId<Self>) -> Result<Option<User>, LDAPError> {
        let mut our_handle = self.bound_handle.clone();
        let (rs, _res) = our_handle
            .search(
                &self.base_dn,
                Scope::OneLevel,
                &self.user_filter.replace("{username}", &id),
                vec!["uid", "userPassword"],
            )
            .await
            .map_err(|_| LDAPError::CannotSearch)?
            .success()
            .map_err(|x| LDAPError::UserError(x))?;
        if rs.len() == 0 {
            return Ok(None);
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
        Ok(Some(user))
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
        let mut backend = Config::create().await.unwrap().ldap_config;
        backend.bound_handle.unbind().await.unwrap();
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
                next: Some("/".to_string()),
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
                next: Some("/".to_string()),
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
                next: Some("/".to_string()),
            })
            .await
            .unwrap();
        assert!(res.is_none());
        let res = backend
            .authenticate(UserCredentials {
                username: "testuser".to_string(),
                password: "THIS IS NOT THE PASSWORD".to_string(),
                next: Some("/".to_string()),
            })
            .await
            .unwrap();
        assert!(res.is_none());
    }
}
