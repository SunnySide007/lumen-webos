//! Every name and path `lumend` uses on the TV, fixed here. Nothing a
//! request carries becomes a path.

use std::path::{Path, PathBuf};

/// The shell's app id.
pub const APP_ID: &str = "io.github.sunnyside007.lumen.shell";
/// LG's Home.
pub const LG_HOME: &str = "com.webos.app.home";

/// The daemon's bus name, and the one `lumend try` and `undo` use, so they
/// never clash with a daemon that runs. Its role (profiles/lumend) allows
/// these two for `BIN` only.
pub const BUS_NAME: &str = "io.github.sunnyside007.lumend";
pub const BUS_NAME_TRY: &str = "io.github.sunnyside007.lumend.try";

/// Root's own folder for lumend, 0700: the binaries, the settings, the
/// lock and the status. Nothing in it is reachable from an app's jail.
pub const DIR: &str = "/var/lib/lumen/lumend";
/// The daemon. Its role names this exact path.
pub const BIN: &str = "/var/lib/lumen/lumend/lumend";
pub const STANDIN: &str = "/var/lib/lumen/lumend/lumen-standin";
pub const CONF_NAME: &str = "lumend.conf";
pub const STATUS_NAME: &str = "status";
pub const LOCK_NAME: &str = "lumend.pid";
/// A tool's word that it closed the shell on purpose: the unix time until
/// which the shell is not started again (`tools/tv.sh close` and `install`
/// write it; keeper.rs).
pub const HOLD_NAME: &str = "shell-hold";
/// Boot hooks' and lumend's logs (CONTRIBUTING.md), never `/tmp`.
pub const VAR_LUMEN: &str = "/var/lib/lumen";
pub const LOG_DIR_NAME: &str = "log";
pub const LOG_NAME: &str = "lumend.log";

/// The kill switch (docs/safety.md, rule 5).
pub const KILL_DIR: &str = "/var/luna/preferences";
pub const KILL_NAME: &str = "lumen_disable";

/// The app jails: Lumen's root is `JAILS/APP_ID`.
pub const JAILS: &str = "/var/palm/jail";
/// Developer-mode apps; LG makes the first two levels 0777.
pub const DEV_APPS: &str = "/media/developer/apps";
/// The three folders below `DEV_APPS` the IPK sets, then Lumen's own.
pub const SHARED: [&str; 3] = ["usr", "palm", "applications"];

/// Where the files `lumend` reads and writes are, under a root that is `/`
/// on the TV and a scratch folder in the tests.
#[derive(Debug, Clone)]
pub struct Root(PathBuf);

impl Root {
    pub fn tv() -> Self {
        Self(PathBuf::from("/"))
    }

    #[cfg(test)]
    pub fn at(path: &Path) -> Self {
        Self(path.to_path_buf())
    }

    /// `abs`, an absolute path on the TV, under this root.
    pub fn path(&self, abs: &str) -> PathBuf {
        self.0.join(abs.trim_start_matches('/'))
    }

    pub fn base(&self) -> &Path {
        &self.0
    }

    /// Whether this is the TV's own root.
    pub fn is_tv(&self) -> bool {
        self.0 == Path::new("/")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_join_under_the_root() {
        let r = Root::at(Path::new("/x/y"));
        assert_eq!(
            r.path("/var/lib/lumen"),
            PathBuf::from("/x/y/var/lib/lumen")
        );
        assert_eq!(Root::tv().path(DIR), PathBuf::from(DIR));
        assert!(BIN.starts_with(DIR) && STANDIN.starts_with(DIR));
        assert!(BUS_NAME_TRY.starts_with(BUS_NAME));
    }
}
