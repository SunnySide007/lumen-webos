//! The time of day for the home's clock: the system's local time, read once
//! a minute by the home loop (`TimeOfDay`). On the TV, glibc's
//! `localtime_r` after `tzset`, so a time zone or summer time the owner sets
//! in LG's settings shows within the minute; on Windows, for the simulator,
//! `GetLocalTime`.

/// An hour (0 to 23), minute and second of local time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimeOfDay {
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
}

impl TimeOfDay {
    /// `HH:MM`, as `--clock` takes it, at second 0.
    pub fn parse(text: &str) -> Option<Self> {
        let (h, m) = text.split_once(':')?;
        let hour: u8 = h.parse().ok().filter(|h| *h < 24)?;
        let minute: u8 = m.parse().ok().filter(|m| *m < 60)?;
        (m.len() == 2).then_some(Self {
            hour,
            minute,
            second: 0,
        })
    }

    /// Seconds until the minute shown changes, at least one.
    pub fn seconds_to_next_minute(self) -> u64 {
        60 - u64::from(self.second.min(59))
    }
}

/// Before this year the TV has not set its clock yet (it boots in 1970 or at
/// its build date until the network or a broadcast tells it the time), and
/// no clock is better than a wrong one.
#[cfg(any(target_os = "linux", windows))]
const FIRST_YEAR: i32 = 2026;

/// The local time now, or `None` where it cannot be read or is not set yet.
#[cfg(target_os = "linux")]
pub fn now() -> Option<TimeOfDay> {
    // POSIX's, in glibc; the libc crate does not declare it.
    unsafe extern "C" {
        fn tzset();
    }

    // SAFETY: `tzset` takes no arguments and only reloads the C library's
    // time zone from TZ or /etc/localtime; the home loop is the only caller
    // of the local time functions in this process.
    unsafe { tzset() };
    // SAFETY: `time` with a null pointer only returns the clock.
    let t = unsafe { libc::time(std::ptr::null_mut()) };
    let mut tm = std::mem::MaybeUninit::<libc::tm>::uninit();
    // SAFETY: `t` is a valid time_t to read, and `tm` is valid for writes of
    // one `struct tm`, the size and alignment libc's declaration has;
    // `localtime_r` is the thread-safe form and writes only there.
    let filled = unsafe { libc::localtime_r(&raw const t, tm.as_mut_ptr()) };
    if filled.is_null() {
        return None;
    }
    // SAFETY: a call that returned non-null filled the whole struct, whose
    // fields are integers and a pointer, valid for any value it wrote.
    let tm = unsafe { tm.assume_init() };
    if tm.tm_year + 1900 < FIRST_YEAR {
        return None;
    }
    Some(TimeOfDay {
        hour: u8::try_from(tm.tm_hour).ok()?,
        minute: u8::try_from(tm.tm_min).ok()?,
        // 60 in a leap second.
        second: u8::try_from(tm.tm_sec).ok()?.min(59),
    })
}

#[cfg(windows)]
pub fn now() -> Option<TimeOfDay> {
    /// Win32's `SYSTEMTIME`.
    #[repr(C)]
    struct SystemTime {
        year: u16,
        month: u16,
        day_of_week: u16,
        day: u16,
        hour: u16,
        minute: u16,
        second: u16,
        milliseconds: u16,
    }

    unsafe extern "system" {
        fn GetLocalTime(time: *mut SystemTime);
    }

    let mut st = std::mem::MaybeUninit::<SystemTime>::uninit();
    // SAFETY: `SystemTime` matches Win32's `SYSTEMTIME` (eight WORDs), and
    // `st` is valid for writes of one; `GetLocalTime` always fills it.
    unsafe { GetLocalTime(st.as_mut_ptr()) };
    // SAFETY: the call filled every field, and any bits are a valid u16.
    let st = unsafe { st.assume_init() };
    if i32::from(st.year) < FIRST_YEAR {
        return None;
    }
    Some(TimeOfDay {
        hour: u8::try_from(st.hour).ok()?,
        minute: u8::try_from(st.minute).ok()?,
        second: u8::try_from(st.second).ok()?.min(59),
    })
}

#[cfg(not(any(target_os = "linux", windows)))]
pub fn now() -> Option<TimeOfDay> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_clock_flag_reads_hours_and_minutes() {
        let t = TimeOfDay::parse("21:41").unwrap();
        assert_eq!((t.hour, t.minute, t.second), (21, 41, 0));
        assert_eq!(TimeOfDay::parse("9:05").map(|t| t.hour), Some(9));
        for bad in ["24:00", "12:60", "12:5", "1241", "", "ab:cd", "12:"] {
            assert_eq!(TimeOfDay::parse(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn the_next_minute_is_never_now() {
        let at = |second| TimeOfDay {
            hour: 0,
            minute: 0,
            second,
        };
        assert_eq!(at(0).seconds_to_next_minute(), 60);
        assert_eq!(at(59).seconds_to_next_minute(), 1);
        assert_eq!(at(60).seconds_to_next_minute(), 1);
    }

    #[test]
    fn the_system_clock_reads_or_says_it_cannot() {
        if let Some(t) = now() {
            assert!(t.hour < 24 && t.minute < 60 && t.second < 60, "{t:?}");
        }
    }
}
