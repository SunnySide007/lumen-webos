use super::*;
use crate::apps::{AppKind, Launcher, MockLauncher};

const FRAME: Duration = Duration::from_micros(16_667);

fn mock_apps() -> Vec<App> {
    MockLauncher::default().list().expect("mock lists")
}

/// Drives a `Home` the way the shell's loop does: events at `now`, then
/// `advance_to(now)`, then a frame only if the screen is not settled.
struct Sim {
    home: Home,
    now: Duration,
}

impl Sim {
    fn new(apps: Vec<App>, back_at_top: BackAtTop) -> Self {
        Self::with(Home::new(apps, back_at_top))
    }

    fn with(home: Home) -> Self {
        let mut sim = Self {
            home,
            now: Duration::ZERO,
        };
        sim.draw_if_needed();
        sim
    }

    fn mock() -> Self {
        Self::new(mock_apps(), BackAtTop::Exit)
    }

    fn full() -> Self {
        Self::with(Home::new(mock_apps(), BackAtTop::Exit).with_look(Look::FULL))
    }

    fn press(&mut self, action: Action) -> Option<Effect> {
        self.home.handle(
            Press {
                action,
                repeat: false,
            },
            self.now,
        )
    }

    fn presses(&mut self, actions: &[Action]) {
        for &action in actions {
            assert_eq!(self.press(action), None, "{action:?}");
        }
    }

    fn draw_if_needed(&mut self) -> bool {
        if self.home.is_settled() {
            return false;
        }
        let _ = self.home.view();
        self.home.frame_drawn();
        true
    }

    fn frame(&mut self) -> bool {
        self.now += FRAME;
        self.home.advance_to(self.now);
        self.draw_if_needed()
    }

    /// Runs frames until one is not needed; returns how many were drawn.
    fn settle(&mut self) -> usize {
        let mut frames = 0;
        while self.frame() {
            frames += 1;
            assert!(frames < 600, "never settled");
        }
        frames
    }

    fn focused_title(&self) -> &str {
        let i = self.home.focused().expect("focus");
        &self.home.apps()[i].title
    }

    fn tile(&self, title: &str) -> TileView<'_> {
        *self
            .home
            .view()
            .tiles
            .iter()
            .find(|t| t.app.title == title)
            .expect("tile on screen")
    }

    fn chip(&self, title: &str) -> ChipView<'_> {
        *self
            .home
            .view()
            .chips
            .iter()
            .find(|c| c.app.title == title)
            .expect("chip on screen")
    }
}

const RIGHT: Action = Action::Move(Dir::Right);
const LEFT: Action = Action::Move(Dir::Left);
const DOWN: Action = Action::Move(Dir::Down);
const UP: Action = Action::Move(Dir::Up);
const OK: Action = Action::Select;
const BACK: Action = Action::Back;

#[test]
fn starts_on_the_first_tile_lifted_with_the_row_and_chips_on_screen() {
    let mut home = Home::new(mock_apps(), BackAtTop::Exit);
    assert!(!home.is_settled(), "the first frame is owed");
    assert!(!home.is_animating());
    home.frame_drawn();
    assert!(home.is_settled());
    let view = home.view();
    let titles: Vec<&str> = view.tiles.iter().map(|t| t.app.title.as_str()).collect();
    // The focused tile is drawn last.
    assert_eq!(
        titles,
        ["Prime Video", "YouTube", "Plex", "Other apps", "Netflix"]
    );
    let netflix = view.tiles.last().unwrap();
    assert!(netflix.focused && netflix.lift == 1.0);
    assert_eq!(netflix.rect, layout::lifted(layout::row_tile(0), 1.0));
    let chips: Vec<&str> = view.chips.iter().map(|c| c.app.title.as_str()).collect();
    assert_eq!(chips, ["HDMI 1", "HDMI 2", "HDMI 3", "HDMI 4", "Settings"]);
    assert_eq!(view.title, Some("Netflix"));
    assert_eq!(
        (view.subtitle, view.clock, view.playing),
        (None, None, None)
    );
    assert_eq!(view.page, 0.0);
}

#[test]
fn right_hands_the_lift_along_the_row_and_the_title_follows() {
    let mut sim = Sim::mock();
    assert_eq!(sim.press(RIGHT), None);
    assert!(!sim.home.is_settled());
    let frames = sim.settle();
    assert!((10..30).contains(&frames), "{frames} frames");
    assert_eq!(sim.focused_title(), "Prime Video");
    assert_eq!(sim.home.view().title, Some("Prime Video"));
    assert_eq!(sim.tile("Netflix").lift, 0.0);
    assert_eq!(sim.tile("Prime Video").lift, 1.0);
}

#[test]
fn down_and_up_go_to_the_nearest_item_across() {
    let mut sim = Sim::mock();
    sim.presses(&[RIGHT, DOWN]);
    // Prime Video's middle is at 600 px: HDMI 3's chip (678) is nearest.
    assert_eq!(sim.focused_title(), "HDMI 3");
    sim.settle();
    assert_eq!(sim.chip("HDMI 3").lift, 1.0);
    assert_eq!(sim.tile("Prime Video").lift, 0.0);
    assert_eq!(sim.home.view().title, Some("HDMI 3"));
    sim.presses(&[RIGHT, RIGHT, UP]);
    // Settings' chip (1166) is nearest Plex's tile (1320).
    assert_eq!(sim.focused_title(), "Plex");
    sim.presses(&[LEFT, LEFT, LEFT, DOWN]);
    assert_eq!(sim.focused_title(), "HDMI 1");
}

#[test]
fn chips_are_sized_to_their_names_and_up_and_down_follow_their_middles() {
    // The B9's inputs as the owner named them.
    let mut apps = mock_apps();
    let named = [("PC", 0), ("Home Theater", 1), ("Google Streamer", 2)];
    for (name, k) in named {
        let input = apps
            .iter_mut()
            .filter(|a| a.kind == AppKind::Input)
            .nth(k)
            .unwrap();
        input.title = name.to_owned();
    }
    let mut home = Home::new(apps, BackAtTop::Stay);
    // 15 px a character stands in for the font.
    home.size_chips(|title| title.chars().count() as f32 * 15.0);
    let mut sim = Sim::with(home);
    sim.settle();
    let view = sim.home.view();
    let widths: Vec<f32> = view.chips.iter().map(|c| c.rect.w).collect();
    let want = |name: &str| layout::chip_width(name.len() as f32 * 15.0);
    assert_eq!(
        widths[..3],
        [want("PC"), want("Home Theater"), want("Google Streamer")]
    );
    assert_eq!(widths[0], layout::CHIP_MIN_W);
    assert!(widths[2] > widths[1] && widths[1] > layout::CHIP_W);
    for pair in view.chips.windows(2) {
        assert!((pair[1].rect.x - pair[0].rect.right() - layout::CHIP_GAP).abs() < 1e-3);
    }
    // Down from Prime Video (its middle at 600 px) lands on the chip whose
    // middle is nearest, as the chips now lie.
    let nearest = view
        .chips
        .iter()
        .min_by(|a, b| {
            let d = |c: &&ChipView<'_>| (c.rect.centre_x() - 600.0).abs();
            d(a).total_cmp(&d(b))
        })
        .map(|c| c.app.title.clone())
        .unwrap();
    sim.presses(&[RIGHT, DOWN]);
    assert_eq!(sim.focused_title(), nearest);
    assert_eq!(nearest, "Google Streamer");
}

#[test]
fn edges_bump_and_spring_back_and_chips_never_lean() {
    let mut sim = Sim::full();
    let rest = sim.tile("Netflix").rect;
    for (action, dx, dy) in [(LEFT, -1.0, 0.0)] {
        assert_eq!(sim.press(action), None);
        for _ in 0..3 {
            sim.frame();
        }
        let r = sim.tile("Netflix").rect;
        assert!(((r.x - rest.x) * dx - BUMP_PEAK).abs() < 0.6 || dx == 0.0);
        assert!(((r.y - rest.y) * dy - BUMP_PEAK).abs() < 0.6 || dy == 0.0);
        sim.settle();
        assert_eq!(sim.tile("Netflix").rect, rest);
    }
    sim.presses(&[DOWN]);
    sim.settle();
    let chip = sim.chip("HDMI 1").rect;
    assert_eq!(sim.press(DOWN), None);
    sim.frame();
    sim.frame();
    assert!(sim.chip("HDMI 1").rect.y > chip.y, "bumped down");
    assert!(
        sim.home.view().tiles.iter().all(|t| t.tilt == [0.0; 2]),
        "a chip's bump leans no tile"
    );
    sim.settle();
    assert_eq!(sim.chip("HDMI 1").rect, chip);
}

#[test]
fn a_repeat_into_an_edge_stops_silently() {
    let mut sim = Sim::mock();
    let effect = sim.home.handle(
        Press {
            action: LEFT,
            repeat: true,
        },
        sim.now,
    );
    assert_eq!(effect, None);
    assert!(sim.home.is_settled());
}

#[test]
fn back_from_a_chip_goes_up_and_back_on_the_row_returns_to_what_played() {
    let mut sim = Sim::mock();
    sim.presses(&[RIGHT, RIGHT, RIGHT, DOWN]);
    assert_eq!(sim.focused_title(), "Settings");
    assert_eq!(sim.press(BACK), None);
    assert_eq!(sim.focused_title(), "Plex");
    // Nothing played: the policy says.
    assert_eq!(sim.press(BACK), Some(Effect::Exit));
    let mut stay = Sim::new(mock_apps(), BackAtTop::Stay);
    assert_eq!(stay.press(BACK), None);
    assert!(stay.home.is_settled(), "nothing to redraw");
    // Something played: Back goes back to it, whatever the policy.
    let hdmi1 = AppId::new("com.webos.app.hdmi1");
    stay.home.set_playing(Some(hdmi1.clone()));
    assert!(!stay.home.is_settled(), "the clock's line shows it");
    assert_eq!(stay.press(BACK), Some(Effect::Launch(hdmi1)));
}

#[test]
fn what_plays_shows_by_the_clock_on_its_chip_and_in_its_subtitle() {
    let mut sim = Sim::mock();
    sim.home
        .set_playing(Some(AppId::new("com.webos.app.hdmi2")));
    sim.home.set_time(21, 41);
    let view = sim.home.view();
    assert_eq!(view.playing, Some("HDMI 2"));
    assert_eq!(view.clock.as_deref(), Some("9:41"));
    let playing: Vec<_> = view.chips.iter().filter(|c| c.playing).collect();
    assert_eq!(playing.len(), 1);
    assert_eq!(playing[0].app.title, "HDMI 2");
    sim.presses(&[DOWN, RIGHT]);
    assert_eq!(sim.home.view().subtitle.as_deref(), Some("Playing now"));
    // An app not on the home still names itself.
    sim.home.set_playing(Some(AppId::new("com.example.gone")));
    assert_eq!(sim.home.view().playing, Some("com.example.gone"));
}

#[test]
fn the_clock_owes_a_frame_only_when_its_minute_changes() {
    let mut sim = Sim::mock();
    sim.home.set_time(0, 5);
    assert_eq!(sim.home.view().clock.as_deref(), Some("12:05"));
    sim.draw_if_needed();
    sim.home.set_time(0, 5);
    assert!(sim.home.is_settled());
    sim.home.set_time(13, 0);
    assert!(!sim.home.is_settled());
    assert_eq!(sim.home.view().clock.as_deref(), Some("1:00"));
    assert_eq!(clock_text(12, 30), "12:30");
}

#[test]
fn select_launches_with_a_press_pulse_and_the_app_plays_after() {
    let mut sim = Sim::mock();
    sim.press(RIGHT);
    sim.settle();
    assert_eq!(sim.press(OK), Some(Effect::Launch(AppId::new("amazon"))));
    let mut lowest = 1.0f32;
    while sim.frame() {
        lowest = lowest.min(sim.tile("Prime Video").lift);
    }
    assert!((lowest - PRESS_DIP).abs() < 0.05, "dipped to {lowest}");
    let id = AppId::new("amazon");
    sim.home.launched(&id, &Ok(()), sim.now);
    assert_eq!(sim.home.playing(), Some(&id));
    assert_eq!(sim.home.view().subtitle.as_deref(), Some("Playing now"));
    // Once something else plays, the subtitle says when it was opened.
    sim.home.set_playing(None);
    sim.home.advance_to(sim.now + Duration::from_secs(150));
    assert_eq!(
        sim.home.view().subtitle.as_deref(),
        Some("Opened 2 minutes ago")
    );
    // A launch that failed changes neither.
    sim.home.launched(
        &AppId::new("netflix"),
        &Err(LaunchError("x".into())),
        sim.now,
    );
    assert_eq!(sim.home.playing(), None);
}

#[test]
fn opened_ago_reads_naturally() {
    let s = Duration::from_secs;
    for (ago, text) in [
        (s(30), "Opened just now"),
        (s(60), "Opened a minute ago"),
        (s(59 * 60), "Opened 59 minutes ago"),
        (s(3600), "Opened an hour ago"),
        (s(5 * 3600), "Opened 5 hours ago"),
        (s(30 * 3600), "Opened yesterday"),
        (s(80 * 3600), "Opened 3 days ago"),
    ] {
        assert_eq!(opened_ago(ago), text);
    }
}

#[test]
fn other_apps_turns_to_its_page_and_back_returns_to_its_tile() {
    let mut sim = Sim::full();
    sim.presses(&[RIGHT, RIGHT, RIGHT, RIGHT]);
    assert_eq!(sim.focused_title(), "Other apps");
    assert_eq!(sim.home.view().subtitle.as_deref(), Some("5 more apps"));
    let wanted = sim.home.wanted_hero().cloned();
    assert_eq!(sim.press(OK), None, "no launch");
    assert!(sim.home.on_others());
    assert_eq!(sim.focused_title(), "Disney+");
    // The pages hand over: early in the turn the home is going and the
    // other page not yet there; late in it, the other way about.
    sim.frame();
    sim.frame();
    let view = sim.home.view();
    assert!(view.page > 0.0 && view.page < 0.4, "{}", view.page);
    assert!(!view.tiles.iter().any(|t| t.named) && !view.chips.is_empty());
    while sim.home.view().page < 0.6 {
        sim.frame();
    }
    let view = sim.home.view();
    assert!(view.page < 1.0);
    assert!(view.tiles.iter().all(|t| t.named) && view.chips.is_empty());
    sim.settle();
    let view = sim.home.view();
    assert_eq!(view.page, 1.0);
    assert!(view.chips.is_empty() && view.title.is_none());
    assert!(view.tiles.iter().all(|t| t.named && t.alpha == 1.0));
    assert_eq!(view.tiles.len(), 5);
    assert_eq!(view.heading_y, layout::HEADING_BASELINE);
    // The art stays the "Other apps" tile's while focus moves on the page.
    sim.presses(&[RIGHT, RIGHT, RIGHT]);
    assert_eq!(sim.focused_title(), "Live TV");
    assert_eq!(sim.home.wanted_hero().cloned(), wanted);
    assert_eq!(
        sim.press(OK),
        Some(Effect::Launch(AppId::new("com.webos.app.livetv")))
    );
    sim.settle();
    assert_eq!(sim.press(BACK), None);
    assert!(!sim.home.on_others());
    assert_eq!(sim.focused_title(), "Other apps");
    sim.settle();
    let view = sim.home.view();
    assert_eq!(view.page, 0.0);
    assert!(view.tiles.iter().all(|t| !t.named));
}

#[test]
fn the_other_apps_page_scrolls_from_its_third_row() {
    let apps: Vec<App> = mock_apps().into_iter().cycle().take(40).collect();
    let mut sim = Sim::new(apps, BackAtTop::Exit);
    sim.presses(&[RIGHT, RIGHT, RIGHT, RIGHT, OK]);
    sim.settle();
    assert_eq!(
        sim.home.view().tiles.len(),
        20,
        "three rows and the top of a fourth"
    );
    sim.presses(&[DOWN, DOWN, DOWN]);
    sim.settle();
    let view = sim.home.view();
    let focused = view.tiles.last().unwrap();
    assert!(focused.focused);
    let row_y = layout::GRID_Y + layout::GRID_PITCH;
    let middle = focused.rect.y + focused.rect.h / 2.0;
    assert!((middle - (row_y + layout::TILE_H / 2.0)).abs() < 1e-3);
    for tile in &view.tiles {
        assert!(tile.rect.bottom() + 60.0 > 0.0 && tile.rect.y < layout::SCREEN_H);
    }
}

#[test]
fn focus_first_comes_home_from_a_chip_and_from_other_apps() {
    let mut sim = Sim::mock();
    assert!(!sim.home.focus_first(sim.now), "already on the first tile");
    assert!(sim.home.is_settled(), "and nothing changed");
    sim.presses(&[RIGHT, DOWN]);
    sim.settle();
    assert!(sim.home.focus_first(sim.now));
    assert_eq!(sim.focused_title(), "Netflix");
    sim.settle();
    sim.presses(&[RIGHT, RIGHT, RIGHT, RIGHT, OK, RIGHT]);
    sim.settle();
    assert!(sim.home.focus_first(sim.now));
    assert!(!sim.home.on_others());
    assert_eq!(sim.focused_title(), "Netflix");
    sim.settle();
    assert_eq!(sim.home.view().page, 0.0);
}

#[test]
fn the_status_line_reports_a_launch_for_two_seconds() {
    let mut sim = Sim::mock();
    let id = AppId::new("netflix");
    let at = Duration::from_secs(30);
    sim.home.launched(&id, &Ok(()), at);
    assert_eq!(sim.home.view().status, Some("Opening Netflix"));
    assert_eq!(sim.home.next_deadline(), Some(at + STATUS_FOR));
    sim.home.frame_drawn();
    sim.home.advance_to(at + STATUS_FOR - FRAME);
    assert!(sim.home.is_settled(), "a status line on its own is still");
    sim.home.advance_to(at + STATUS_FOR);
    assert_eq!(sim.home.view().status, None);
    assert!(!sim.home.is_settled(), "its removal needs a frame");
    let err = Err(LaunchError("boom".to_owned()));
    sim.home.launched(&id, &err, at + STATUS_FOR);
    assert_eq!(sim.home.view().status, Some("Could not open Netflix"));
}

#[test]
fn no_apps_shows_the_empty_state_and_back_still_leaves() {
    let mut sim = Sim::new(Vec::new(), BackAtTop::Exit);
    for action in [RIGHT, DOWN, OK, BACK] {
        assert_eq!(sim.press(action), None);
    }
    assert!(sim.home.is_settled());
    let view = sim.home.view();
    assert!(view.empty && view.tiles.is_empty() && view.chips.is_empty());
    assert_eq!(view.title, None);
}

#[test]
fn inputs_alone_start_on_the_first_chip() {
    let apps = vec![
        App::new("com.webos.app.hdmi1", "HDMI 1", AppKind::Input),
        App::new("com.webos.app.hdmi2", "HDMI 2", AppKind::Input),
    ];
    let mut sim = Sim::new(apps, BackAtTop::Exit);
    assert_eq!(sim.focused_title(), "HDMI 1");
    assert!(sim.home.view().tiles.is_empty());
    sim.presses(&[RIGHT, UP]);
    assert_eq!(sim.home.view().title, Some(CONTROL_TITLE), "up to the gear");
    sim.presses(&[DOWN]);
    assert_eq!(sim.focused_title(), "HDMI 2", "the chip nearest the gear");
    assert_eq!(sim.press(BACK), Some(Effect::Exit));
}

#[test]
fn up_from_the_row_reaches_the_gear_and_down_or_back_leaves_it() {
    let mut sim = Sim::full();
    sim.presses(&[RIGHT]);
    let art = sim.home.wanted_hero().cloned();
    sim.presses(&[UP]);
    assert_eq!(sim.home.focused(), None);
    let view = sim.home.view();
    assert_eq!(view.title, Some(CONTROL_TITLE));
    assert_eq!(view.subtitle.as_deref(), Some(CONTROL_SUBTITLE));
    assert!(view.gear.expect("gear").focused);
    assert_eq!(
        sim.home.wanted_hero().cloned(),
        art,
        "the art stays while on the gear"
    );
    sim.settle();
    let gear = sim.home.view().gear.expect("gear");
    assert_eq!(gear.lift, 1.0);
    assert!(gear.rect.w > layout::GEAR.w, "the focused gear grows");
    assert!(sim.home.view().tiles.iter().all(|t| t.lift == 0.0));
    sim.presses(&[UP, LEFT, RIGHT]);
    assert_eq!(sim.home.focused(), None, "nothing past the gear");

    let below = layout::nearest(
        sim.home.lines.row.len(),
        layout::GEAR.centre_x(),
        layout::row_tile,
    )
    .expect("a row");
    sim.presses(&[DOWN]);
    assert_eq!(sim.home.focused(), Some(sim.home.lines.row[below]));
    sim.presses(&[UP, BACK]);
    assert_eq!(
        sim.home.focused(),
        Some(sim.home.lines.row[below]),
        "Back too"
    );
    sim.settle();
    assert_eq!(sim.home.view().gear.expect("gear").lift, 0.0);
}

#[test]
fn ok_on_the_gear_opens_the_control_center_and_back_closes_it() {
    let mut sim = Sim::full();
    sim.presses(&[UP]);
    assert_eq!(sim.press(OK), Some(Effect::ReadPrivacy));
    assert!(sim.home.control_open());
    let view = sim.home.view().control.expect("opening");
    assert_eq!(view.privacy[0].value, "Reading…");
    sim.settle();
    let view = sim.home.view().control.expect("open");
    assert_eq!((view.shown, view.offset), (1.0, 0.0));
    assert!(view.motion[0].focused);

    // Left, right and down do nothing: Reduce Motion is the one switch.
    sim.presses(&[LEFT, RIGHT, DOWN]);
    let view = sim.home.view().control.expect("open");
    assert_eq!(view.motion.len(), 1);
    assert!(view.motion[0].focused);

    sim.home.set_privacy(Reading::Failed("no exec".into()));
    assert_eq!(
        sim.home.view().control.expect("open").privacy[0].value,
        "Can't read: no exec"
    );

    assert_eq!(sim.press(BACK), None);
    assert!(!sim.home.control_open());
    sim.settle();
    assert!(sim.home.view().control.is_none());
    assert_eq!(
        sim.home.view().title,
        Some(CONTROL_TITLE),
        "back on the gear"
    );
}

#[test]
fn reduce_motion_turns_every_effect_off_and_back_and_says_to_save_it() {
    let mut sim = Sim::full();
    sim.presses(&[UP]);
    sim.press(OK);
    sim.settle();
    let Some(Effect::SaveMotion(reduced)) = sim.press(OK) else {
        panic!("OK saves");
    };
    assert_eq!(reduced, Reduced::ALL);
    let look = sim.home.look;
    assert!(!look.tilt && !look.light && !look.parallax && !look.slide && !look.fade);
    assert!(look.hero && look.dim && look.shadow, "only motion");
    assert!(sim.home.view().control.expect("open").motion[0].on);
    assert_eq!(sim.home.reduced(), reduced);
    // Up and Down stay on the one switch; OK again turns it all back.
    sim.presses(&[DOWN, UP]);
    let Some(Effect::SaveMotion(reduced)) = sim.press(OK) else {
        panic!("OK saves");
    };
    assert_eq!(reduced, Reduced::default());
    assert_eq!(sim.home.look, Look::FULL);
    assert!(!sim.home.view().control.expect("open").motion[0].on);
}

#[test]
fn with_nothing_moving_in_the_launch_look_the_switch_does_nothing() {
    let mut sim = Sim::with(Home::new(mock_apps(), BackAtTop::Exit).with_look(Look::PLAIN));
    sim.presses(&[UP]);
    sim.press(OK);
    assert_eq!(sim.press(OK), None, "nothing to take away");
    assert_eq!(sim.home.look, Look::PLAIN);
    let row = &sim.home.view().control.expect("open").motion[0];
    assert!(!row.on && !row.editable);
    // An effect the launch look keeps off stays off with the switch off.
    let launch = Look {
        light: false,
        ..Look::FULL
    };
    let mut sim = Sim::with(Home::new(mock_apps(), BackAtTop::Exit).with_look(launch));
    sim.presses(&[UP]);
    sim.press(OK);
    sim.press(OK);
    sim.press(OK);
    assert_eq!(sim.home.look, launch);
}

#[test]
fn saved_reduce_motion_applies_at_start_and_home_closes_the_panel() {
    let reduced = Reduced::from_words("parallax slide");
    let home = Home::new(mock_apps(), BackAtTop::Exit)
        .with_look(Look::FULL)
        .with_reduced(reduced);
    assert!(!home.look.parallax && !home.look.slide && home.look.tilt);
    assert_eq!(home.view().page_slide, 0.0);
    let mut sim = Sim::with(home);
    sim.presses(&[UP]);
    sim.press(OK);
    assert_eq!(
        sim.home.view().control.expect("open").offset,
        0.0,
        "no slide"
    );
    assert!(sim.home.focus_first(sim.now));
    assert!(!sim.home.control_open());
    assert_eq!(sim.focused_title(), "Netflix");
    sim.settle();
    assert!(sim.home.view().control.is_none());
}

#[test]
fn time_running_backwards_steps_nothing() {
    let mut sim = Sim::mock();
    sim.press(RIGHT);
    sim.home.advance_to(Duration::from_millis(100));
    let lift = sim.tile("Prime Video").lift;
    sim.home.advance_to(Duration::from_millis(50));
    assert_eq!(sim.tile("Prime Video").lift, lift);
}

#[test]
fn settle_now_ends_every_animation_and_owes_a_frame() {
    let mut sim = Sim::full();
    sim.presses(&[RIGHT, RIGHT, RIGHT, RIGHT, OK]);
    sim.frame();
    assert!(sim.home.is_animating());
    sim.home.settle_now();
    assert!(!sim.home.is_animating());
    assert!(!sim.home.is_settled());
    assert_eq!(sim.home.view().page, 1.0);
}

// The art.

const PLEX: Hero = Hero::Named("hero-plex");

#[test]
fn the_plain_home_wants_no_hero_and_shows_the_gradient() {
    let mut sim = Sim::mock();
    assert_eq!(sim.home.look(), &Look::PLAIN);
    assert_eq!(sim.home.wanted_hero(), None);
    sim.presses(&[RIGHT, RIGHT, RIGHT]);
    assert_eq!(sim.home.wanted_hero(), None);
    assert_eq!(sim.home.view().art, ShelfView::PLACEHOLDER);
}

#[test]
fn the_wanted_hero_follows_focus_onto_the_chips() {
    let mut sim = Sim::full();
    // Netflix has no colour in the mock: the ambient hero.
    assert_eq!(sim.home.wanted_hero(), Some(&Hero::Ambient));
    sim.presses(&[RIGHT, RIGHT, RIGHT]);
    assert_eq!(sim.home.wanted_hero(), Some(&PLEX));
    sim.presses(&[DOWN, LEFT, LEFT, LEFT, LEFT]);
    assert_eq!(sim.focused_title(), "HDMI 1");
    assert_eq!(sim.home.wanted_hero(), Some(&Hero::Named("hero-hdmi1")));
    // Nothing is shown until the shell says the textures are in.
    assert_eq!(sim.home.shelf_target(), None);
    assert_eq!(sim.home.view().art, ShelfView::PLACEHOLDER);
}

#[test]
fn the_owners_art_changes_the_wanted_hero() {
    let mut sim = Sim::full();
    let owner: HashSet<AppId> = [AppId::new("netflix")].into_iter().collect();
    sim.home.set_owner_art(owner);
    assert_eq!(
        sim.home.wanted_hero(),
        Some(&Hero::Owner(AppId::new("netflix")))
    );
}

#[test]
fn the_view_carries_the_crossfade_and_the_screen_settles_after_it() {
    let mut sim = Sim::full();
    sim.settle();
    sim.home.show_hero(Some(Hero::Ambient));
    assert!(!sim.home.is_settled(), "the crossfade owes frames");
    assert!(sim.home.is_fading());
    sim.frame();
    sim.frame();
    let art = sim.home.view().art;
    // The layered hero is the base; the gradient fades out over it.
    assert_eq!(art.base, Some(&Hero::Ambient));
    let (over, alpha) = art.over.expect("mid crossfade");
    assert_eq!(over, None);
    assert!(alpha > 0.0 && alpha < 1.0, "{alpha}");
    let frames = sim.settle();
    assert!((25..50).contains(&frames), "{frames} frames");
    assert_eq!(
        sim.home.take_faded(),
        Some(Faded {
            from: None,
            to: Some(Hero::Ambient)
        })
    );
    sim.home.show_hero(Some(Hero::Ambient));
    assert!(sim.home.is_settled(), "the same hero again changes nothing");
}

#[test]
fn settle_now_ends_a_crossfade() {
    let mut sim = Sim::full();
    sim.home.show_hero(Some(PLEX));
    sim.frame();
    sim.home.settle_now();
    assert!(!sim.home.is_fading());
    assert_eq!(sim.home.view().art.base, Some(&PLEX));
}

// Focus light and tilt, and the parallax.

const PEAK: f32 = style::TILT_DEGREES * std::f32::consts::PI / 180.0;

/// Frames until settled, keeping each app's largest lean either way and
/// every light seen.
fn watch(sim: &mut Sim) -> (Vec<[f32; 2]>, Vec<(usize, SweepView)>) {
    let mut most = vec![[0.0f32; 2]; sim.home.apps().len()];
    let mut sweeps = Vec::new();
    let mut frames = 0;
    loop {
        for tile in sim.home.view().tiles {
            for (most, tilt) in most[tile.index].iter_mut().zip(tile.tilt) {
                if tilt.abs() > most.abs() {
                    *most = tilt;
                }
            }
            sweeps.extend(tile.sweep.map(|s| (tile.index, s)));
        }
        if !sim.frame() {
            return (most, sweeps);
        }
        frames += 1;
        assert!(frames < 600, "never settled");
    }
}

#[test]
fn a_newly_focused_tile_leans_toward_the_move_and_the_light_crosses_it() {
    let mut sim = Sim::full();
    sim.settle();
    sim.press(RIGHT);
    let (most, sweeps) = watch(&mut sim);
    // Prime Video (app 1) leans its right edge back, up to the peak;
    // nothing else leans.
    assert!(
        most[1][0] > 0.95 * PEAK && most[1][0] <= PEAK,
        "{:?}",
        most[1]
    );
    assert_eq!(most[1][1], 0.0);
    assert!(
        most.iter()
            .enumerate()
            .all(|(i, m)| i == 1 || *m == [0.0; 2])
    );
    assert!(sweeps.iter().all(|&(i, s)| i == 1 && s.dir == Dir::Right));
    let ats: Vec<f32> = sweeps.iter().map(|(_, s)| s.at).collect();
    assert!(ats.windows(2).all(|w| w[0] <= w[1]), "{ats:?}");
    assert!(ats[0] < 0.0 && ats[ats.len() - 1] > 1.0, "{ats:?}");
    assert!(sim.home.is_settled());
    // Onto a chip: no lean and no light.
    sim.press(DOWN);
    let (most, sweeps) = watch(&mut sim);
    assert!(most.iter().all(|m| *m == [0.0; 2]) && sweeps.is_empty());
}

#[test]
fn a_held_key_never_leans_a_tile_past_the_peak() {
    let mut sim = Sim::full();
    sim.settle();
    let mut most = vec![0.0f32; sim.home.apps().len()];
    let presses = [RIGHT; 4].into_iter().chain([LEFT, RIGHT].repeat(6));
    for (n, action) in presses.enumerate() {
        let _ = sim.home.handle(
            Press {
                action,
                repeat: n > 0 && n < 4,
            },
            sim.now,
        );
        for _ in 0..7 {
            sim.frame();
            for tile in sim.home.view().tiles {
                most[tile.index] = most[tile.index].max(tile.tilt[0].abs());
            }
        }
    }
    assert!(most.iter().all(|&m| m <= PEAK * 1.0001), "{most:?}");
    sim.settle();
    assert!(sim.home.view().tiles.iter().all(|t| t.tilt == [0.0; 2]));
}

#[test]
fn the_light_alone_or_neither_as_the_look_says() {
    let light_only = Look {
        tilt: false,
        ..Look::FULL
    };
    let mut sim = Sim::with(Home::new(mock_apps(), BackAtTop::Exit).with_look(light_only));
    sim.press(RIGHT);
    let (most, sweeps) = watch(&mut sim);
    assert!(most.iter().all(|m| *m == [0.0; 2]));
    assert!(!sweeps.is_empty());
    let mut sim = Sim::mock();
    sim.presses(&[RIGHT, LEFT]);
    let (most, sweeps) = watch(&mut sim);
    assert!(most.iter().all(|m| *m == [0.0; 2]) && sweeps.is_empty());
}

/// Frames until settled, keeping the art's largest drift either way.
fn drifted(sim: &mut Sim) -> [f32; 2] {
    let mut most = [0.0f32; 2];
    loop {
        for (most, drift) in most.iter_mut().zip(sim.home.view().drift) {
            if drift.abs() > most.abs() {
                *most = drift;
            }
        }
        if !sim.frame() {
            return most;
        }
    }
}

#[test]
fn the_ambient_heros_layers_drift_against_a_move_and_not_behind_other_apps() {
    let mut sim = Sim::full();
    sim.home.show_hero(Some(Hero::Ambient));
    sim.settle();
    sim.press(RIGHT);
    let most = drifted(&mut sim);
    assert!(most[0] < -0.95 && most[0] >= -1.0, "{most:?}");
    assert_eq!(most[1], 0.0);
    assert_eq!(sim.home.view().drift, [0.0; 2]);
    for action in [LEFT, RIGHT].repeat(5) {
        sim.press(action);
        for _ in 0..4 {
            sim.frame();
            assert!(sim.home.view().drift[0].abs() <= 1.0001);
        }
    }
    sim.settle();
    // Down drifts too, on the home.
    sim.press(DOWN);
    assert!(drifted(&mut sim)[1] < -0.9);
    // Behind the "Other apps" page nothing drifts.
    sim.presses(&[UP, RIGHT, RIGHT, RIGHT, OK]);
    sim.settle();
    sim.press(RIGHT);
    assert_eq!(drifted(&mut sim), [0.0; 2]);
}

#[test]
fn a_flat_hero_or_a_look_without_parallax_or_tilt_never_drifts() {
    let no_parallax = Look {
        parallax: false,
        ..Look::FULL
    };
    let no_tilt = Look {
        tilt: false,
        ..Look::FULL
    };
    for (look, hero) in [
        (Look::FULL, PLEX),
        (no_parallax, Hero::Ambient),
        (no_tilt, Hero::Ambient),
    ] {
        let mut sim = Sim::with(Home::new(mock_apps(), BackAtTop::Exit).with_look(look));
        sim.home.show_hero(Some(hero.clone()));
        sim.settle();
        sim.press(RIGHT);
        assert_eq!(drifted(&mut sim), [0.0; 2], "{look:?} {hero:?}");
    }
}

/// A fixed-seed random walk over keys, repeat flags, frame lengths and app
/// counts: nothing panics, every rect stays finite, and every screen
/// settles once the keys stop.
#[test]
fn random_use_always_settles() {
    let mut seed = 0x2545_F491_4F6C_DD1Du64;
    let mut next = move |n: u64| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed % n
    };
    let actions = [UP, DOWN, LEFT, RIGHT, OK, BACK];
    for _ in 0..200 {
        let count = next(20) as usize;
        let apps = mock_apps().into_iter().cycle().take(count).collect();
        let mut sim = Sim::new(apps, BackAtTop::Stay);
        if next(2) == 0 {
            sim.home.look = Look::FULL;
        }
        if next(2) == 0 {
            sim.home.set_playing(Some(AppId::new("netflix")));
        }
        for _ in 0..40 {
            let press = Press {
                action: actions[next(6) as usize],
                repeat: next(3) == 0,
            };
            if let Some(Effect::Launch(id)) = sim.home.handle(press, sim.now) {
                sim.home.launched(&id, &Ok(()), sim.now);
            }
            sim.now += Duration::from_millis(1 + next(400));
            sim.home.advance_to(sim.now);
            let view = sim.home.view();
            for tile in &view.tiles {
                let r = tile.rect;
                assert!(
                    [r.x, r.y, r.w, r.h, tile.lift, tile.tilt[0], tile.tilt[1]]
                        .iter()
                        .all(|v| v.is_finite())
                );
                assert!(tile.tilt.iter().all(|t| t.abs() <= PEAK * 1.0001));
            }
            assert!(view.tiles.iter().filter(|t| t.focused).count() <= 1);
            sim.draw_if_needed();
        }
        sim.now += STATUS_FOR;
        sim.settle();
        assert!(sim.home.is_settled());
    }
}

#[test]
fn the_pages_hand_over_and_cross_only_faintly() {
    assert_eq!(page_fades(0.0), (1.0, 0.0));
    assert_eq!(page_fades(1.0), (0.0, 1.0));
    for i in 0..=100 {
        let (home, others) = page_fades(i as f32 / 100.0);
        assert!(home.min(others) <= 1.0 / 3.0 + 1e-6, "{i}: {home} {others}");
    }
    let (home, others) = page_fades(0.5);
    assert!((home - others).abs() < 1e-6, "even at the middle");
}
