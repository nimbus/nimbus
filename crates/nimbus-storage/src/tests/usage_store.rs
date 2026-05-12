use super::*;

#[test]
fn usage_store_counts_unique_monthly_active_users_per_month() {
    let store = UsageStore::create_in_memory().expect("usage store should open");
    let march_10 = utc_unix_ms(2026, Month::March, 10);
    let march_20 = utc_unix_ms(2026, Month::March, 20);
    let april_2 = utc_unix_ms(2026, Month::April, 2);

    assert!(
        store
            .record_monthly_active_user("https://issuer.example.com|ada", march_10)
            .expect("first monthly active user should record"),
    );
    assert!(
        !store
            .record_monthly_active_user("https://issuer.example.com|ada", march_20)
            .expect("same user in same month should dedupe"),
    );
    assert!(
        store
            .record_monthly_active_user("https://issuer.example.com|grace", march_20)
            .expect("second user in same month should record"),
    );

    let march = store
        .monthly_active_users_for(march_20)
        .expect("march usage should load");
    assert_eq!(march.month, "2026-03");
    assert_eq!(march.monthly_active_users, 2);
    assert_eq!(march.last_recorded_at_unix_ms, Some(march_20));
    assert_eq!(
        store
            .distinct_identities_for_month(march_20)
            .expect("march identities should load"),
        vec![
            "https://issuer.example.com|ada".to_string(),
            "https://issuer.example.com|grace".to_string()
        ]
    );

    assert!(
        store
            .record_monthly_active_user("https://issuer.example.com|ada", april_2)
            .expect("same user in next month should count again"),
    );
    let april = store
        .monthly_active_users_for(april_2)
        .expect("april usage should load");
    assert_eq!(april.month, "2026-04");
    assert_eq!(april.monthly_active_users, 1);
}

fn utc_unix_ms(year: i32, month: Month, day: u8) -> u64 {
    let date = Date::from_calendar_date(year, month, day).expect("calendar date should build");
    let datetime = PrimitiveDateTime::new(date, Time::MIDNIGHT).assume_utc();
    u64::try_from(datetime.unix_timestamp_nanos() / 1_000_000)
        .expect("unix milliseconds should fit in u64")
}
