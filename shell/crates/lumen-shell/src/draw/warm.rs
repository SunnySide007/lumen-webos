//! Glyphs made ahead (docs/specs/shell-mvp.md, "Frame time: finding the
//! missed vsyncs", stage 3).
//!
//! The first time a title is drawn, its glyphs are rasterised on the main
//! thread and uploaded into the atlas that the same frame reads. On the B9 an
//! upload in a burst cost 9–25 ms, the driver waiting for the GPU to finish
//! with the texture ("Measured on the B9", 14). `warm` makes every title's
//! glyphs while the screen is settled, so a burst neither rasterises nor
//! uploads them.

use std::time::Duration;

use lumen_core::apps::App;
use lumen_core::art::Look;
use lumen_core::control::{ControlView, Motion, MotionRow};
use lumen_core::home::{CONTROL_SUBTITLE, CONTROL_TITLE, ChipView, TileView, View};
use lumen_core::layout;
use lumen_core::privacy::Line;
use lumen_core::shelf::ShelfView;

use crate::atlas::Upload;
use crate::render::Batch;

use super::{Assets, paint};

/// The lines under a name the home can write, besides an app's own status
/// lines: their glyphs, digits included, are made once.
const SUBTITLES: [&str; 8] = [
    "Playing now",
    "Opened just now",
    "Opened a minute ago",
    "Opened 0123456789 minutes ago",
    "Opened an hour ago",
    "Opened yesterday",
    "Opened 2 days ago",
    "1 more app, 2 more apps",
];

/// What the Privacy panel's values can say (`privacy::lines`), the digits
/// and the thousands' comma included.
const PRIVACY_VALUES: [&str; 8] = [
    "Reading…",
    "Blocking, Logging only, Not running, Not installed",
    "Can't read: no answer, no output, the read stopped",
    "0123456789 of 0123456789 lookups blocked, would be blocked",
    "No lookups counted",
    "All stopped, 1 of 4 jobs running",
    "Closed, Open, Unknown",
    "Lumen Guard, Today, LG's reporting, Port 11111",
];

/// Makes every glyph the apps' names will need, as the screen draws them:
/// above the row, across a tile, on a chip, under a tile on the "Other
/// apps" page, by the clock, and in the status lines `Home::launched`
/// writes; and the clock's digits, the subtitles and the page's heading;
/// and the Control Center's.
/// Each app is painted once per status line, focused and without its icon,
/// into a batch that is thrown away. The glyphs' pixels wait in
/// `assets.atlas` for the caller to upload. If the atlas fills and is
/// emptied part way, the glyphs made before are gone, so it paints
/// everything once more, as `draw::home` does.
pub fn warm(apps: &[App], assets: &mut Assets) {
    let generation = assets.atlas.generation();
    paint_all(apps, assets);
    if assets.atlas.generation() != generation {
        paint_all(apps, assets);
    }
}

fn paint_all(apps: &[App], assets: &mut Assets) {
    let mut scratch = Batch::default();
    let subtitles = SUBTITLES.iter().map(|s| (*s).to_owned());
    for (index, app) in apps.iter().enumerate() {
        let plain = App {
            icon: None,
            ..app.clone()
        };
        let statuses = [
            format!("Opening {}", app.title),
            format!("Could not open {}", app.title),
        ];
        // The first app carries the subtitles and the clock's digits too.
        let extra: Vec<String> = if index == 0 {
            subtitles.clone().collect()
        } else {
            Vec::new()
        };
        for (n, status) in statuses.iter().chain(extra.iter()).enumerate() {
            let view = View {
                art: ShelfView::PLACEHOLDER,
                drift: [0.0; 2],
                // Half turned, so the name above the row and the page's
                // heading both draw.
                page: 0.5,
                page_slide: lumen_core::style::PAGE_SLIDE,
                title: Some(&app.title),
                subtitle: Some(status.clone()),
                heading_y: layout::HEADING_BASELINE,
                clock: (n == 0).then(|| "0123456789:".to_owned()),
                playing: Some(&app.title),
                tiles: vec![TileView {
                    app: &plain,
                    rect: layout::lifted(layout::row_tile(0), 1.0),
                    lift: 1.0,
                    focused: true,
                    index,
                    tilt: [0.0; 2],
                    sweep: None,
                    alpha: 1.0,
                    named: true,
                }],
                chips: vec![ChipView {
                    app: &plain,
                    rect: layout::chip_lifted(layout::chip_rect(0), 1.0),
                    lift: 1.0,
                    focused: true,
                    playing: true,
                    alpha: 1.0,
                }],
                status: (n < statuses.len()).then_some(status.as_str()),
                empty: false,
                look: Look::PLAIN,
                gear: None,
                control: None,
            };
            paint(&mut scratch, &view, assets);
        }
    }
    paint(&mut scratch, &control_view(), assets);
}

/// The home with focus on the gear and the Control Center open, its lines
/// saying everything they can.
fn control_view() -> View<'static> {
    let privacy = PRIVACY_VALUES
        .iter()
        .map(|value| Line {
            label: "Lumen Guard",
            value: (*value).to_owned(),
            warn: false,
        })
        .collect();
    let motion = Motion::ALL
        .iter()
        .map(|m| MotionRow {
            label: m.label(),
            on: true,
            editable: true,
            focused: false,
            lift: 0.0,
        })
        .collect();
    View {
        art: ShelfView::PLACEHOLDER,
        drift: [0.0; 2],
        page: 0.0,
        page_slide: lumen_core::style::PAGE_SLIDE,
        title: Some(CONTROL_TITLE),
        subtitle: Some(CONTROL_SUBTITLE.to_owned()),
        heading_y: layout::HEADING_BASELINE,
        clock: None,
        playing: None,
        tiles: Vec::new(),
        chips: Vec::new(),
        status: None,
        empty: false,
        look: Look::PLAIN,
        gear: None,
        control: Some(ControlView {
            shown: 1.0,
            offset: 0.0,
            privacy,
            motion,
        }),
    }
}

/// `warm: 106 glyphs, 103 uploads, 525 KB in 0.7 ms` (the simulator's
/// mock apps, on the PC): what `warm` made and how long it and the uploads
/// took.
pub fn line(glyphs: u32, uploads: &[Upload], took: Duration) -> String {
    let bytes: usize = uploads.iter().map(|u| u.rgba.len()).sum();
    format!(
        "warm: {glyphs} glyphs, {} uploads, {} KB in {:.1} ms",
        uploads.len(),
        bytes / 1024,
        took.as_secs_f64() * 1000.0
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    use lumen_core::apps::{Launcher, MockLauncher};

    // That warm titles leave no upload for a burst is the home loop's test.

    #[test]
    fn warming_makes_each_title_once() {
        let apps = MockLauncher::default().list().unwrap();
        let mut assets = Assets::new().expect("the built-in font parses");
        warm(&apps, &mut assets);
        assert!(assets.text.take_rasterised() > 0);
        assert!(!assets.atlas.take_uploads().is_empty());
        warm(&apps, &mut assets);
        assert_eq!(
            assets.text.take_rasterised(),
            0,
            "all cached the first time"
        );
        assert!(assets.atlas.take_uploads().is_empty());
    }

    #[test]
    fn the_warm_line_counts_glyphs_uploads_and_kilobytes() {
        let upload = |kb: usize| Upload {
            x: 0,
            y: 0,
            w: 1,
            h: 1,
            rgba: vec![0; kb * 1024],
        };
        assert_eq!(
            line(
                106,
                &[upload(500), upload(25)],
                Duration::from_micros(2_400)
            ),
            "warm: 106 glyphs, 2 uploads, 525 KB in 2.4 ms"
        );
    }
}
