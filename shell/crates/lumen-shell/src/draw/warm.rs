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
use lumen_core::home::{TileView, View};
use lumen_core::layout;

use crate::atlas::Upload;
use crate::render::Batch;

use super::{Assets, paint};

/// Makes every glyph the apps' titles will need, as the screen draws them:
/// under the tile, on the Top Shelf, as a letter tile, and in the status
/// lines `Home::launched` writes. Each app is painted once per status line,
/// focused and without its icon, into a batch that is thrown away. The
/// glyphs' pixels wait in `assets.atlas` for the caller to upload. If the
/// atlas fills and is emptied part way, the glyphs made before are gone, so
/// it paints everything once more, as `draw::home` does.
pub fn warm(apps: &[App], assets: &mut Assets) {
    let generation = assets.atlas.generation();
    paint_all(apps, assets);
    if assets.atlas.generation() != generation {
        paint_all(apps, assets);
    }
}

fn paint_all(apps: &[App], assets: &mut Assets) {
    let mut scratch = Batch::default();
    for (index, app) in apps.iter().enumerate() {
        let plain = App {
            icon: None,
            ..app.clone()
        };
        for status in [
            format!("Opening {}", app.title),
            format!("Could not open {}", app.title),
        ] {
            let view = View {
                shelf: layout::TOP_SHELF,
                shelf_title: Some(&app.title),
                tiles: vec![TileView {
                    app: &plain,
                    rect: layout::lifted(layout::tile_rect(0), 1.0),
                    lift: 1.0,
                    focused: true,
                    index,
                }],
                status: Some(&status),
                empty: false,
            };
            paint(&mut scratch, &view, assets);
        }
    }
}

/// `warm: 106 glyphs, 103 uploads, 525 KB in 0.7 ms` (the simulator's 12
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
    use lumen_core::focus::Dir;
    use lumen_core::home::{BackAtTop, Effect, Home};
    use lumen_core::input::{Action, Press};

    use crate::draw;

    /// The atlas uploads in each burst of a walk over the mock grid (R×4, D,
    /// R, D, L, U, U, OK, BACK), drawn at 60 Hz as `--fixed-dt 0.016667`
    /// draws it. With `warmed`, `warm` ran after the first frame and its
    /// uploads went then.
    fn uploads_per_burst(warmed: bool) -> Vec<usize> {
        use Dir::{Down, Left, Right, Up};
        let mut home = Home::new(MockLauncher::default().list().unwrap(), BackAtTop::Exit);
        let mut assets = Assets::new().expect("the built-in font parses");
        let mut batch = Batch::default();
        draw::home(&mut batch, &home.view(), &mut assets);
        home.frame_drawn();
        if warmed {
            warm(home.apps(), &mut assets);
        }
        assets.atlas.take_uploads();
        let frame = Duration::from_nanos(16_666_667);
        let mut now = Duration::ZERO;
        let mut bursts = Vec::new();
        let walk = [Right, Right, Right, Right, Down, Right, Down, Left, Up, Up];
        let presses = walk
            .into_iter()
            .map(Action::Move)
            .chain([Action::Select, Action::Back]);
        for action in presses {
            now += Duration::from_millis(500);
            let press = Press {
                action,
                repeat: false,
            };
            if let Some(Effect::Launch(id)) = home.handle(press, now) {
                home.launched(&id, &Ok(()), now);
            }
            let mut uploads = 0;
            loop {
                home.advance_to(now);
                if home.is_settled() {
                    break;
                }
                draw::home(&mut batch, &home.view(), &mut assets);
                uploads += assets.atlas.take_uploads().len();
                home.frame_drawn();
                now += frame;
            }
            bursts.push(uploads);
        }
        bursts
    }

    #[test]
    fn warm_titles_leave_no_upload_for_a_burst() {
        let cold = uploads_per_burst(false);
        assert_eq!(cold.len(), 12);
        assert!(cold.iter().sum::<usize>() > 0, "{cold:?}");
        let warmed = uploads_per_burst(true);
        assert!(warmed.iter().all(|&n| n == 0), "{warmed:?}, cold {cold:?}");
    }

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
