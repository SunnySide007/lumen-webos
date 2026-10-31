//! The Top Shelf's art and the look's switches (docs/specs/look-and-feel.md,
//! "Phase 4 features").
//!
//! `hero_for` says which hero the shelf shows for an app, in the owner's
//! order (decision 5, "both"): the owner's own art first, then a hero drawn
//! for that app, then the palette hero nearest its colour, and the ambient
//! hero for everything else. `Look` says which of the Phase 4 features are
//! on, so the TV can measure each one on and off with one build.

use std::collections::HashSet;
use std::fmt;

use crate::apps::{App, AppId};
use crate::style;

/// A Top Shelf hero: full-bleed art for the row-0 tile the shelf shows.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Hero {
    /// The owner's own art for this app, `art-owner/<app id>.png`.
    Owner(AppId),
    /// A hero drawn for the app, by its file name.
    Named(&'static str),
    /// The palette hero `hero-palette-<n>`, one per `style::TILE_PALETTE`
    /// entry.
    Palette(usize),
    /// The aurora, a still of Lumen's ambient loop, in three layers.
    Ambient,
}

/// The ambient hero's layers, back to front: the sky, the curtains' light,
/// the ridge and lake.
pub const AMBIENT_LAYERS: [&str; 3] = ["hero-ambient-bg", "hero-ambient-mid", "hero-ambient-fg"];

impl Hero {
    /// The name the log gives it.
    pub fn name(&self) -> String {
        match self {
            Self::Owner(id) => format!("art-owner/{id}"),
            Self::Named(name) => (*name).to_owned(),
            Self::Palette(n) => format!("hero-palette-{n}"),
            Self::Ambient => "hero-ambient".to_owned(),
        }
    }

    /// Whether it is drawn as layers that shift with the focus tilt: the
    /// ambient hero, when the look has parallax.
    pub fn is_layered(&self, look: &Look) -> bool {
        *self == Self::Ambient && look.parallax
    }

    /// The files it is drawn from, back to front, without the `.png`: in
    /// Lumen's art folder, or the owner's for `Owner`. The ambient hero
    /// comes as its layers with parallax, else as the flat picture they make.
    /// `half` picks the 960 x 320 copies; the owner's art has one size,
    /// whichever it is.
    pub fn files(&self, look: &Look) -> Vec<String> {
        let suffix = if look.half { "-half" } else { "" };
        match self {
            Self::Owner(id) => vec![id.to_string()],
            Self::Ambient if look.parallax => AMBIENT_LAYERS
                .iter()
                .map(|layer| format!("{layer}{suffix}"))
                .collect(),
            _ => vec![format!("{}{suffix}", self.name())],
        }
    }
}

/// The heroes drawn for particular apps, by app ID, as the B9 names them
/// (`listLaunchPoints`, `listApps` and `getAllInputStatus`, 2026-10-21).
pub const NAMED: [(&str, &str); 9] = [
    ("com.webos.app.hdmi1", "hero-hdmi1"),
    ("com.webos.app.hdmi2", "hero-hdmi2"),
    ("com.webos.app.hdmi3", "hero-hdmi3"),
    ("com.webos.app.hdmi4", "hero-hdmi4"),
    ("com.webos.app.livetv", "hero-live-tv"),
    // LG's Plex app, and plx-native.
    ("cdp-30", "hero-plex"),
    ("com.beb.plxnative", "hero-plex"),
    ("org.webosbrew.hbchannel", "hero-homebrew"),
    ("com.palm.app.settings", "hero-settings"),
];

/// The hero the Top Shelf shows for `app`, given which apps the owner made
/// art for: the owner's art, then a hero named for the app, then the
/// palette hero nearest its colour by hue, then the ambient hero. A colour
/// with no hue to speak of (black, white, greys, and colours too near them)
/// takes the ambient hero, as an app with no colour does.
pub fn hero_for(app: &App, owner_art: &HashSet<AppId>) -> Hero {
    if owner_art.contains(&app.id) {
        return Hero::Owner(app.id.clone());
    }
    if let Some(&(_, name)) = NAMED.iter().find(|(id, _)| *id == app.id.as_str()) {
        return Hero::Named(name);
    }
    match app.color.and_then(palette_for) {
        Some(n) => Hero::Palette(n),
        None => Hero::Ambient,
    }
}

/// The `TILE_PALETTE` entry nearest `rgb` by hue, or `None` if its chroma
/// is under `style::HERO_MIN_CHROMA`.
pub fn palette_for(rgb: [u8; 3]) -> Option<usize> {
    let hue = hue(rgb.map(|c| f32::from(c) / 255.0))?;
    let mut best = None;
    for (n, entry) in style::TILE_PALETTE.iter().enumerate() {
        let Some(entry_hue) = hue_of(*entry) else {
            continue;
        };
        let d = (hue - entry_hue).abs();
        let d = d.min(360.0 - d);
        if best.is_none_or(|(_, nearest)| d < nearest) {
            best = Some((n, d));
        }
    }
    best.map(|(n, _)| n)
}

fn hue_of(c: style::Color) -> Option<f32> {
    hue([c.r, c.g, c.b])
}

/// The hue in degrees (0 to 360) of a colour with channels 0 to 1, or `None`
/// if its chroma (the spread of its channels) is under the threshold.
fn hue([r, g, b]: [f32; 3]) -> Option<f32> {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let chroma = max - min;
    if chroma < style::HERO_MIN_CHROMA {
        return None;
    }
    let sector = if max == r {
        (g - b) / chroma
    } else if max == g {
        (b - r) / chroma + 2.0
    } else {
        (r - g) / chroma + 4.0
    };
    Some((sector * 60.0).rem_euclid(360.0))
}

/// Which Phase 4 features are on. Every one is, unless the launch
/// parameters' `"look"` turns it off (`Look::parse`), so the TV can be
/// measured with a feature on and off in one build.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Look {
    /// The Top Shelf hero; without it the shelf keeps its placeholder.
    pub hero: bool,
    /// The ordered dither on gradients and shapes.
    pub dither: bool,
    /// The newly focused tile's lean toward the move.
    pub tilt: bool,
    /// The highlight that sweeps across it once.
    pub light: bool,
    /// The ambient hero's layers shifting with the tilt; without it, the
    /// ambient hero is drawn flat.
    pub parallax: bool,
    /// The heroes from their 960 x 320 files, scaled up: the hero's
    /// fallback. Off unless asked for.
    pub half: bool,
}

impl Look {
    /// Every feature on, at full size: the default.
    pub const FULL: Self = Self {
        hero: true,
        dither: true,
        tilt: true,
        light: true,
        parallax: true,
        half: false,
    };

    /// Every feature off: the home screen as it was before Phase 4.
    pub const PLAIN: Self = Self {
        hero: false,
        dither: false,
        tilt: false,
        light: false,
        parallax: false,
        half: false,
    };

    /// Reads a look such as `plain`, `-dither,-tilt` or `plain,+hero`, from
    /// the full look: words split by commas or spaces, applied in order.
    /// `plain` turns every feature off and `full` every one on (at full
    /// size); a feature's name turns it on, or off after a `-`, and a `+`
    /// before it changes nothing. Also returns the words that name nothing,
    /// which change nothing either: a typo must not keep the TV's Home from
    /// starting.
    pub fn parse(text: &str) -> (Self, Vec<String>) {
        let mut look = Self::FULL;
        let mut unknown = Vec::new();
        for word in text
            .split(|c: char| c == ',' || c.is_whitespace())
            .filter(|w| !w.is_empty())
        {
            let (on, name) = match word.strip_prefix('-') {
                Some(name) => (false, name),
                None => (true, word.strip_prefix('+').unwrap_or(word)),
            };
            match name {
                "plain" if on => look = Self::PLAIN,
                "full" if on => look = Self::FULL,
                _ => match look.switch(name) {
                    Some(feature) => *feature = on,
                    None => unknown.push(word.to_owned()),
                },
            }
        }
        (look, unknown)
    }

    fn switch(&mut self, name: &str) -> Option<&mut bool> {
        Some(match name {
            "hero" => &mut self.hero,
            "dither" => &mut self.dither,
            "tilt" => &mut self.tilt,
            "light" => &mut self.light,
            "parallax" => &mut self.parallax,
            "half" => &mut self.half,
            _ => return None,
        })
    }
}

impl Default for Look {
    fn default() -> Self {
        Self::FULL
    }
}

/// As the start-up log gives it: `hero on, dither off, tilt on, light on,
/// parallax on; art full size`.
impl fmt::Display for Look {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let on = |b: bool| if b { "on" } else { "off" };
        write!(
            f,
            "hero {}, dither {}, tilt {}, light {}, parallax {}; art {} size",
            on(self.hero),
            on(self.dither),
            on(self.tilt),
            on(self.light),
            on(self.parallax),
            if self.half { "half" } else { "full" }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apps::AppKind;

    /// The B9's launch points with their `iconColor` (`listLaunchPoints`,
    /// 2026-10-21; "" is none), its inputs (`getAllInputStatus`) and the
    /// apps LG keeps out of the launch points that the shell shows
    /// (`listApps`): the IDs the table must name, and the colours the
    /// palette and the threshold must sort.
    const B9: [(&str, &str); 32] = [
        ("com.webos.app.photovideo", "#7c20d5"),
        ("netflix", "#ffffff"),
        ("cdp-30", "#000000"),
        ("amazon", "#0578ff"),
        ("youtube.leanback.v4", "#ffffff"),
        ("spotify-beehive", "#191414"),
        ("com.disney.disneyplus-prod", "#054553"),
        ("com.apple.appletv", "#000000"),
        ("com.peacock.tv", "#000000"),
        ("hulu", "#000000"),
        ("lgchannels.us", "#ffffff"),
        ("com.webos.app.igallery", "#139380"),
        ("com.webos.app.browser", "#FFFFFF"),
        ("com.webos.app.music", "#7d53d3"),
        ("com.webos.app.discovery", "#d7156d"),
        ("com.instantbits.cast.webvideo", "#ff9800"),
        ("com.cbs-all-access.webapp.prod", "#0064ff"),
        ("com.webos.app.homeconnect", "#ffffff"),
        ("com.plutotv.app", "#fff200"),
        ("com.beellet.coral.reef.screensaver", "#ebf2fa"),
        ("com.beellet.space.stars.screensaver", "#2b2d42"),
        ("webos.day", "#fd322e"),
        ("org.webosbrew.hbchannel", "#cf0652"),
        ("com.open.hidden.menu", ""),
        ("com.github.gprot42.webosterminal", ""),
        ("com.beb.plxnative", "#060606"),
        ("com.webos.app.hdmi1", ""),
        ("com.webos.app.hdmi2", ""),
        ("com.webos.app.hdmi3", ""),
        ("com.webos.app.hdmi4", ""),
        ("com.webos.app.livetv", ""),
        ("com.palm.app.settings", ""),
    ];

    fn rgb(hex: &str) -> Option<[u8; 3]> {
        let hex = hex.strip_prefix('#')?;
        let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
        Some([byte(0)?, byte(2)?, byte(4)?])
    }

    fn b9_app(id: &str) -> App {
        let (_, color) = B9.iter().find(|(b9, _)| *b9 == id).expect("on the B9");
        let app = App::new(id, id, AppKind::App);
        match rgb(color) {
            Some(c) => app.with_color(c),
            None => app,
        }
    }

    fn hero(id: &str) -> Hero {
        hero_for(&b9_app(id), &HashSet::new())
    }

    #[test]
    fn apps_with_a_hero_of_their_own_get_it() {
        for (id, name) in [
            ("com.webos.app.hdmi1", "hero-hdmi1"),
            ("com.webos.app.hdmi2", "hero-hdmi2"),
            ("com.webos.app.hdmi3", "hero-hdmi3"),
            ("com.webos.app.hdmi4", "hero-hdmi4"),
            ("com.webos.app.livetv", "hero-live-tv"),
            ("cdp-30", "hero-plex"),
            ("com.beb.plxnative", "hero-plex"),
            ("org.webosbrew.hbchannel", "hero-homebrew"),
            ("com.palm.app.settings", "hero-settings"),
        ] {
            assert_eq!(hero(id), Hero::Named(name), "{id}");
        }
        for (id, _) in &NAMED {
            assert!(B9.iter().any(|(b9, _)| b9 == id), "{id} is not on the B9");
        }
    }

    #[test]
    fn a_named_hero_comes_before_the_apps_colour() {
        // Homebrew Channel has a colour of its own (#cf0652).
        assert!(b9_app("org.webosbrew.hbchannel").color.is_some());
        assert_eq!(
            hero("org.webosbrew.hbchannel"),
            Hero::Named("hero-homebrew")
        );
    }

    #[test]
    fn coloured_apps_get_the_palette_hero_nearest_their_hue() {
        for (id, n) in [
            // Hues: 272, 212, 191, 171, 260, 333, 36, 216, 57, 1 degrees.
            ("com.webos.app.photovideo", 6),
            ("amazon", 5),
            ("com.disney.disneyplus-prod", 4),
            ("com.webos.app.igallery", 4),
            ("com.webos.app.music", 6),
            ("com.webos.app.discovery", 7),
            ("com.instantbits.cast.webvideo", 2),
            ("com.cbs-all-access.webapp.prod", 5),
            ("com.plutotv.app", 2),
            ("webos.day", 0),
        ] {
            assert_eq!(hero(id), Hero::Palette(n), "{id}");
        }
    }

    #[test]
    fn black_white_grey_and_nearly_grey_take_the_ambient_hero() {
        for id in [
            "netflix",
            "youtube.leanback.v4",
            "com.apple.appletv",
            "hulu",
            "com.peacock.tv",
            "com.webos.app.browser",
            "lgchannels.us",
            "com.webos.app.homeconnect",
            // #191414, #ebf2fa and #2b2d42: a hue, but too little of it.
            "spotify-beehive",
            "com.beellet.coral.reef.screensaver",
            "com.beellet.space.stars.screensaver",
        ] {
            assert_eq!(hero(id), Hero::Ambient, "{id}");
        }
    }

    #[test]
    fn an_app_with_no_colour_takes_the_ambient_hero() {
        for id in ["com.open.hidden.menu", "com.github.gprot42.webosterminal"] {
            assert_eq!(b9_app(id).color, None);
            assert_eq!(hero(id), Hero::Ambient, "{id}");
        }
    }

    #[test]
    fn the_owners_art_comes_first() {
        let owner: HashSet<AppId> = ["netflix", "com.webos.app.hdmi1", "amazon"]
            .into_iter()
            .map(AppId::new)
            .collect();
        for id in ["netflix", "com.webos.app.hdmi1", "amazon"] {
            assert_eq!(
                hero_for(&b9_app(id), &owner),
                Hero::Owner(AppId::new(id)),
                "{id}"
            );
        }
        assert_eq!(hero_for(&b9_app("hulu"), &owner), Hero::Ambient);
    }

    #[test]
    fn each_palette_colour_takes_its_own_hero() {
        for (n, c) in style::TILE_PALETTE.iter().enumerate() {
            let rgb = [c.r, c.g, c.b].map(|v| (v * 255.0).round() as u8);
            assert_eq!(palette_for(rgb), Some(n), "palette {n}");
        }
    }

    #[test]
    fn the_chroma_threshold_sorts_by_the_spread_of_the_channels() {
        let spread = (style::HERO_MIN_CHROMA * 255.0).ceil() as u8;
        assert!(palette_for([100, 100, 100 + spread]).is_some());
        assert_eq!(palette_for([100, 100, 100 + spread - 1]), None);
        assert_eq!(palette_for([0, 0, 0]), None);
        assert_eq!(palette_for([255, 255, 255]), None);
    }

    #[test]
    fn heroes_name_their_files() {
        let full = Look::FULL;
        let half = Look { half: true, ..full };
        let flat = Look {
            parallax: false,
            ..full
        };
        assert_eq!(Hero::Ambient.files(&full), AMBIENT_LAYERS.map(String::from));
        assert_eq!(
            Hero::Ambient.files(&half),
            AMBIENT_LAYERS.map(|l| format!("{l}-half"))
        );
        assert_eq!(Hero::Ambient.files(&flat), ["hero-ambient"]);
        assert!(Hero::Ambient.is_layered(&full) && !Hero::Ambient.is_layered(&flat));
        assert_eq!(Hero::Palette(3).files(&half), ["hero-palette-3-half"]);
        assert_eq!(Hero::Named("hero-plex").files(&full), ["hero-plex"]);
        let owner = Hero::Owner(AppId::new("netflix"));
        assert_eq!(owner.files(&half), ["netflix"], "one size, whichever");
        assert_eq!(owner.name(), "art-owner/netflix");
        assert!(!owner.is_layered(&full));
    }

    #[test]
    fn a_look_is_full_unless_turned_off() {
        assert_eq!(Look::parse(""), (Look::FULL, vec![]));
        assert_eq!(Look::default(), Look::FULL);
        assert_eq!(Look::parse("plain").0, Look::PLAIN);
        let (look, unknown) = Look::parse("-dither,-tilt");
        assert!(unknown.is_empty());
        assert_eq!(
            look,
            Look {
                dither: false,
                tilt: false,
                ..Look::FULL
            }
        );
        assert_eq!(
            Look::parse("plain, +hero half").0,
            Look {
                hero: true,
                half: true,
                ..Look::PLAIN
            }
        );
        assert_eq!(Look::parse("plain,full").0, Look::FULL);
        assert_eq!(Look::parse("-light,light").0, Look::FULL, "in order");
    }

    #[test]
    fn words_that_name_nothing_change_nothing() {
        let (look, unknown) = Look::parse("-dither,sparkle,-plain,-full,");
        assert_eq!(
            look,
            Look {
                dither: false,
                ..Look::FULL
            }
        );
        assert_eq!(unknown, ["sparkle", "-plain", "-full"]);
    }

    #[test]
    fn a_look_reads_back_as_the_log_gives_it() {
        assert_eq!(
            Look::parse("-dither").0.to_string(),
            "hero on, dither off, tilt on, light on, parallax on; art full size"
        );
        assert_eq!(
            Look::PLAIN.to_string(),
            "hero off, dither off, tilt off, light off, parallax off; art full size"
        );
    }
}
