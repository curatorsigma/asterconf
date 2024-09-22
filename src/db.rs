use std::error::Error;
/// Functions for reading and writing into the DB
use std::fmt::Display;

use sqlx::{postgres::PgRow, Row};
use tracing::Level;

use crate::types::{CallForward, Config, Context, Extension, HasId, NoId};

#[derive(Debug, PartialEq)]
pub enum DBError {
    CannotStartTransaction,
    CannotCommitTransaction,
    CannotRollbackTransaction,
    CannotInsertCallForward,
    CannotInsertContextMapping(String, i32),
    CannotSelectCallForwards,
    ContextDoesNotExist(String),
    CannotDeleteCallForward,
    CannotSelectCallForward(i32),
    CannotUpdateCallForwardDestination,
    CannotSelectContexts(i32),
    CannotDeleteContextMapping(String, i32),
    OverlappingCallForwards(Extension, Context),
}
impl Display for DBError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Self::CannotStartTransaction => {
                write!(f, "Unable to start transaction")
            }
            Self::CannotCommitTransaction => {
                write!(f, "Unable to commit transaction")
            }
            Self::CannotRollbackTransaction => {
                write!(f, "Unable to rollback transaction")
            }
            Self::CannotInsertCallForward => {
                write!(f, "Unable to insert a call forward")
            }
            Self::CannotInsertContextMapping(x, y) => {
                write!(
                    f,
                    "Unable to insert a context mapping fwd_id {x} to context {y}"
                )
            }
            Self::CannotSelectCallForwards => {
                write!(f, "Unable to select call forwards from starting extension")
            }
            Self::CannotSelectCallForward(x) => {
                write!(f, "Unable to select call forward from its id {x}")
            }
            Self::ContextDoesNotExist(x) => {
                write!(f, "The context with name {x} does not exit in the config")
            }
            Self::CannotDeleteCallForward => {
                write!(f, "Unable to delete call forward")
            }
            Self::CannotUpdateCallForwardDestination => {
                write!(f, "Unable to update the destination of a call forward")
            }
            Self::CannotSelectContexts(x) => {
                write!(f, "Unable to select contexts for call forward {x}")
            }
            Self::CannotDeleteContextMapping(x, y) => {
                write!(f, "Unable to delete context {x} for call forward {y}")
            }
            Self::OverlappingCallForwards(exten, context) => {
                write!(
                    f,
                    "There is already a call forward from {exten} active in context {context}."
                )
            }
        }
    }
}
impl Error for DBError {}

/// Set or Update a call forward.
///
/// Note that call forwards contain data for the Contexts in which they are relevant
/// No two call forwards from the same extension can be applicable in the same context.
/// This function returns DBError::OverlappingCallForwards if this happens.
///
/// If there is no conflicting context, this function may create another call forward from the same
/// Extension that already has another (in other contexts)
#[tracing::instrument(level=Level::DEBUG,skip_all,err)]
pub async fn new_call_forward<'a>(
    config: &Config,
    new_forward: CallForward<'a, NoId>,
) -> Result<CallForward<'a, HasId>, DBError> {
    let existing_forwards = get_call_forwards_from_startpoint(config, &new_forward.from).await?;
    for fwd in existing_forwards {
        if let Some(overlap) = fwd.intersecting_contexts(&new_forward).next() {
            return Err(DBError::OverlappingCallForwards(
                fwd.from.clone(),
                (*overlap).clone(),
            ));
        };
    }

    // The good case: there are no overlapping call forwards with new_forward
    let mut tx = config
        .pool
        .begin()
        .await
        .map_err(|_| DBError::CannotStartTransaction)?;
    let new_id_result = sqlx::query(
        "INSERT INTO call_forward (from_extension, to_extension) VALUES ($1, $2) RETURNING fwd_id",
    )
    .bind(&new_forward.from.extension)
    .bind(&new_forward.to.extension)
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| DBError::CannotInsertCallForward)?;

    let new_id: i32 = new_id_result.get("fwd_id");

    for ctx in new_forward.in_contexts.iter() {
        sqlx::query("INSERT INTO map_call_forward_context (fwd_id, context) VALUES ($1, $2)")
            .bind(new_id)
            .bind(&ctx.asterisk_name)
            .execute(&mut *tx)
            .await
            .map_err(|_| DBError::CannotInsertContextMapping(ctx.asterisk_name.clone(), new_id))?;
    }
    tx.commit()
        .await
        .map_err(|_| DBError::CannotCommitTransaction)?;
    Ok(new_forward.set_id(new_id))
}

fn convert_to_call_forwards(
    config: &Config,
    call_forwards: Vec<PgRow>,
) -> Result<Vec<CallForward<HasId>>, DBError> {
    let mut result: Vec<CallForward<HasId>> = vec![];
    'row: for row in call_forwards {
        let fwd_id: i32 = row.get("fwd_id");
        let from_extension: String = row.get("from_extension");
        let to_extension: String = row.get("to_extension");
        let context: String = row.get("context");
        for fwd in result.iter_mut() {
            if fwd.to.extension == to_extension && fwd.from.extension == from_extension {
                let context_as_object = Context::create_from_name(config, &context);
                match context_as_object {
                    None => {
                        return Err(DBError::ContextDoesNotExist(context));
                    }
                    Some(x) => {
                        fwd.in_contexts.push(x);
                        continue 'row;
                    }
                };
            };
        }
        // this destination has no call forward already set
        // so we add a new call forward into result
        result.push(CallForward::<HasId>::new(
            config,
            from_extension,
            to_extension,
            vec![context],
            fwd_id,
        )?);
    }
    Ok(result)
}

/// Get all call forwards that start at `startpoint`
#[tracing::instrument(level=Level::DEBUG,skip(config),err)]
pub async fn get_all_call_forwards<'a>(
    config: &'a Config,
) -> Result<Vec<CallForward<'a, HasId>>, DBError> {
    let call_forwards = sqlx::query(
        "SELECT call_forward.fwd_id, call_forward.from_extension, call_forward.to_extension, map_call_forward_context.context
            FROM call_forward
         INNER JOIN map_call_forward_context
            ON map_call_forward_context.fwd_id = call_forward.fwd_id"
    )
        .fetch_all(&config.pool)
        .await
        .map_err(|_| DBError::CannotSelectCallForwards)?;
    convert_to_call_forwards(config, call_forwards)
}

/// Get all call forwards that start at `startpoint`
#[tracing::instrument(level=Level::DEBUG,skip(config),err)]
pub async fn get_call_forwards_from_startpoint<'a>(
    config: &'a Config,
    startpoint: &Extension,
) -> Result<Vec<CallForward<'a, HasId>>, DBError> {
    let call_forwards = sqlx::query(
        "SELECT call_forward.fwd_id, call_forward.from_extension, call_forward.to_extension, map_call_forward_context.context
            FROM call_forward
         INNER JOIN map_call_forward_context
            ON map_call_forward_context.fwd_id = call_forward.fwd_id
         WHERE from_extension = $1"
    )
        .bind(startpoint.extension.clone())
        .fetch_all(&config.pool)
        .await
        .map_err(|_| DBError::CannotSelectCallForwards)?;
    convert_to_call_forwards(config, call_forwards)
}

/// Get call forward with a specific id
#[tracing::instrument(level=Level::DEBUG,skip(config),err)]
pub async fn get_call_forward_by_id<'a>(
    config: &'a Config,
    fwdid: i32,
) -> Result<CallForward<'a, HasId>, DBError> {
    let call_forwards = sqlx::query(
        "SELECT call_forward.fwd_id, call_forward.from_extension, call_forward.to_extension, map_call_forward_context.context
            FROM call_forward
         INNER JOIN map_call_forward_context
            ON map_call_forward_context.fwd_id = call_forward.fwd_id
        WHERE
            call_forward.fwd_id = $1"
    )
        .bind(fwdid)
        .fetch_all(&config.pool)
        .await
        .map_err(|_| DBError::CannotSelectCallForwards)?;
    let forwards = convert_to_call_forwards(config, call_forwards)?;
    if forwards.len() == 1 {
        Ok(forwards
            .into_iter()
            .next()
            .expect("Length was just checked"))
    } else {
        Err(DBError::CannotSelectCallForward(fwdid))
    }
}

/// Remove a given call forward
#[tracing::instrument(level=Level::DEBUG,skip(config),err)]
pub async fn delete_call_forward_by_id<'a>(config: &'a Config, fwd_id: i32) -> Result<(), DBError> {
    let mut tx = config
        .pool
        .begin()
        .await
        .map_err(|_| DBError::CannotStartTransaction)?;
    sqlx::query("DELETE FROM call_forward WHERE fwd_id = $1")
        .bind(fwd_id)
        .execute(&mut *tx)
        .await
        .map_err(|_| DBError::CannotDeleteCallForward)?;
    tx.commit()
        .await
        .map_err(|_| DBError::CannotCommitTransaction)?;
    Ok(())
}

#[tracing::instrument(level=Level::DEBUG,skip(config),err)]
pub async fn update_call_forward<'a>(
    config: &'a Config,
    forward: &CallForward<'a, HasId>,
) -> Result<(), DBError> {
    // update the contexts
    //  get the contexts currently in the DB
    //  calculate the diff
    //  apply the diff
    let mut tx = config
        .pool
        .begin()
        .await
        .map_err(|_| DBError::CannotStartTransaction)?;

    // make sure the call forward actually exists
    let res = sqlx::query("SELECT COUNT(*) AS count FROM call_forward WHERE fwd_id = $1")
        .bind(Into::<i32>::into(forward.fwd_id))
        .fetch_one(&mut *tx)
        .await
        .map_err(|_| DBError::CannotSelectCallForward(Into::<i32>::into(forward.fwd_id)))?;
    if res.get::<i64, &str>("count") != 1 {
        return Err(DBError::CannotSelectCallForward(Into::<i32>::into(
            forward.fwd_id,
        )));
    }

    // Update the source
    sqlx::query("UPDATE call_forward SET from_extension = $1 WHERE fwd_id = $2")
        .bind(&forward.from.extension)
        .bind(Into::<i32>::into(forward.fwd_id))
        .execute(&mut *tx)
        .await
        .map_err(|_| DBError::CannotUpdateCallForwardDestination)?;

    // Update the target
    sqlx::query("UPDATE call_forward SET to_extension = $1 WHERE fwd_id = $2")
        .bind(&forward.to.extension)
        .bind(Into::<i32>::into(forward.fwd_id))
        .execute(&mut *tx)
        .await
        .map_err(|_| DBError::CannotUpdateCallForwardDestination)?;

    // Get the contexts currently in the DB
    let context_res: Vec<String> =
        sqlx::query("SELECT context FROM map_call_forward_context WHERE fwd_id = $1")
            .bind(Into::<i32>::into(forward.fwd_id))
            .map(|row: PgRow| row.get("context"))
            .fetch_all(&mut *tx)
            .await
            .map_err(|_| DBError::CannotSelectContexts(Into::<i32>::into(forward.fwd_id)))?;

    // The contexts that are set in forward, but not yet in the DB
    let contexts_to_set = forward.in_contexts.iter().filter_map(|x| {
        if context_res.contains(&x.asterisk_name) {
            None
        } else {
            Some(x.asterisk_name.clone())
        }
    });
    // Insert the contexts which are new
    for ctx_to_set in contexts_to_set {
        sqlx::query("INSERT INTO map_call_forward_context (fwd_id, context) VALUES ($1, $2)")
            .bind(Into::<i32>::into(forward.fwd_id))
            .bind(&ctx_to_set)
            .execute(&mut *tx)
            .await
            .map_err(|_| {
                DBError::CannotInsertContextMapping(
                    ctx_to_set.clone(),
                    Into::<i32>::into(forward.fwd_id),
                )
            })?;
    }

    // The contexts that are in the DB, but not in forward anymore
    let contexts_to_delete = context_res.iter().filter(|&x| {
        forward
            .in_contexts
            .iter()
            .any(|ctx| ctx.asterisk_name == *x)
    });
    // Delete the contexts which are no longer required
    for ctx_to_delete in contexts_to_delete {
        sqlx::query("DELETE FROM map_call_forward_context WHERE fwd_id = $1 and context = $2 ")
            .bind(Into::<i32>::into(forward.fwd_id))
            .bind(ctx_to_delete)
            .execute(&mut *tx)
            .await
            .map_err(|_| {
                DBError::CannotDeleteContextMapping(
                    ctx_to_delete.clone(),
                    Into::<i32>::into(forward.fwd_id),
                )
            })?;
    }

    tx.commit()
        .await
        .map_err(|_| DBError::CannotCommitTransaction)?;
    Ok(())
}

#[cfg(test)]
mod db_tests {
    use sqlx::{PgPool, Row};

    use crate::types::{CallForward, Config, Context, Extension, NoId};

    #[sqlx::test]
    async fn auth_test(pool: PgPool) -> sqlx::Result<()> {
        let mut conn = pool.acquire().await?;

        let foo = sqlx::query("SELECT 1 + 1 AS sum")
            .fetch_one(&mut *conn)
            .await?
            .get::<i32, &str>("sum");
        assert_eq!(foo, 2);
        Ok(())
    }

    #[sqlx::test(fixtures("call_forward"))]
    async fn get_call_forwards_from_startpoint(
        pool: PgPool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut config = Config::create().await?;
        config.pool = pool;

        let startpoint = Extension::create_from_name(&config, "702".to_string());
        let res = super::get_call_forwards_from_startpoint(&config, &startpoint).await?;
        assert_eq!(res.len(), 2);
        assert_eq!(res[0].in_contexts.len(), 2);
        Ok(())
    }

    #[sqlx::test(fixtures("call_forward"))]
    async fn get_all_call_forwards(pool: PgPool) -> Result<(), Box<dyn std::error::Error>> {
        let mut config = Config::create().await?;
        config.pool = pool;

        let res = super::get_all_call_forwards(&config).await?;
        assert_eq!(res.len(), 4);
        assert_eq!(res[0].in_contexts.len(), 2);
        Ok(())
    }

    #[sqlx::test(fixtures("call_forward"))]
    async fn get_call_forward_by_id(pool: PgPool) -> Result<(), Box<dyn std::error::Error>> {
        let mut config = Config::create().await?;
        config.pool = pool;

        let res = super::get_call_forward_by_id(&config, 2).await?;
        assert_eq!(res.in_contexts.len(), 2);
        let res = super::get_call_forward_by_id(&config, 5).await;
        assert_eq!(res, Err(super::DBError::CannotSelectCallForward(5)));
        Ok(())
    }

    #[sqlx::test]
    async fn insert_call_forward(pool: PgPool) -> Result<(), Box<dyn std::error::Error>> {
        let mut config = Config::create().await?;
        config.pool = pool;

        let forward = CallForward::<NoId>::new(
            &config,
            "702".to_string(),
            "12341234".to_string(),
            vec!["from_external".to_string()],
        )?;
        super::new_call_forward(&config, forward).await?;
        Ok(())
    }

    #[sqlx::test(fixtures("call_forward"))]
    async fn insert_conflicting_context(pool: PgPool) -> Result<(), Box<dyn std::error::Error>> {
        let mut config = Config::create().await?;
        config.pool = pool;

        let forward = CallForward::<NoId>::new(
            &config,
            "702".to_string(),
            "12341234".to_string(),
            vec!["from_external".to_string()],
        )?;
        let newly_inserted = super::new_call_forward(&config, forward).await;
        assert_eq!(
            newly_inserted,
            Err(super::DBError::OverlappingCallForwards(
                Extension::create_from_name(&config, "702".to_string()),
                Context::create_from_name(&config, "from_external".to_string())
                    .unwrap()
                    .clone()
            ))
        );
        Ok(())
    }

    #[sqlx::test(fixtures("call_forward"))]
    async fn delete_call_forward(pool: PgPool) -> Result<(), Box<dyn std::error::Error>> {
        let mut config = Config::create().await?;
        config.pool = pool;

        let startpoint = Extension::create_from_name(&config, "702".to_string());
        let mut res = super::get_call_forwards_from_startpoint(&config, &startpoint).await?;
        let first_len = res.len();
        let to_delete = res.pop().unwrap();
        super::delete_call_forward_by_id(&config, to_delete.fwd_id.into()).await?;
        let res = super::get_call_forwards_from_startpoint(&config, &startpoint).await?;
        assert_eq!(first_len - 1, res.len());
        Ok(())
    }

    #[sqlx::test(fixtures("call_forward"))]
    async fn update_call_forward_change_dest(
        pool: PgPool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut config = Config::create().await?;
        config.pool = pool;

        let startpoint = Extension::create_from_name(&config, "702".to_string());
        let mut res = super::get_call_forwards_from_startpoint(&config, &startpoint).await?;
        let mut fwd = res.pop().unwrap();
        fwd.to = startpoint.clone();
        super::update_call_forward(&config, &fwd).await?;
        let res = super::get_call_forwards_from_startpoint(&config, &startpoint).await?;
        assert_eq!(res.last().unwrap().to.extension, "702".to_string());

        Ok(())
    }

    #[sqlx::test(fixtures("call_forward"))]
    async fn update_call_forward_change_source(
        pool: PgPool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut config = Config::create().await?;
        config.pool = pool;

        let startpoint = Extension::create_from_name(&config, "704".to_string());
        let mut res = super::get_call_forwards_from_startpoint(&config, &startpoint).await?;
        let mut fwd = res.pop().unwrap();
        fwd.from = startpoint.clone();
        super::update_call_forward(&config, &fwd).await?;
        let res = super::get_call_forwards_from_startpoint(&config, &startpoint).await?;
        assert_eq!(res.last().unwrap().from.extension, "704".to_string());

        Ok(())
    }

    #[sqlx::test(fixtures("call_forward"))]
    async fn update_call_forward_add_context(
        pool: PgPool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut config = Config::create().await?;
        config.pool = pool;

        let startpoint = Extension::create_from_name(&config, "702".to_string());
        let from_sales = Context::create_from_name(&config, "from_sales").unwrap();

        let mut res = super::get_call_forwards_from_startpoint(&config, &startpoint).await?;
        let mut fwd = res.pop().unwrap();

        assert!(!fwd.in_contexts.contains(&from_sales));
        fwd.in_contexts.push(from_sales);
        super::update_call_forward(&config, &fwd).await?;
        let res = super::get_call_forwards_from_startpoint(&config, &startpoint).await?;
        assert!(res.last().unwrap().in_contexts.contains(&from_sales));

        Ok(())
    }

    #[sqlx::test(fixtures("call_forward"))]
    async fn update_call_forward_delete_context(
        pool: PgPool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut config = Config::create().await?;
        config.pool = pool;

        let startpoint = Extension::create_from_name(&config, "702".to_string());
        let mut res = super::get_call_forwards_from_startpoint(&config, &startpoint).await?;
        let mut fwd = res.pop().unwrap();
        let from_internal = Context::create_from_name(&config, "from_internal").unwrap();
        assert!(fwd.in_contexts.contains(&from_internal));

        // the last thing inserted is the from_internal forwarding
        fwd.in_contexts.pop();
        super::update_call_forward(&config, &fwd).await?;
        let res = super::get_call_forwards_from_startpoint(&config, &startpoint).await?;
        assert!(!res.last().unwrap().in_contexts.contains(&from_internal));

        Ok(())
    }
}
