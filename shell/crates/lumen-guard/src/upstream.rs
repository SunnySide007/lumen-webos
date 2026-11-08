//! Where a forwarded lookup goes (stage G3; decision 11's one resolver).
//! With the owner's TLS switch off, to the router in clear, as G1 and G2 did.
//! With it on, to the TLS server (Quad9 over DNS over TLS), except in clear
//! through the router, as a fallback, in two cases only:
//!
//! * the TV's clock reads before the floor (`--clock-floor`), or a
//!   certificate was refused by it: the B9 boots at 2019-01-01 until LG's time
//!   server answers, and its first lookups must not wait on TLS;
//! * the TLS server could not be reached, for a backoff of 10 s, doubling
//!   after each failure up to 5 min, after each address was tried once.
//!
//! Every change of route is said on stderr (the hook's log), with how long a
//! fallback lasted and how many lookups went in clear during it, and each
//! failure is said too. The counts go in the status file.

use std::time::{Duration, Instant};

const FIRST_BACKOFF: Duration = Duration::from_secs(10);
const MAX_BACKOFF: Duration = Duration::from_secs(5 * 60);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Why {
    /// The clock can't be trusted to check a certificate.
    Clock,
    /// The TLS server could not be reached or would not do TLS.
    Down,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    /// The switch is off: the router, as before G3. Not a fallback.
    Clear,
    Tls,
    /// The switch is on, but this lookup goes to the router in clear.
    Fallback(Why),
}

impl Route {
    pub fn word(self) -> &'static str {
        match self {
            Route::Clear => "clear",
            Route::Tls => "tls",
            Route::Fallback(Why::Clock) => "fallback-clock",
            Route::Fallback(Why::Down) => "fallback-down",
        }
    }
}

/// Why a TLS exchange failed before an answer came.
#[derive(Debug)]
pub struct Failure {
    pub text: String,
    /// A certificate was refused as not yet valid or expired: the clock's
    /// fault, most likely, rather than the server's.
    pub clock: bool,
}

#[derive(Debug)]
pub struct Upstream {
    servers: usize,
    clock_floor: u64,
    /// The TLS server to try next, an index into `--tls-server`.
    next_server: usize,
    /// Failures in a row since the last success or backoff.
    failures: usize,
    backoff: Duration,
    /// The fallback after failures: until when, why, and for how long.
    down: Option<(Instant, Why, Duration)>,
    last_error: String,
    /// The route last said on stderr.
    shown: Option<Route>,
    /// When the current fallback began (unix seconds), or 0.
    fallback_since: u64,
    /// Lookups that went in clear during the current fallback.
    in_fallback: u64,
}

impl Upstream {
    pub fn new(servers: usize, clock_floor: u64) -> Self {
        Self {
            servers: servers.max(1),
            clock_floor,
            next_server: 0,
            failures: 0,
            backoff: FIRST_BACKOFF,
            down: None,
            last_error: String::new(),
            shown: None,
            fallback_since: 0,
            in_fallback: 0,
        }
    }

    /// The route for a lookup at `unix` (the TV's clock) and `now`, and a
    /// line to say when it differs from the last one said.
    pub fn route(&mut self, tls: bool, unix: u64, now: Instant) -> (Route, Option<String>) {
        let route = self.peek(tls, unix, now);
        if self.shown == Some(route) {
            return (route, None);
        }
        let said = self.say_change(route, unix);
        self.shown = Some(route);
        (route, Some(said))
    }

    /// The route, without noting it.
    pub fn peek(&self, tls: bool, unix: u64, now: Instant) -> Route {
        if !tls {
            return Route::Clear;
        }
        if unix < self.clock_floor {
            return Route::Fallback(Why::Clock);
        }
        match self.down {
            Some((until, why, _)) if now < until => Route::Fallback(why),
            _ => Route::Tls,
        }
    }

    fn say_change(&mut self, to: Route, unix: u64) -> String {
        let text = match to {
            Route::Tls if self.fallback_since > 0 => format!(
                "upstream tls again after {} s of fallback, in which {} lookups went in clear",
                unix.saturating_sub(self.fallback_since),
                self.in_fallback
            ),
            Route::Tls => "upstream tls".to_owned(),
            Route::Clear => "upstream clear: the TLS switch is off".to_owned(),
            Route::Fallback(Why::Clock) if unix < self.clock_floor => format!(
                "FALLBACK: lookups go in clear through the router while the clock ({unix}) \
                 reads before {}, so no certificate can be checked",
                self.clock_floor
            ),
            Route::Fallback(why) => format!(
                "FALLBACK: lookups go in clear through the router for {} s{}: {}",
                self.down.map_or(0, |(_, _, d)| d.as_secs()),
                if why == Why::Clock {
                    " (the clock may be wrong)"
                } else {
                    ""
                },
                self.last_error
            ),
        };
        match to {
            Route::Fallback(_) if self.fallback_since == 0 => {
                // 1, not 0, if the clock reads 0: 0 means no fallback.
                self.fallback_since = unix.max(1);
                self.in_fallback = 0;
            }
            Route::Fallback(_) => {}
            Route::Tls | Route::Clear => {
                self.fallback_since = 0;
                self.in_fallback = 0;
            }
        }
        text
    }

    /// Counts a lookup that went in clear while the switch was on.
    pub fn went_clear(&mut self) {
        self.in_fallback += 1;
    }

    /// When the current fallback began, or 0.
    pub fn fallback_since(&self) -> u64 {
        self.fallback_since
    }

    /// The TLS server to try next.
    pub fn server(&self) -> usize {
        self.next_server
    }

    /// Notes a failed exchange with server `server`: the next lookup tries
    /// the next address, and once every address has failed in a row (or a
    /// certificate was refused by the clock), lookups fall back for a backoff.
    /// Returns the line to say.
    pub fn failed(&mut self, server: usize, failure: Failure, now: Instant) -> String {
        self.last_error = failure.text;
        if server == self.next_server {
            self.next_server = (self.next_server + 1) % self.servers;
        }
        self.failures += 1;
        if failure.clock || self.failures >= self.servers {
            let why = if failure.clock { Why::Clock } else { Why::Down };
            self.down = Some((now + self.backoff, why, self.backoff));
            self.backoff = (self.backoff * 2).min(MAX_BACKOFF);
            self.failures = 0;
            // Said again even if a fallback was already shown: a new backoff.
            if self.shown == Some(Route::Fallback(why)) {
                self.shown = None;
            }
        }
        format!(
            "tls: {}; this lookup goes in clear through the router",
            self.last_error
        )
    }

    /// Notes an answer over TLS: failures and backoff start again.
    pub fn succeeded(&mut self) {
        self.failures = 0;
        self.backoff = FIRST_BACKOFF;
        self.down = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FLOOR: u64 = 1_793_491_200;
    const NOW: u64 = FLOOR + 100;

    fn down(text: &str) -> Failure {
        Failure {
            text: text.to_owned(),
            clock: false,
        }
    }

    #[test]
    fn off_means_clear_and_is_no_fallback() {
        let mut u = Upstream::new(2, FLOOR);
        let t = Instant::now();
        let (route, said) = u.route(false, 0, t);
        assert_eq!(route, Route::Clear);
        assert!(said.unwrap().contains("switch is off"));
        assert_eq!(u.fallback_since(), 0);
        assert_eq!(u.route(false, 0, t), (Route::Clear, None));
    }

    #[test]
    fn the_boot_clock_falls_back_at_once_and_comes_back() {
        let mut u = Upstream::new(2, FLOOR);
        let t = Instant::now();
        // 2019-01-01, as the B9 boots.
        let (route, said) = u.route(true, 1_546_300_800, t);
        assert_eq!(route, Route::Fallback(Why::Clock));
        assert!(said.unwrap().contains("FALLBACK"));
        assert_eq!(u.fallback_since(), 1_546_300_800);
        u.went_clear();
        u.went_clear();
        assert_eq!(u.route(true, 1_546_300_810, t), (route, None));
        // LG's time server answered.
        let (route, said) = u.route(true, NOW, t);
        assert_eq!(route, Route::Tls);
        let said = said.unwrap();
        assert!(said.contains("2 lookups went in clear"), "{said}");
        assert_eq!(u.fallback_since(), 0);
    }

    #[test]
    fn tries_each_server_then_backs_off_doubling() {
        let mut u = Upstream::new(2, FLOOR);
        let t = Instant::now();
        assert_eq!(u.route(true, NOW, t).0, Route::Tls);
        assert_eq!(u.server(), 0);
        let said = u.failed(0, down("connect 9.9.9.9:853: timed out"), t);
        assert!(said.contains("goes in clear"), "{said}");
        // One address failed: the next lookup tries the other.
        assert_eq!(u.peek(true, NOW, t), Route::Tls);
        assert_eq!(u.server(), 1);
        u.failed(1, down("connect 149.112.112.112:853: refused"), t);
        assert_eq!(u.server(), 0);
        let (route, said) = u.route(true, NOW, t);
        assert_eq!(route, Route::Fallback(Why::Down));
        let said = said.unwrap();
        assert!(
            said.contains("for 10 s") && said.contains("refused"),
            "{said}"
        );
        // After the backoff, TLS is tried again; two more failures back off 20 s.
        let later = t + Duration::from_secs(10);
        assert_eq!(u.route(true, NOW + 10, later).0, Route::Tls);
        u.failed(0, down("a"), later);
        u.failed(1, down("b"), later);
        let (route, said) = u.route(true, NOW + 10, later);
        assert_eq!(route, Route::Fallback(Why::Down));
        assert!(said.unwrap().contains("for 20 s"));
        assert_eq!(
            u.peek(true, NOW, later + Duration::from_secs(19)),
            Route::Fallback(Why::Down)
        );
        // A success resets the backoff.
        let after = later + Duration::from_secs(20);
        assert_eq!(u.peek(true, NOW, after), Route::Tls);
        u.succeeded();
        u.failed(0, down("a"), after);
        u.failed(1, down("b"), after);
        assert_eq!(
            u.peek(true, NOW, after + Duration::from_secs(10)),
            Route::Tls
        );
    }

    #[test]
    fn the_backoff_stops_at_five_minutes() {
        let mut u = Upstream::new(1, FLOOR);
        let mut t = Instant::now();
        for _ in 0..12 {
            u.failed(0, down("x"), t);
            t += MAX_BACKOFF;
        }
        u.failed(0, down("x"), t);
        assert_eq!(
            u.peek(true, NOW, t + MAX_BACKOFF - Duration::from_secs(1)),
            Route::Fallback(Why::Down)
        );
        assert_eq!(u.peek(true, NOW, t + MAX_BACKOFF), Route::Tls);
    }

    #[test]
    fn a_certificate_refused_by_the_clock_backs_off_at_once() {
        let mut u = Upstream::new(2, FLOOR);
        let t = Instant::now();
        u.route(true, NOW, t);
        u.failed(
            0,
            Failure {
                text: "certificate expired".to_owned(),
                clock: true,
            },
            t,
        );
        let (route, said) = u.route(true, NOW, t);
        assert_eq!(route, Route::Fallback(Why::Clock));
        assert!(said.unwrap().contains("clock may be wrong"));
    }
}
