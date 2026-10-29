use crate::i18n::UiLanguage;

#[derive(Clone, Copy)]
struct DateTime {
    year: u16,
    month: u16,
    day: u16,
    hour: u16,
    minute: u16,
    second: u16,
}

pub(super) fn compact(timestamp: u64, language: UiLanguage) -> String {
    local_time(timestamp)
        .map(|date| format_date(date, language, false))
        .unwrap_or_else(|| language.text("Date unavailable").into())
}

pub(super) fn full(timestamp: u64, language: UiLanguage) -> String {
    local_time(timestamp)
        .map(|date| format_date(date, language, true))
        .unwrap_or_else(|| language.text("Date unavailable").into())
}

fn format_date(date: DateTime, language: UiLanguage, full: bool) -> String {
    let (first, second) = if language == UiLanguage::English {
        (date.month, date.day)
    } else {
        (date.day, date.month)
    };
    if full {
        format!(
            "{first:02}/{second:02}/{:04} {:02}:{:02}:{:02}",
            date.year, date.hour, date.minute, date.second
        )
    } else {
        format!(
            "{first:02}/{second:02}/{:02} {:02}:{:02}",
            date.year % 100,
            date.hour,
            date.minute
        )
    }
}

#[cfg(windows)]
fn local_time(timestamp: u64) -> Option<DateTime> {
    use std::ptr;
    use windows_sys::Win32::{
        Foundation::{FILETIME, SYSTEMTIME},
        System::Time::{FileTimeToSystemTime, SystemTimeToTzSpecificLocalTime},
    };
    if timestamp == 0 {
        return None;
    }
    let ticks = timestamp
        .checked_mul(10_000_000)?
        .checked_add(116_444_736_000_000_000)?;
    let file_time = FILETIME {
        dwLowDateTime: ticks as u32,
        dwHighDateTime: (ticks >> 32) as u32,
    };
    let mut utc = SYSTEMTIME::default();
    let mut local = SYSTEMTIME::default();
    // Both calls fill initialized SYSTEMTIME values; a null zone requests Windows' local zone.
    let valid = unsafe {
        FileTimeToSystemTime(&file_time, &mut utc) != 0
            && SystemTimeToTzSpecificLocalTime(ptr::null(), &utc, &mut local) != 0
    };
    valid.then_some(DateTime {
        year: local.wYear,
        month: local.wMonth,
        day: local.wDay,
        hour: local.wHour,
        minute: local.wMinute,
        second: local.wSecond,
    })
}

#[cfg(not(windows))]
fn local_time(timestamp: u64) -> Option<DateTime> {
    // The Windows desktop app uses the local zone. Other platforms display UTC.
    if timestamp == 0 || timestamp > 253_402_300_799 {
        return None;
    }
    let days = (timestamp / 86_400) as i64 + 719_468;
    let era = days / 146_097;
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    let seconds = timestamp % 86_400;
    Some(DateTime {
        year: year as u16,
        month: month as u16,
        day: day as u16,
        hour: (seconds / 3_600) as u16,
        minute: ((seconds / 60) % 60) as u16,
        second: (seconds % 60) as u16,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_follow_the_interface_language_and_include_time() {
        let date = DateTime {
            year: 2026,
            month: 10,
            day: 29,
            hour: 15,
            minute: 4,
            second: 7,
        };
        assert_eq!(
            format_date(date, UiLanguage::English, false),
            "10/29/26 15:04"
        );
        assert_eq!(
            format_date(date, UiLanguage::PortugueseBrazil, true),
            "29/10/2026 15:04:07"
        );
        assert_eq!(
            format_date(date, UiLanguage::Spanish, false),
            "29/10/26 15:04"
        );
        assert!(local_time(0).is_none());
        assert!(local_time(u64::MAX).is_none());
        assert!(local_time(1_700_000_000).is_some());
    }
}
