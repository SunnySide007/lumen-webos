use super::*;
use crate::render::Shader;
use std::time::Duration;

use lumen_core::apps::{App, AppId, AppKind, Launcher, MockLauncher};
use lumen_core::art::Look;
use lumen_core::focus::Dir;
use lumen_core::home::{BackAtTop, Home};
use lumen_core::input::{Action, Press};

/// A guard against regressions, not a measured limit: B1 drew 856 to 1,631
/// quads at 60 fps on the B9, and the home draws far fewer. What limits a
/// frame, quads or shaded pixels, is for the bench to measure.
const QUAD_BUDGET: usize = 600;

fn mock_home() -> Home {
    Home::new(MockLauncher::default().list().unwrap(), BackAtTop::Exit)
}

fn assets() -> Assets {
    Assets::new().expect("the built-in font parses")
}

fn painted(home: &Home, assets: &mut Assets) -> Batch {
    let mut batch = Batch::default();
    super::home(&mut batch, &home.view(), assets);
    batch
}

fn quads(home: &Home, assets: &mut Assets) -> usize {
    painted(home, assets).quad_count()
}

fn press(dir: Dir) -> Press {
    Press {
        action: Action::Move(dir),
        repeat: false,
    }
}

fn act(action: Action) -> Press {
    Press {
        action,
        repeat: false,
    }
}

/// The ambient hero's three layers, in textures as the loop leaves them.
fn ambient(assets: &mut Assets) {
    let layers = [(0.0, 1.0), (0.0, 0.62), (0.7, 1.0)].map(|rows| HeroLayer {
        texture: Texture::Own(7),
        rows,
    });
    assets.heroes.insert(Hero::Ambient, layers.into());
}

#[test]
fn the_start_screen_fits_the_quad_budget() {
    let mut home = mock_home();
    home.set_time(21, 41);
    home.set_playing(Some(AppId::new("com.webos.app.hdmi1")));
    let n = quads(&home, &mut assets());
    assert!(n > 40, "{n} quads: nothing drawn?");
    assert!(n <= QUAD_BUDGET, "{n} quads");
}

#[test]
fn the_other_apps_page_scrolled_deep_fits_the_quad_budget() {
    let apps = MockLauncher::default()
        .list()
        .unwrap()
        .into_iter()
        .cycle()
        .take(60)
        .collect();
    let mut home = Home::new(apps, BackAtTop::Exit);
    let mut assets = assets();
    for _ in 0..4 {
        let _ = home.handle(press(Dir::Right), Duration::ZERO);
    }
    assert_eq!(home.handle(act(Action::Select), Duration::ZERO), None);
    for row in 1..=5 {
        let _ = home.handle(press(Dir::Down), Duration::ZERO);
        home.settle_now();
        home.show_status("Could not open Web Browser".to_owned(), Duration::ZERO);
        let n = quads(&home, &mut assets);
        assert!(n <= QUAD_BUDGET, "row {row}: {n} quads");
    }
}

#[test]
fn a_page_turning_draws_both_pages_and_fits_the_budget() {
    let mut home = mock_home();
    let mut assets = assets();
    for _ in 0..4 {
        let _ = home.handle(press(Dir::Right), Duration::ZERO);
    }
    home.settle_now();
    assert_eq!(home.handle(act(Action::Select), Duration::ZERO), None);
    home.advance_to(Duration::from_millis(50));
    let view = home.view();
    assert!(view.page > 0.0 && view.page < 1.0);
    let n = quads(&home, &mut assets);
    assert!(n <= QUAD_BUDGET, "{n} quads");
}

#[test]
fn a_newly_focused_tile_leans_whole_with_its_light_and_fits_the_budget() {
    let mut assets = assets();
    let mut home = mock_home().with_look(Look::FULL);
    assert_eq!(home.handle(press(Dir::Right), Duration::ZERO), None);
    // Halfway through the light, the lean on its way back.
    home.advance_to(style::SWEEP_TIME / 2);
    let tile = *home.view().tiles.last().unwrap();
    assert!(tile.tilt[0] > 0.0 && tile.sweep.is_some());
    let batch = painted(&home, &mut assets);
    // Prime Video's tile (five quads: `Batch::rounded_fill4`), its name
    // across it and the light's two quads lean; its shadow and every other
    // tile stay level.
    let name = "PrimeVideo".len();
    assert_eq!(batch.leaning_quads(), 5 + name + 2);
    let n = batch.quad_count();
    assert!(n <= QUAD_BUDGET, "{n} quads");
    // Settled, nothing leans.
    home.settle_now();
    assert_eq!(painted(&home, &mut assets).leaning_quads(), 0);
}

#[test]
fn the_art_is_drawn_first_by_its_own_program_over_the_whole_screen() {
    let mut home = mock_home().with_look(Look::FULL);
    let mut assets = assets();
    let layer = HeroLayer {
        texture: Texture::Own(9),
        rows: (0.0, 1.0),
    };
    let plex = Hero::Named("hero-plex");
    assets.heroes.insert(plex.clone(), vec![layer]);
    home.show_hero(Some(plex));
    home.settle_now();
    let batch = painted(&home, &mut assets);
    assert_eq!(batch.quad_bounds(0), (0.0, 0.0, SCREEN_W, SCREEN_H));
    let runs = batch.runs();
    assert!(runs[0].shader == Shader::Art && runs[0].texture == Texture::Own(9));
    assert_eq!(runs[0].count, 6, "the art alone");
    assert!(
        runs[1..].iter().all(|r| r.shader != Shader::Art),
        "the rest is the main program's and the fill's"
    );
    // Lumen's art has its fade drawn in: nothing goes over it. The owner's
    // gets a fade of its own, one quad.
    let owner = Hero::Owner(AppId::new("netflix"));
    assets.heroes.insert(owner.clone(), vec![layer]);
    let plain = batch.quad_count();
    home.show_hero(Some(owner));
    home.settle_now();
    let batch = painted(&home, &mut assets);
    assert_eq!(batch.quad_count(), plain + 1);
    assert_eq!(batch.quad_bounds(1).1, SCREEN_H * style::OWNER_SCRIM_FROM);
}

#[test]
fn without_art_the_gradient_fills_the_screen() {
    let batch = painted(&mock_home(), &mut assets());
    assert_eq!(batch.quad_bounds(0), (0.0, 0.0, SCREEN_W, SCREEN_H));
    assert!(batch.runs().iter().all(|r| r.shader != Shader::Art));
}

#[test]
fn the_ambient_heros_layers_drift_each_its_own_way_and_never_show_an_edge() {
    let mut assets = assets();
    let mut home = mock_home().with_look(Look::FULL);
    ambient(&mut assets);
    home.show_hero(Some(Hero::Ambient));
    home.settle_now();
    let windows =
        |home: &Home, assets: &mut Assets| painted(home, assets).image_uvs()[..3].to_vec();
    // At rest the three line up: one window, a little inside the art.
    let rest = windows(&home, &mut assets);
    assert!(rest.iter().all(|w| (w.0, w.2) == (rest[0].0, rest[0].2)));
    assert!(rest[0].0 > 0.0 && rest[0].2 < 1.0, "{rest:?}");
    assert_eq!(home.handle(press(Dir::Right), Duration::ZERO), None);
    home.advance_to(Duration::from_millis(70));
    let moved = windows(&home, &mut assets);
    let shift: Vec<f32> = moved.iter().zip(&rest).map(|(m, r)| m.0 - r.0).collect();
    assert!(shift[0] > 0.0, "{shift:?}");
    for (i, px) in style::PARALLAX_PX.iter().enumerate() {
        let share = shift[i] / shift[0];
        assert!(
            (share - px / style::PARALLAX_PX[0]).abs() < 0.01,
            "{shift:?}"
        );
    }
    for w in &moved {
        assert!(
            w.0 >= 0.0 && w.1 >= 0.0 && w.2 <= 1.0 && w.3 <= 1.0,
            "{moved:?}"
        );
    }
}

#[test]
fn the_sheen_costs_no_quad_and_each_resting_shadow_four_round_its_tile() {
    let mut assets = assets();
    let mut plain = mock_home();
    plain.settle_now();
    let base = quads(&plain, &mut assets);
    let sheen = Look {
        sheen: true,
        ..Look::PLAIN
    };
    let mut lit = mock_home().with_look(sheen);
    lit.settle_now();
    assert_eq!(quads(&lit, &mut assets), base);
    let shadow = Look {
        shadow: true,
        ..Look::PLAIN
    };
    let mut shaded = mock_home().with_look(shadow);
    shaded.settle_now();
    // Each round the part its opaque tile hides (`Batch::shadow_around`).
    let resting = shaded.view().tiles.len() - 1;
    assert_eq!(quads(&shaded, &mut assets), base + 4 * resting);
}

#[test]
fn what_plays_adds_its_dot_by_the_clock_and_on_its_chip() {
    let mut assets = assets();
    let mut home = mock_home();
    home.set_time(9, 41);
    home.settle_now();
    let before = quads(&home, &mut assets);
    home.set_playing(Some(AppId::new("com.webos.app.hdmi2")));
    // Two dots and "HDMI2" by the clock.
    assert_eq!(quads(&home, &mut assets), before + 2 + "HDMI2".len());
}

#[test]
fn the_empty_state_draws() {
    let home = Home::new(Vec::new(), BackAtTop::Exit);
    assert!(quads(&home, &mut assets()) > 2);
}

#[test]
fn a_tile_takes_its_apps_colour_or_the_palettes_and_text_that_reads_on_it() {
    let app = App::new("a", "A", AppKind::App);
    assert_eq!(app_color(&app, 3), tile_color(3));
    let app = app.with_color([1, 2, 3]);
    assert_eq!(app_color(&app, 3), Rgba::rgb8(1, 2, 3));
    assert_eq!(on_color(Rgba::rgb8(255, 255, 255)), ON_LIGHT_TILE);
    assert_eq!(on_color(Rgba::rgb8(0, 0, 0)), ON_TILE);
    assert_eq!(on_color(tile_color(5)), ON_TILE);
}

#[test]
fn a_missing_icon_falls_back_to_the_name() {
    let mut assets = assets();
    let app = App::new("a", "plex", AppKind::App).with_icon("/no/such/icon.png");
    let mut home = Home::new(vec![app], BackAtTop::Exit);
    home.settle_now();
    let with_missing = quads(&home, &mut assets);
    let mut plain = Home::new(vec![App::new("a", "plex", AppKind::App)], BackAtTop::Exit);
    plain.settle_now();
    assert_eq!(with_missing, quads(&plain, &mut assets));
}

#[test]
fn a_frame_that_fills_the_atlas_never_points_at_stale_pixels() {
    // Canonical, as the TV's launcher gives them.
    let icons = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../assets/mock-icons")
        .canonicalize()
        .unwrap();
    let apps = MockLauncher::default()
        .with_icons_in(&icons)
        .list()
        .unwrap();
    let mut home = Home::new(apps, BackAtTop::Exit);
    home.settle_now();
    home.set_time(9, 41);
    home.show_status("Opening Netflix".to_owned(), Duration::ZERO);
    let clean = quads(&home, &mut assets());

    // Leave R rows of the atlas free, so the frame fills it at a different
    // point each time: early, in the middle, or not at all.
    let mut emptied = 0;
    for free in (8..=320).step_by(8) {
        let mut assets = assets();
        let (w, h) = (2046, 1022 - free);
        assets
            .atlas
            .insert(w, h, &vec![0; (w * h * 4) as usize])
            .unwrap();
        let before = assets.atlas.generation();
        let batch = painted(&home, &mut assets);
        if assets.atlas.generation() != before {
            emptied += 1;
        }
        assert_eq!(batch.quad_count(), clean, "{free} rows free");
        // Every image must sample pixels placed in the current atlas.
        let live: Vec<_> = assets.atlas.take_uploads().iter().map(content_uv).collect();
        for uv in batch.image_uvs() {
            let inside = live.iter().any(|r| {
                uv.0 >= r.0 - 1e-6 && uv.1 >= r.1 - 1e-6 && uv.2 <= r.2 + 1e-6 && uv.3 <= r.3 + 1e-6
            });
            assert!(inside, "{free} rows free: {uv:?} is stale");
        }
    }
    assert!(
        emptied > 10,
        "the sweep must empty the atlas mid-frame: {emptied}"
    );
}

/// The texture coordinates of an upload's pixels, inside its padding.
fn content_uv(up: &crate::atlas::Upload) -> (f32, f32, f32, f32) {
    use crate::atlas::{ATLAS_H, ATLAS_W};
    let (aw, ah) = (ATLAS_W as f32, ATLAS_H as f32);
    (
        (up.x + 1) as f32 / aw,
        (up.y + 1) as f32 / ah,
        (up.x + up.w - 1) as f32 / aw,
        (up.y + up.h - 1) as f32 / ah,
    )
}

#[test]
fn no_tile_shares_a_colour_with_its_neighbours() {
    let cols = layout::GRID_COLUMNS;
    for i in 0..40 {
        if (i + 1) % cols != 0 {
            assert_ne!(tile_color(i), tile_color(i + 1), "tiles {i} and the next");
        }
        assert_ne!(tile_color(i), tile_color(i + cols), "tile {i} and below");
    }
}

/// The home with focus on the gear and the Control Center open on `look`,
/// its Privacy panel read, settled.
fn control_open(look: Look) -> Home {
    let mut home = mock_home().with_look(look);
    let _ = home.handle(press(Dir::Up), Duration::ZERO);
    let _ = home.handle(act(Action::Select), Duration::ZERO);
    let read =
        "mode block\nupdated 9\nlookups_today 6789\nblocked_today 512\n--- rules\n--- jobs\n";
    home.set_privacy(lumen_core::privacy::Reading::Read(
        lumen_core::privacy::parse(read, 9),
    ));
    home.settle_now();
    home
}

#[test]
fn a_chip_sized_to_its_name_draws_it_whole() {
    let mut assets = assets();
    let mut apps = MockLauncher::default().list().unwrap();
    let input = apps.iter_mut().find(|a| a.kind == AppKind::Input).unwrap();
    input.title = "Google Streamer".to_owned();
    let mut home = Home::new(apps, BackAtTop::Exit).with_look(Look::DEFAULT);
    let text = Text::new().unwrap();
    home.size_chips(|title| text.width(title, style::CHIP_PX));
    for focused in [false, true] {
        if focused {
            let _ = home.handle(press(Dir::Down), Duration::ZERO);
        }
        home.settle_now();
        let view = home.view();
        let chip = view.chips[0];
        assert_eq!(chip.app.title, "Google Streamer");
        assert_eq!(chip.lift, if focused { 1.0 } else { 0.0 });
        let px = (style::CHIP_PX * layout::CHIP_FOCUS_SCALE).round();
        let zoom = chip.rect.h / layout::CHIP_H;
        let room = (chip.rect.w / zoom - 2.0 * style::CHIP_PAD_X) * layout::CHIP_FOCUS_SCALE;
        assert_eq!(
            assets.text.fit(&chip.app.title, px, room),
            "Google Streamer"
        );
        assert!(painted(&home, &mut assets).quad_count() > 0);
    }
}

#[test]
fn the_gear_is_a_button_and_a_cog_of_ten_circles() {
    let mut assets = assets();
    let mut home = mock_home();
    home.settle_now();
    let batch = painted(&home, &mut assets);
    let gear = home.view().gear.expect("on the home").rect;
    let inside = (0..batch.quad_count())
        .filter(|&i| {
            let (x0, y0, x1, y1) = batch.quad_bounds(i);
            let pad = 1.5;
            x0 >= gear.x - pad
                && y0 >= gear.y - pad
                && x1 <= gear.right() + pad
                && y1 <= gear.y + gear.h + pad
        })
        .count();
    assert_eq!(inside, 1 + 8 + 2);
}

#[test]
fn the_control_center_fits_the_budget_and_dims_only_with_the_looks_dim() {
    let mut assets = assets();
    let dimmed = painted(&control_open(Look::FULL), &mut assets);
    let n = dimmed.quad_count();
    assert!(n <= QUAD_BUDGET, "{n} quads");
    let undimmed = painted(
        &control_open(Look {
            dim: false,
            ..Look::FULL
        }),
        &mut assets,
    );
    // The dim is no quad: the home under it is darker by the dim's alpha,
    // and the panel, drawn last, is not.
    assert_eq!(undimmed.quad_count(), n);
    let k = 1.0 - PANEL_DIM.3;
    let (home, panel) = (undimmed.quad_color(0), undimmed.quad_color(n - 1));
    assert_eq!(
        dimmed.quad_color(0),
        Rgba(home.0 * k, home.1 * k, home.2 * k, home.3)
    );
    assert_eq!(dimmed.quad_color(n - 1), panel);
    // On its way in, it slides from the right.
    let mut home = mock_home().with_look(Look::FULL);
    let _ = home.handle(press(Dir::Up), Duration::ZERO);
    let _ = home.handle(act(Action::Select), Duration::ZERO);
    home.advance_to(Duration::from_millis(50));
    let control = home.view().control.expect("opening");
    assert!(control.offset > 0.0 && control.shown < 1.0);
    assert!(quads(&home, &mut assets) <= QUAD_BUDGET);
}

/// The main program's pixels in a frame, in screens: flat, shape and image
/// quads (`Batch::shaded`).
fn main_pixels(batch: &Batch) -> f32 {
    batch.shaded()[2..].iter().sum()
}

/// The main program's pixels a frame may shade, in screens: about what
/// the home shaded when it held its budget on the B9 (0.40 screens at
/// 16.9 ms a frame, 2026-11-09, 9d5a988), where a screen of them cost
/// about 11 ms ("Other apps" at 1.03 and the Control Center at 1.73 ran at
/// about 25 and 28 ms).
const MAIN_PIXEL_BUDGET: f32 = 0.4;

/// The fill's pixels a frame may shade, in screens: the Control Center
/// held its budget with 0.28 (2026-11-10, bc053bd: 16.84 ms a frame).
const FILL_PIXEL_BUDGET: f32 = 0.4;

#[test]
fn every_screen_keeps_to_the_main_programs_pixel_budget() {
    let mut assets = assets();
    ambient(&mut assets);
    let check = |name: &str, home: &Home, assets: &mut Assets| {
        let batch = painted(home, assets);
        let s = batch.shaded();
        let main = main_pixels(&batch);
        assert!(main <= MAIN_PIXEL_BUDGET, "{name}: {s:?}");
        assert!(s[1] <= FILL_PIXEL_BUDGET, "{name}: {s:?}");
        assert!((s[0] - 1.92).abs() < 0.01, "{name}: the art's three layers");
        main
    };
    // The B9's 30 apps, with the ambient hero behind them, at the look the
    // owner sees.
    let apps = MockLauncher::default()
        .list()
        .unwrap()
        .into_iter()
        .cycle()
        .take(30)
        .collect();
    let mut home = Home::new(apps, BackAtTop::Exit).with_look(Look::DEFAULT);
    home.show_hero(Some(Hero::Ambient));
    home.settle_now();
    check("home", &home, &mut assets);
    let _ = home.handle(press(Dir::Right), Duration::ZERO);
    home.advance_to(style::SWEEP_TIME / 2);
    check("home, moving", &home, &mut assets);
    home.settle_now();
    for _ in 0..3 {
        let _ = home.handle(press(Dir::Right), Duration::ZERO);
    }
    home.settle_now();
    assert_eq!(home.handle(act(Action::Select), Duration::ZERO), None);
    home.advance_to(Duration::from_millis(150));
    check("the page turning", &home, &mut assets);
    home.settle_now();
    assert_eq!(home.view().page, 1.0);
    check("other apps", &home, &mut assets);
    for (i, dir) in [Dir::Right, Dir::Down, Dir::Down].into_iter().enumerate() {
        let _ = home.handle(press(dir), Duration::ZERO);
        home.advance_to(style::SWEEP_TIME / 2);
        check(&format!("other apps, moving {i}"), &home, &mut assets);
        home.settle_now();
    }
    let mut home = control_open(Look::DEFAULT);
    home.show_hero(Some(Hero::Ambient));
    home.settle_now();
    let main = check("the control center", &home, &mut assets);
    assert!(main > 0.0);
}

#[test]
fn the_control_centers_notes_fit_its_panel() {
    let text = Text::new().unwrap();
    let room = layout::PANEL_W - 2.0 * layout::PANEL_PAD;
    for note in [PRIVACY_NOTE, REDUCE_MOTION_NOTE] {
        let w = text.width(note, style::NOTE_PX);
        assert!(w <= room, "{note}: {w} of {room}");
    }
    // The note sits under the switch, inside the panel.
    let p = layout::panel(0.0);
    let row = layout::motion_row(p, 0);
    let note = row.bottom() + layout::MOTION_NOTE_GAP + style::NOTE_PX;
    assert!(note < p.bottom() - layout::PANEL_PAD, "{note}");
}
