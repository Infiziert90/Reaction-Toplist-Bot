use chrono::*;

pub fn parse_iso_week(week_param_opt: Option<&str>) -> Result<IsoWeek, Box<dyn std::error::Error>> {
    let week_param = week_param_opt.unwrap_or("+0");
    let first_char = week_param.chars().next().ok_or("week parameter empty")?;
    if first_char == '+' || first_char == '-' {
        let week_offset: i64 = week_param.parse()?;
        let today = Local::now();
        let target_day = today + Duration::weeks(week_offset);
        Ok(target_day.iso_week())
    } else {
        let (year_str, week_str) = week_param.split_once("-").ok_or("bad iso week format (expected `yyyy-ww`)")?;
        let date_opt = NaiveDate::from_isoywd_opt(year_str.parse()?, week_str.parse()?, Weekday::Mon);
        Ok(date_opt.ok_or("invalid ISO week")?.iso_week())
    }
}

pub fn iso_week_to_datetime(naive: IsoWeek) -> DateTime<Utc> {
    let naive_date = NaiveDate::from_isoywd_opt(naive.year(), naive.week(), Weekday::Mon).unwrap();
    DateTime::from_naive_utc_and_offset(naive_date.and_hms_opt(0, 0, 0).unwrap(), Utc)
}


/// Discord's epoch starts at "2015-01-01T00:00:00+00:00"
const DISCORD_EPOCH: u64 = 1_420_070_400_000;

/// Create a numeric snowflake (id) pretending to be created at the provided unix timestamp.
/// Intented for usage in time-relative APIs suchas serenity::GetMessages
///
/// When using as the lower end of a range, use `time_snowflake(…, false)`
/// to be exclusive, `- 1` to be inclusive.
///
/// When using as the higher end of a range, use `time_snowflake(…, true)`
/// to be exclusive, `+ 1` to be inclusive.
///
/// Inspired by discord.py:
/// https://github.com/Rapptz/discord.py/blob/dc50736bfc3340d7b999d9f165808f8dcb8f1a60/discord/utils.py#L373
pub fn time_snowflake(datetime: DateTime<Utc>, high: bool) -> u64 {
    let discord_millis = datetime.timestamp_millis() as u64 - DISCORD_EPOCH;
    (discord_millis << 22) + (if high { (1 << 22) - 1 } else { 0 })
}

/// Extract the timestamp of a snowflake (id).
pub fn snowflake_time<T: Into<u64>>(id: T) -> DateTime<Utc> {
    Utc.timestamp_millis_opt(((id.into() >> 22) + DISCORD_EPOCH) as i64).unwrap()
}

