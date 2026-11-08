//! Where each app goes on the home (decision 10, "the grid shows the
//! contract"): the contract's apps in the row, then the "Other apps" tile;
//! the inputs, then Settings, as chips under it; everything else, LG's Home
//! and Live TV among them, on the "Other apps" page. The order and contents
//! will be edited from Lumen Remote; until then this table decides.

use crate::apps::{App, AppKind};
use crate::layout::{CHIPS_MAX, ROW_MAX};

/// The "Other apps" tile's ID. It names no webOS app: Select on it turns to
/// the "Other apps" page.
pub const OTHERS_ID: &str = "lumen.others";
pub const OTHERS_TITLE: &str = "Other apps";
/// LG's Settings: the last chip.
pub const SETTINGS: &str = "com.palm.app.settings";

/// The contract's apps (decision 10), in the row's order: Netflix, Prime
/// Video, YouTube (the ad-free build keeps LG's ID) and Plex. Each entry is
/// one tile, taken by the first of its IDs the TV has: plx-native before
/// LG's Plex.
pub const CONTRACT: [&[&str]; 4] = [
    &["netflix"],
    &["amazon"],
    &["youtube.leanback.v4"],
    &["com.beb.plxnative", "cdp-30"],
];

/// Where each app goes, by its place in the list it came in.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Arrangement {
    /// The row's apps, left to right; "Other apps" follows them when
    /// `others` has any.
    pub row: Vec<usize>,
    /// The chips, left to right.
    pub chips: Vec<usize>,
    /// The "Other apps" page, in the list's order.
    pub others: Vec<usize>,
}

/// Arranges `apps`. Without any of the contract's apps (another TV, or a
/// list that failed part way), the row takes the first apps that are not
/// inputs or Settings, so the home is never empty while apps exist.
pub fn arrange(apps: &[App]) -> Arrangement {
    let find = |id: &str| apps.iter().position(|app| app.id.as_str() == id);
    let mut row: Vec<usize> = CONTRACT
        .iter()
        .filter_map(|ids| ids.iter().find_map(|id| find(id)))
        .collect();
    if row.is_empty() {
        row = (0..apps.len())
            .filter(|&i| apps[i].kind == AppKind::App && apps[i].id.as_str() != SETTINGS)
            .take(ROW_MAX - 1)
            .collect();
    }
    let settings = find(SETTINGS);
    let room = CHIPS_MAX - usize::from(settings.is_some());
    let mut chips: Vec<usize> = (0..apps.len())
        .filter(|&i| apps[i].kind == AppKind::Input)
        .take(room)
        .collect();
    chips.extend(settings);
    let others = (0..apps.len())
        .filter(|i| !row.contains(i) && !chips.contains(i))
        .collect();
    Arrangement { row, chips, others }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apps::{Launcher, MockLauncher};

    fn titles(apps: &[App], at: &[usize]) -> Vec<String> {
        at.iter().map(|&i| apps[i].title.clone()).collect()
    }

    #[test]
    fn the_mock_puts_the_contract_in_the_row_and_the_inputs_in_chips() {
        let apps = MockLauncher::default().list().unwrap();
        let a = arrange(&apps);
        assert_eq!(
            titles(&apps, &a.row),
            ["Netflix", "Prime Video", "YouTube", "Plex"]
        );
        assert_eq!(
            titles(&apps, &a.chips),
            ["HDMI 1", "HDMI 2", "HDMI 3", "HDMI 4", "Settings"]
        );
        assert_eq!(
            titles(&apps, &a.others),
            ["Disney+", "Apple TV", "Spotify", "Live TV", "Web Browser"]
        );
    }

    #[test]
    fn plx_native_takes_the_plex_tile_before_lgs_plex() {
        let apps = vec![
            App::new("cdp-30", "Plex", AppKind::App),
            App::new("netflix", "Netflix", AppKind::App),
            App::new("com.beb.plxnative", "PlxNative", AppKind::App),
        ];
        let a = arrange(&apps);
        assert_eq!(a.row, [1, 2]);
        assert_eq!(a.others, [0], "LG's Plex is one press further");
    }

    #[test]
    fn without_the_contract_the_first_apps_take_the_row() {
        let apps: Vec<App> = ["a", "b", "c", "d", "e", "f"]
            .iter()
            .map(|id| App::new(id, id, AppKind::App))
            .chain([
                App::new(SETTINGS, "Settings", AppKind::App),
                App::new("in", "HDMI", AppKind::Input),
            ])
            .collect();
        let a = arrange(&apps);
        assert_eq!(a.row, [0, 1, 2, 3]);
        assert_eq!(a.chips, [7, 6]);
        assert_eq!(a.others, [4, 5]);
    }

    #[test]
    fn inputs_past_the_chips_room_go_to_other_apps_and_settings_stays() {
        let mut apps: Vec<App> = (0..9)
            .map(|i| App::new(&format!("in{i}"), &format!("Input {i}"), AppKind::Input))
            .collect();
        apps.push(App::new(SETTINGS, "Settings", AppKind::App));
        let a = arrange(&apps);
        assert_eq!(a.chips.len(), CHIPS_MAX);
        assert_eq!(a.chips.last(), Some(&9));
        assert_eq!(a.others, [6, 7, 8]);
        assert!(a.row.is_empty(), "inputs never take the row");
    }

    #[test]
    fn nothing_listed_arranges_nothing() {
        assert_eq!(arrange(&[]), Arrangement::default());
    }
}
