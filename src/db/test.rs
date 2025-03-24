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

/// noop test, but required for TLS auth in the other db tests
#[test]
fn __load_crypto_provider() {
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("Failed to install rustls crypto provider");
}

#[sqlx::test(fixtures("call_forward"))]
async fn get_call_forwards_from_startpoint(pool: PgPool) -> Result<(), Box<dyn std::error::Error>> {
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
async fn update_call_forward_change_dest(pool: PgPool) -> Result<(), Box<dyn std::error::Error>> {
    let mut config = Config::create().await?;
    config.pool = pool;

    let startpoint = Extension::create_from_name(&config, "702".to_string());
    let res = super::get_call_forwards_from_startpoint(&config, &startpoint).await?;
    let mut fwd = res
        .into_iter()
        .find(|f| f.to.extension == "something-external".to_owned())
        .unwrap();
    fwd.to = startpoint.clone();
    super::update_call_forward(&config, &fwd).await?;
    let res = super::get_call_forwards_from_startpoint(&config, &startpoint).await?;
    let by_id = super::get_call_forward_by_id(&config, fwd.fwd_id.into()).await?;
    assert_eq!(res.len(), 2);
    assert_eq!(res[0].to.extension, "702".to_string());
    assert_eq!(res[1].to.extension, "704".to_string());

    Ok(())
}

#[sqlx::test(fixtures("call_forward"))]
async fn update_call_forward_change_source(pool: PgPool) -> Result<(), Box<dyn std::error::Error>> {
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
async fn update_call_forward_add_context(pool: PgPool) -> Result<(), Box<dyn std::error::Error>> {
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
