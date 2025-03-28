use sqlx::{PgPool, Row};
use time::{
    macros::{date, time},
    PrimitiveDateTime,
};

use crate::types::{
    CallForward, Config, Context, DayOfWeek, Extension, NoId, Timeframe, TimeframeDaily,
    TimeframeMonthly, TimeframeOnce, TimeframeWeekly,
};

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
    let res = super::get_call_forward_by_id(&config, 5).await.unwrap_err();
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
    super::new_call_forward(&config, forward).await.unwrap_err();
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

#[sqlx::test(fixtures("empty"))]
async fn test_insert_timeframe_once(pool: PgPool) -> Result<(), Box<dyn std::error::Error>> {
    let new_timeframe = TimeframeOnce::new(
        PrimitiveDateTime::new(date!(2023 - 01 - 15), time!(10:30)),
        PrimitiveDateTime::new(date!(2023 - 02 - 15), time!(10:45)),
    );
    let inserted_timeframe =
        super::insert_timeframe_once(&mut pool.acquire().await.unwrap(), new_timeframe).await?;
    assert_eq!(inserted_timeframe.id(), 1);
    Ok(())
}

#[sqlx::test(fixtures("empty"))]
async fn test_insert_timeframe_daily(pool: PgPool) -> Result<(), Box<dyn std::error::Error>> {
    let new_timeframe = TimeframeDaily::new(time!(10:30), time!(15:53));
    let inserted_timeframe =
        super::insert_timeframe_daily(&mut pool.acquire().await.unwrap(), new_timeframe).await?;
    assert_eq!(inserted_timeframe.id(), 1);
    Ok(())
}

#[sqlx::test(fixtures("empty"))]
async fn test_insert_timeframe_weekly(pool: PgPool) -> Result<(), Box<dyn std::error::Error>> {
    let new_timeframe = TimeframeWeekly::new(
        DayOfWeek::Monday,
        time!(10:30),
        DayOfWeek::Thursday,
        time!(15:53),
    );
    let inserted_timeframe =
        super::insert_timeframe_weekly(&mut pool.acquire().await.unwrap(), new_timeframe).await?;
    assert_eq!(inserted_timeframe.id(), 1);
    Ok(())
}

#[sqlx::test(fixtures("empty"))]
async fn test_insert_timeframe_monthly(pool: PgPool) -> Result<(), Box<dyn std::error::Error>> {
    let new_timeframe = TimeframeMonthly::new(12, time!(10:30), 18, time!(15:53));
    let inserted_timeframe =
        super::insert_timeframe_monthly(&mut pool.acquire().await.unwrap(), new_timeframe).await?;
    assert_eq!(inserted_timeframe.id(), 1);
    Ok(())
}

#[sqlx::test(fixtures("call_forward"))]
async fn insert_timeframe_enum(pool: PgPool) -> Result<(), Box<dyn std::error::Error>> {
    let timeframe_once = Timeframe::Once(TimeframeOnce::new(
        PrimitiveDateTime::new(date!(2023 - 01 - 15), time!(10:30)),
        PrimitiveDateTime::new(date!(2023 - 02 - 15), time!(10:45)),
    ));
    let timeframe_daily = Timeframe::Daily(TimeframeDaily::new(time!(10:30), time!(15:53)));
    let timeframe_weekly = Timeframe::Weekly(TimeframeWeekly::new(
        DayOfWeek::Monday,
        time!(10:30),
        DayOfWeek::Thursday,
        time!(15:53),
    ));
    let timeframe_monthly =
        Timeframe::Monthly(TimeframeMonthly::new(12, time!(10:30), 18, time!(15:53)));
    super::add_timeframe_to_forward(pool.clone(), 1, timeframe_once).await?;
    super::add_timeframe_to_forward(pool.clone(), 2, timeframe_daily).await?;
    super::add_timeframe_to_forward(pool.clone(), 3, timeframe_weekly).await?;
    super::add_timeframe_to_forward(pool.clone(), 4, timeframe_monthly).await?;

    assert_eq!(super::get_timeframes(pool.clone(), 1).await?.len(), 1);
    assert_eq!(super::get_timeframes(pool.clone(), 2).await?.len(), 1);
    assert_eq!(super::get_timeframes(pool.clone(), 3).await?.len(), 1);
    assert_eq!(super::get_timeframes(pool.clone(), 4).await?.len(), 1);
    Ok(())
}

#[sqlx::test(fixtures("call_forward"))]
async fn unlink_timeframes(pool: PgPool) -> Result<(), Box<dyn std::error::Error>> {
    let timeframe_once = Timeframe::Once(TimeframeOnce::new(
        PrimitiveDateTime::new(date!(2023 - 01 - 15), time!(10:30)),
        PrimitiveDateTime::new(date!(2023 - 02 - 15), time!(10:45)),
    ));
    let timeframe_daily = Timeframe::Daily(TimeframeDaily::new(time!(10:30), time!(15:53)));
    let timeframe_weekly = Timeframe::Weekly(TimeframeWeekly::new(
        DayOfWeek::Monday,
        time!(10:30),
        DayOfWeek::Thursday,
        time!(15:53),
    ));
    let timeframe_monthly =
        Timeframe::Monthly(TimeframeMonthly::new(12, time!(10:30), 18, time!(15:53)));

    let inserted_once = super::add_timeframe_to_forward(pool.clone(), 1, timeframe_once).await?;
    let inserted_daily = super::add_timeframe_to_forward(pool.clone(), 1, timeframe_daily).await?;
    let inserted_weekly =
        super::add_timeframe_to_forward(pool.clone(), 1, timeframe_weekly).await?;
    let inserted_monthly =
        super::add_timeframe_to_forward(pool.clone(), 2, timeframe_monthly).await?;

    assert_eq!(super::get_timeframes(pool.clone(), 1).await?.len(), 3);
    assert_eq!(super::get_timeframes(pool.clone(), 2).await?.len(), 1);

    super::unlink_timeframe_once(pool.clone(), inserted_once.id()).await?;
    super::unlink_timeframe_monthly(pool.clone(), inserted_monthly.id()).await?;

    assert_eq!(super::get_timeframes(pool.clone(), 1).await?.len(), 2);
    assert_eq!(super::get_timeframes(pool.clone(), 2).await?.len(), 0);

    super::unlink_timeframe_daily(pool.clone(), inserted_daily.id()).await?;
    super::unlink_timeframe_weekly(pool.clone(), inserted_weekly.id()).await?;

    assert_eq!(super::get_timeframes(pool.clone(), 1).await?.len(), 0);
    Ok(())
}

#[sqlx::test(fixtures("call_forward"))]
async fn update_timeframe(pool: PgPool) -> Result<(), Box<dyn std::error::Error>> {
    let timeframe_once = Timeframe::Once(TimeframeOnce::new(
        PrimitiveDateTime::new(date!(2023 - 01 - 15), time!(10:30)),
        PrimitiveDateTime::new(date!(2023 - 02 - 15), time!(10:45)),
    ));
    let timeframe_daily = Timeframe::Daily(TimeframeDaily::new(time!(10:30), time!(15:53)));
    let timeframe_weekly = Timeframe::Weekly(TimeframeWeekly::new(
        DayOfWeek::Monday,
        time!(10:30),
        DayOfWeek::Thursday,
        time!(15:53),
    ));
    let timeframe_monthly =
        Timeframe::Monthly(TimeframeMonthly::new(12, time!(10:30), 18, time!(15:53)));

    let mut inserted_once = match super::add_timeframe_to_forward(pool.clone(), 1, timeframe_once).await? {
        Timeframe::Once(x) => x,
        _ => { panic!() }
    };
    let mut inserted_daily = match super::add_timeframe_to_forward(pool.clone(), 1, timeframe_daily).await? {
        Timeframe::Daily(x) => x,
        _ => { panic!() }
    };
    let mut inserted_weekly =
        match super::add_timeframe_to_forward(pool.clone(), 1, timeframe_weekly).await? {
            Timeframe::Weekly(x) => x,
            _ => { panic!() }
        };
    let mut inserted_monthly =
        match super::add_timeframe_to_forward(pool.clone(), 1, timeframe_monthly).await? {
            Timeframe::Monthly(x) => x,
            _ => { panic!() }
        };

    inserted_once.end_time = PrimitiveDateTime::new(date!(2022 - 01 - 01), time!(12:47));
    inserted_daily.start_time = time!(12:47);
    inserted_weekly.start_dow = DayOfWeek::Tuesday;
    inserted_monthly.end_dom = 9;

    let mut con = pool.clone().acquire().await?;
    super::update_timeframe_once(&mut con, &inserted_once).await?;
    super::update_timeframe_daily(&mut con, &inserted_daily).await?;
    super::update_timeframe_weekly(&mut con, &inserted_weekly).await?;
    super::update_timeframe_monthly(&mut con, &inserted_monthly).await?;

    let res = super::get_timeframes(pool.clone(), 1).await?;
    assert_eq!(res.len(), 4);
    for tf in res {
        match tf {
            Timeframe::Once(x) => {
                assert_eq!(
                    x,
                    inserted_once);
            }
            Timeframe::Daily(x) => {
                assert_eq!(
                    x,
                    inserted_daily);
            }
            Timeframe::Weekly(x) => {
                assert_eq!(
                    x,
                    inserted_weekly);
            }
            Timeframe::Monthly(x) => {
                assert_eq!(
                    x,
                    inserted_monthly);
            }
        }
    };

    Ok(())
}

