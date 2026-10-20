//! The apps on the home screen and what launches them. The TV's launcher
//! (luna, milestone B3) and the simulator's `MockLauncher` share one trait.

use std::fmt;

/// A webOS app ID, such as `netflix` or `com.webos.app.hdmi1`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AppId(String);

impl AppId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for AppId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppKind {
    App,
    /// An HDMI or other input, which webOS also opens as an app.
    Input,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct App {
    pub id: AppId,
    pub title: String,
    pub kind: AppKind,
}

impl App {
    pub fn new(id: &str, title: &str, kind: AppKind) -> Self {
        Self {
            id: AppId::new(id),
            title: title.to_owned(),
            kind,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchError(pub String);

impl fmt::Display for LaunchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for LaunchError {}

pub trait Launcher {
    /// The apps and inputs to show, in launch order.
    fn list(&mut self) -> Result<Vec<App>, LaunchError>;
    fn launch(&mut self, id: &AppId) -> Result<(), LaunchError>;
}

/// The simulator's launcher: a fixed list modelled on the B9's apps and HDMI
/// inputs (12 entries, so rows of 5, 5 and 2). The IDs are illustrative;
/// the TV's launcher reports the real ones. Launches are only recorded.
#[derive(Debug, Clone)]
pub struct MockLauncher {
    apps: Vec<App>,
    launched: Vec<AppId>,
    failing: Option<AppId>,
}

impl Default for MockLauncher {
    fn default() -> Self {
        use AppKind::{App as A, Input as I};
        let apps = [
            ("netflix", "Netflix", A),
            ("amazon", "Prime Video", A),
            ("youtube.leanback.v4", "YouTube", A),
            ("plx-native", "Plex", A),
            ("com.disney.disneyplus-prod", "Disney+", A),
            ("com.apple.appletv", "Apple TV", A),
            ("spotify-beehive", "Spotify", A),
            ("com.webos.app.livetv", "Live TV", A),
            ("com.webos.app.hdmi1", "HDMI 1", I),
            ("com.webos.app.hdmi2", "HDMI 2", I),
            ("com.webos.app.browser", "Web Browser", A),
            ("com.palm.app.settings", "Settings", A),
        ];
        Self::with_apps(
            apps.iter()
                .map(|&(id, title, kind)| App::new(id, title, kind))
                .collect(),
        )
    }
}

impl MockLauncher {
    pub fn with_apps(apps: Vec<App>) -> Self {
        Self {
            apps,
            launched: Vec::new(),
            failing: None,
        }
    }

    /// Makes launching `id` fail, for tests of the error path.
    pub fn failing_on(mut self, id: &str) -> Self {
        self.failing = Some(AppId::new(id));
        self
    }

    /// Every successful launch, oldest first.
    pub fn launched(&self) -> &[AppId] {
        &self.launched
    }
}

impl Launcher for MockLauncher {
    fn list(&mut self) -> Result<Vec<App>, LaunchError> {
        Ok(self.apps.clone())
    }

    fn launch(&mut self, id: &AppId) -> Result<(), LaunchError> {
        if self.failing.as_ref() == Some(id) {
            return Err(LaunchError(format!("{id} failed to start (simulated)")));
        }
        if !self.apps.iter().any(|app| &app.id == id) {
            return Err(LaunchError(format!("no app {id}")));
        }
        self.launched.push(id.clone());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_mock_lists_twelve_apps_and_two_inputs() {
        let apps = MockLauncher::default().list().expect("mock never fails");
        assert_eq!(apps.len(), 12);
        assert_eq!(apps[0].title, "Netflix");
        let inputs = apps.iter().filter(|a| a.kind == AppKind::Input).count();
        assert_eq!(inputs, 2);
    }

    #[test]
    fn launches_are_recorded() {
        let mut mock = MockLauncher::default();
        mock.launch(&AppId::new("netflix")).expect("known app");
        mock.launch(&AppId::new("com.webos.app.hdmi1"))
            .expect("known input");
        assert_eq!(
            mock.launched(),
            &[AppId::new("netflix"), AppId::new("com.webos.app.hdmi1")]
        );
    }

    #[test]
    fn unknown_and_failing_apps_report_errors() {
        let mut mock = MockLauncher::default().failing_on("amazon");
        let err = mock.launch(&AppId::new("amazon")).unwrap_err();
        assert!(err.to_string().contains("amazon"));
        assert!(mock.launch(&AppId::new("nope")).is_err());
        assert!(mock.launched().is_empty());
    }
}
