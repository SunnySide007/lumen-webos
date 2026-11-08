//! The ssh command line, and what ssh's complaints mean in a few words.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// Windows' own OpenSSH client, as remote.md asks; elsewhere (and when it is
/// missing) the `ssh` on PATH.
pub fn default_ssh(get: impl Fn(&str) -> Option<String>) -> PathBuf {
    if cfg!(windows) {
        let root = get("SystemRoot").unwrap_or_else(|| r"C:\Windows".into());
        let own = Path::new(&root)
            .join("System32")
            .join("OpenSSH")
            .join("ssh.exe");
        if own.is_file() {
            return own;
        }
    }
    PathBuf::from("ssh")
}

/// ssh to lumen-ctl with only the app's key, never a password, no terminal,
/// no forwarding, and no configuration file that could add either: the
/// options `tools/remote/ctl.py` runs on the B9. The forced command on the TV
/// runs `lumen-ctl serve` whatever is asked, so no command is sent.
pub fn argv(ssh: &Path, key: &Path, host: &str, port: u16) -> Vec<OsString> {
    let mut v: Vec<OsString> = vec![ssh.into(), "-F".into(), "none".into(), "-i".into()];
    v.push(key.into());
    for arg in [
        "-p",
        &port.to_string(),
        "-T",
        "-o",
        "IdentitiesOnly=yes",
        "-o",
        "BatchMode=yes",
        "-o",
        "PasswordAuthentication=no",
        "-o",
        "KbdInteractiveAuthentication=no",
        "-o",
        "ControlPath=none",
        "-o",
        "ConnectTimeout=8",
        "-o",
        "StrictHostKeyChecking=accept-new",
        "-o",
        "ServerAliveInterval=15",
        "-o",
        "ClearAllForwardings=yes",
        &format!("root@{host}"),
    ] {
        v.push(arg.into());
    }
    v
}

/// What a closed session says to the person: ssh's last words, in plain
/// terms where they are ones we know.
pub fn explain(stderr: &str, code: Option<i32>, key: &Path, host: &str) -> String {
    let has = |s: &str| stderr.contains(s);
    if has("UNPROTECTED PRIVATE KEY FILE") || has("ad permissions") {
        // The account's own name, so the line pastes into any shell.
        let user = std::env::var("USERNAME").unwrap_or_else(|_| "%USERNAME%".into());
        return format!(
            "Windows' ssh refuses the key: other accounts can read {key}. Make it yours \
             alone: icacls \"{key}\" /inheritance:r /grant:r \"{user}:F\"",
            key = key.display(),
        );
    }
    if has("REMOTE HOST IDENTIFICATION HAS CHANGED") || has("Host key verification failed") {
        return format!(
            "The TV's host key isn't the one ssh knows for {host}, so it stopped. If the TV \
             was reset, remove {host}'s line from ~/.ssh/known_hosts"
        );
    }
    if has("Permission denied") {
        return "The TV refused the key: is it in authorized_keys? (tools/remote.sh status)".into();
    }
    if has("timed out") {
        return format!("No answer from {host}: the TV may be off or asleep");
    }
    if has("Connection refused") {
        return format!("{host} refused the connection: is SSH on?");
    }
    if has("Could not resolve hostname") {
        return format!("No address for {host}");
    }
    if has("No route to host") || has("Network is unreachable") || has("unreachable") {
        return format!("No way to {host} from this PC");
    }
    let last = stderr
        .lines()
        .map(str::trim)
        .rfind(|l| !l.is_empty() && !l.starts_with("Warning: Permanently added"));
    match (last, code) {
        (Some(line), _) => line.chars().take(200).collect(),
        (None, Some(0) | None) => "The TV closed the link".into(),
        (None, Some(c)) => format!("ssh ended (exit {c})"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_key_no_password_no_terminal_no_forwarding() {
        let v = argv(Path::new("ssh"), Path::new("/k/id"), "tv.lan", 2222);
        let v: Vec<String> = v.iter().map(|a| a.to_string_lossy().into_owned()).collect();
        let joined = v.join(" ");
        assert!(
            joined.starts_with("ssh -F none -i /k/id -p 2222 -T "),
            "{joined}"
        );
        for opt in [
            "IdentitiesOnly=yes",
            "BatchMode=yes",
            "PasswordAuthentication=no",
            "KbdInteractiveAuthentication=no",
            "ControlPath=none",
            "ClearAllForwardings=yes",
        ] {
            assert!(v.iter().any(|a| a == opt), "{opt}");
        }
        assert_eq!(v.last().unwrap(), "root@tv.lan");
    }

    #[test]
    fn says_what_ssh_meant() {
        let key = Path::new("/k/id");
        for (stderr, want) in [
            ("WARNING: UNPROTECTED PRIVATE KEY FILE!\n", "icacls"),
            (
                "root@tv: Permission denied (publickey).\n",
                "refused the key",
            ),
            (
                "ssh: connect to host tv port 22: Connection timed out\n",
                "off or asleep",
            ),
            (
                "ssh: connect to host tv port 22: Connection refused\n",
                "is SSH on",
            ),
            ("Host key verification failed.\n", "known_hosts"),
            (
                "ssh: Could not resolve hostname tv: No such host is known.\n",
                "No address",
            ),
        ] {
            let got = explain(stderr, Some(255), key, "tv");
            assert!(got.contains(want), "{stderr:?}: {got}");
        }
        assert_eq!(
            explain(
                "Warning: Permanently added 'tv' (ED25519)\n",
                Some(0),
                key,
                "tv"
            ),
            "The TV closed the link"
        );
        assert_eq!(explain("", Some(3), key, "tv"), "ssh ended (exit 3)");
        assert_eq!(
            explain("lumen-ctl: something\n\n", Some(2), key, "tv"),
            "lumen-ctl: something"
        );
    }
}
