/// Tests for the different components

#[test]
fn order_on_timeframes() {
    let timeframe_once = Timeframe::Once(TimeframeOnce::new(
        PrimitiveDateTime::new(date!(2023 - 01 - 15), time!(10:30)).assume_utc(),
        PrimitiveDateTime::new(date!(2023 - 02 - 15), time!(10:45)).assume_utc(),
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

    assert!(timeframe_once < timeframe_daily);
    assert!(timeframe_daily < timeframe_weekly);
    assert!(timeframe_weekly < timeframe_monthly);
    // NOTE: ordering within a type is irrelevant
}
