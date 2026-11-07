//! Privacy stage S1 (docs/specs/privacy.md), as `profiles/privacy/lumen-privacy`
//! did it: an `INPUT` rule rejecting tcp 11111, crashreportd's port, from
//! everything but loopback, and LG's remote-diagnostics jobs stopped
//! (crashreportd first, since it starts whenever rdxd does). Nothing here
//! outlasts the next boot.
//!
//! The jobs start at different points of the boot, some after lumend, and
//! upstart offers lumend no event for them, so their state is read: every
//! 10 s for the first ten minutes, as the hook did, then every five minutes
//! for as long as lumend runs, which the hook couldn't.

use std::time::Duration;

use crate::run::{Program, Runner};

/// LG's jobs S1 holds down, in the order they are stopped.
pub const JOBS: [&str; 4] = ["crashreportd", "rdxd", "uploadd", "remotelogger"];
/// The order the undo starts them in, as their boot does.
pub const START_ORDER: [&str; 4] = ["remotelogger", "rdxd", "crashreportd", "uploadd"];
/// The rule, after `-I`, `-D` or `-C`.
const RULE: [&str; 10] = [
    "INPUT", "-p", "tcp", "--dport", "11111", "!", "-i", "lo", "-j", "REJECT",
];

/// When the next round is due, `since` lumend's start.
pub fn next_round(since: Duration) -> Duration {
    if since < Duration::from_secs(600) {
        Duration::from_secs(10)
    } else {
        Duration::from_secs(300)
    }
}

fn rule_listed(runner: &dyn Runner) -> Result<bool, String> {
    let out = runner.run(Program::Iptables, &["-L", "INPUT", "-n"])?;
    if !out.success {
        return Err(format!("iptables -L failed: {}", out.stderr.trim()));
    }
    Ok(out.stdout.contains("tcp dpt:11111"))
}

/// The rule in place: what was done, in words for the log.
pub fn add_rule(runner: &dyn Runner) -> Result<&'static str, String> {
    if rule_listed(runner)? {
        return Ok("tcp 11111 rule already in place");
    }
    let mut args = vec!["-I"];
    args.extend(RULE);
    let out = runner.run(Program::Iptables, &args)?;
    if out.success {
        Ok("tcp 11111 rejected from the LAN")
    } else {
        Err(format!(
            "FAILED to add the tcp 11111 rule: {}",
            crate::log::clean(out.stderr.trim())
        ))
    }
}

/// Whether `initctl status` says the job runs.
fn running(runner: &dyn Runner, job: &str) -> bool {
    runner
        .run(Program::Initctl, &["status", job])
        .is_ok_and(|out| out.stdout.contains("start/"))
}

/// One round: stops each job that runs. One line a job it acted on.
pub fn stop_jobs(runner: &dyn Runner) -> Vec<String> {
    let mut lines = Vec::new();
    for job in JOBS {
        if !running(runner, job) {
            continue;
        }
        match runner.run(Program::Initctl, &["stop", job]) {
            Ok(out) if out.success => lines.push(format!("stopped {job}")),
            Ok(out) => lines.push(format!(
                "FAILED to stop {job}: {}",
                crate::log::clean(out.stderr.trim())
            )),
            Err(e) => lines.push(format!("FAILED to stop {job}: {e}")),
        }
    }
    lines
}

/// The undo, as `profiles/privacy/s1-undo.sh`: the rule out (every copy),
/// the jobs started in their boot order.
pub fn undo(runner: &dyn Runner) -> Vec<String> {
    let mut lines = Vec::new();
    let mut n = 0;
    while n < 10 && rule_listed(runner).unwrap_or(false) {
        let mut args = vec!["-D"];
        args.extend(RULE);
        match runner.run(Program::Iptables, &args) {
            Ok(out) if out.success => n += 1,
            _ => break,
        }
    }
    lines.push(format!("removed {n} tcp 11111 rule(s)"));
    for job in START_ORDER {
        let started = runner
            .run(Program::Initctl, &["start", job])
            .is_ok_and(|o| o.success);
        let state = if running(runner, job) {
            "runs"
        } else {
            "does not run"
        };
        lines.push(format!(
            "{job}: {}, {state}",
            if started { "started" } else { "not started" }
        ));
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run::fake::FakeRunner;

    #[test]
    fn adds_the_rule_once() {
        let r = FakeRunner::default();
        r.answer(
            "iptables -L INPUT -n",
            true,
            "Chain INPUT (policy ACCEPT)\n",
        );
        r.answer(
            "iptables -I INPUT -p tcp --dport 11111 ! -i lo -j REJECT",
            true,
            "",
        );
        assert_eq!(add_rule(&r).unwrap(), "tcp 11111 rejected from the LAN");

        let r = FakeRunner::default();
        r.answer(
            "iptables -L",
            true,
            "REJECT     tcp  --  0.0.0.0/0  0.0.0.0/0  tcp dpt:11111 reject-with icmp-port-unreachable\n",
        );
        assert_eq!(add_rule(&r).unwrap(), "tcp 11111 rule already in place");
        assert_eq!(r.ran().len(), 1, "nothing added");
    }

    #[test]
    fn stops_only_jobs_that_run() {
        let r = FakeRunner::default();
        r.answer(
            "initctl status rdxd",
            true,
            "rdxd start/running, process 812\n",
        );
        r.answer("initctl status", true, "x stop/waiting\n");
        r.answer("initctl stop rdxd", true, "");
        assert_eq!(stop_jobs(&r), ["stopped rdxd"]);
        assert!(
            !r.ran()
                .iter()
                .any(|l| l.starts_with("initctl stop crashreportd"))
        );
    }

    #[test]
    fn says_when_a_stop_fails() {
        let r = FakeRunner::default();
        r.answer("initctl status uploadd", true, "uploadd start/running\n");
        r.answer("initctl status", true, "stop/waiting\n");
        r.answer("initctl stop uploadd", false, "");
        assert_eq!(stop_jobs(&r), ["FAILED to stop uploadd: "]);
    }

    #[test]
    fn undo_removes_the_rule_and_starts_the_jobs_in_boot_order() {
        let r = FakeRunner::default();
        r.answer("iptables -D", true, "");
        r.answer("iptables -L", true, "");
        r.answer("initctl start", true, "");
        r.answer("initctl status", true, "start/running\n");
        let lines = undo(&r);
        assert_eq!(lines[0], "removed 0 tcp 11111 rule(s)");
        let starts: Vec<String> = r
            .ran()
            .into_iter()
            .filter(|l| l.starts_with("initctl start"))
            .collect();
        assert_eq!(
            starts,
            START_ORDER.map(|j| format!("initctl start {j}")).to_vec()
        );
    }

    #[test]
    fn rounds_slow_down_after_ten_minutes() {
        assert_eq!(next_round(Duration::from_secs(0)), Duration::from_secs(10));
        assert_eq!(
            next_round(Duration::from_secs(599)),
            Duration::from_secs(10)
        );
        assert_eq!(
            next_round(Duration::from_secs(600)),
            Duration::from_secs(300)
        );
    }
}
