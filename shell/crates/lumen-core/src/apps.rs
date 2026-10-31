//! The apps on the home screen and what launches them. The TV's launcher
//! (luna, milestone B3) and the simulator's `MockLauncher` share one trait.

use std::fmt;
use std::path::{Path, PathBuf};

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
    /// A PNG to draw on the tile. webOS gives each app one (its appinfo's
    /// `icon` or `largeIcon`); without one, or if it will not load, the tile
    /// shows the title's first letter.
    pub icon: Option<PathBuf>,
    /// The tile's colour as RGB (webOS's `iconColor`); without one, a colour
    /// from the shell's palette.
    pub color: Option<[u8; 3]>,
}

impl App {
    pub fn new(id: &str, title: &str, kind: AppKind) -> Self {
        Self {
            id: AppId::new(id),
            title: title.to_owned(),
            kind,
            icon: None,
            color: None,
        }
    }

    pub fn with_icon(mut self, path: impl Into<PathBuf>) -> Self {
        self.icon = Some(path.into());
        self
    }

    pub fn with_color(mut self, rgb: [u8; 3]) -> Self {
        self.color = Some(rgb);
        self
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

/// So the shell can pick its launcher at run time: luna on the TV, the mock
/// in the simulator.
impl<L: Launcher + ?Sized> Launcher for Box<L> {
    fn list(&mut self) -> Result<Vec<App>, LaunchError> {
        (**self).list()
    }

    fn launch(&mut self, id: &AppId) -> Result<(), LaunchError> {
        (**self).launch(id)
    }
}

/// LG's own Home. Lumen is the TV's Home in its place (takeover/README.md),
/// and its tile is the way back: LG's launch points leave it out.
pub const LG_HOME: &str = "com.webos.app.home";
/// That tile's title. LG's is "Home", which is Lumen now.
pub const LG_HOME_TITLE: &str = "LG Home";

/// The longest title kept, in characters. On the TV titles come from other
/// apps' appinfo, and fitting one to a tile takes time in its length; LG's
/// own run to about 30.
pub const MAX_TITLE_CHARS: usize = 64;

/// A title as the shell shows it: trimmed of spaces and control characters,
/// those inside as spaces, and cut to `MAX_TITLE_CHARS`.
pub fn clean_title(title: &str) -> String {
    title
        .trim_matches(|c: char| c.is_whitespace() || c.is_control())
        .chars()
        .take(MAX_TITLE_CHARS)
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
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
            ("com.beb.plxnative", "Plex", A),
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

    /// Gives the apps the simulator's placeholder pictograms from `dir`
    /// (shell/assets/mock-icons): a play button for video, a note for music,
    /// and so on. Plex and Disney+ keep none, to show the letter tiles.
    pub fn with_icons_in(mut self, dir: &Path) -> Self {
        for app in &mut self.apps {
            let icon = match app.id.as_str() {
                "netflix" | "amazon" | "youtube.leanback.v4" | "com.apple.appletv" => "play",
                "spotify-beehive" => "note",
                "com.webos.app.livetv" => "tv",
                "com.webos.app.browser" => "globe",
                "com.palm.app.settings" => "gear",
                _ if app.kind == AppKind::Input => "input",
                _ => continue,
            };
            app.icon = Some(dir.join(format!("{icon}.png")));
        }
        self
    }

    /// Adds LG's Home last among the apps, as the TV's launcher does, unless
    /// the list has it. The simulator shows it; tests keep the twelve.
    pub fn with_lg_home(mut self) -> Self {
        if !self.apps.iter().any(|app| app.id.as_str() == LG_HOME) {
            let at = self
                .apps
                .iter()
                .rposition(|app| app.kind == AppKind::App)
                .map_or(0, |i| i + 1);
            let lg_home = App::new(LG_HOME, LG_HOME_TITLE, AppKind::App);
            self.apps.insert(at, lg_home);
        }
        self
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
    fn mock_apps_have_no_icons_until_given_a_folder() {
        let plain = MockLauncher::default().list().unwrap();
        assert!(plain.iter().all(|a| a.icon.is_none() && a.color.is_none()));

        let dir = Path::new("assets/mock-icons");
        let apps = MockLauncher::default().with_icons_in(dir).list().unwrap();
        let icon = |id: &str| {
            apps.iter()
                .find(|a| a.id.as_str() == id)
                .and_then(|a| a.icon.clone())
        };
        assert_eq!(icon("netflix"), Some(dir.join("play.png")));
        assert_eq!(icon("com.webos.app.hdmi2"), Some(dir.join("input.png")));
        assert_eq!(icon("com.palm.app.settings"), Some(dir.join("gear.png")));
        assert_eq!(icon("com.beb.plxnative"), None, "a letter tile");
        assert_eq!(icon("com.disney.disneyplus-prod"), None, "a letter tile");
        assert_eq!(apps.iter().filter(|a| a.icon.is_some()).count(), 10);
    }

    #[test]
    fn apps_take_an_icon_and_a_colour() {
        let app = App::new("x", "X", AppKind::App)
            .with_icon("/usr/palm/applications/x/icon.png")
            .with_color([1, 2, 3]);
        assert_eq!(
            app.icon.as_deref(),
            Some(Path::new("/usr/palm/applications/x/icon.png"))
        );
        assert_eq!(app.color, Some([1, 2, 3]));
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

    #[test]
    fn lg_home_comes_last_among_the_apps_once() {
        let mut mock = MockLauncher::default().with_lg_home().with_lg_home();
        let apps = mock.list().unwrap();
        assert_eq!(apps.len(), 13);
        // The mock's last entry is an app, Settings, so LG Home follows it.
        let last = apps.last().unwrap();
        assert_eq!(
            (last.id.as_str(), last.title.as_str(), last.kind),
            (LG_HOME, LG_HOME_TITLE, AppKind::App)
        );
        assert!(mock.launch(&AppId::new(LG_HOME)).is_ok());

        // Ahead of the inputs that follow the last app, and not added twice.
        let inputs_last = MockLauncher::with_apps(vec![
            App::new("netflix", "Netflix", AppKind::App),
            App::new("com.webos.app.hdmi1", "HDMI 1", AppKind::Input),
        ]);
        let ids: Vec<String> = inputs_last
            .with_lg_home()
            .list()
            .unwrap()
            .iter()
            .map(|a| a.id.to_string())
            .collect();
        assert_eq!(ids, ["netflix", LG_HOME, "com.webos.app.hdmi1"]);
        let listed = MockLauncher::with_apps(vec![App::new(LG_HOME, "Home", AppKind::App)]);
        let apps = listed.with_lg_home().list().unwrap();
        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0].title, "Home", "LG's own entry is kept");
    }

    #[test]
    fn a_boxed_launcher_is_a_launcher() {
        let mut boxed: Box<dyn Launcher> = Box::new(MockLauncher::default());
        assert_eq!(boxed.list().unwrap().len(), 12);
        assert!(boxed.launch(&AppId::new("netflix")).is_ok());
    }

    #[test]
    fn titles_are_trimmed_cleaned_and_capped() {
        assert_eq!(clean_title("  Streamer "), "Streamer");
        assert_eq!(clean_title("Two\nlines\t"), "Two lines");
        // Nothing but control characters is no title, so the ID stands in.
        assert_eq!(clean_title("\u{0}\u{1b}\u{7f}"), "");
        assert_eq!(clean_title("\u{0}Netflix\u{7}"), "Netflix");
        let long = "é".repeat(200);
        assert_eq!(clean_title(&long).chars().count(), MAX_TITLE_CHARS);
        assert_eq!(clean_title(""), "");
    }
}
