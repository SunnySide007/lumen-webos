//! The home screen's state: focus, motion and the status line. It takes key
//! presses, hands back effects for the shell to carry out (launch an app, or
//! leave), says whether anything still moves, and lists what to draw. Nothing
//! here draws or reads a clock: every call that can change the screen takes
//! the current time.

use std::collections::HashSet;
use std::time::Duration;

use crate::anim::{Spring, Tuning, kick_for_peak};
use crate::apps::{App, AppId, LaunchError};
use crate::art::{Hero, Look, hero_for};
use crate::focus::{Dir, GridFocus, Move};
use crate::input::{Action, Press};
use crate::layout::{self, Rect};
use crate::shelf::{Faded, Shelf, ShelfView};
use crate::style;

// The springs' values live in `style`.
pub const LIFT: Tuning = style::LIFT;
pub const SCROLL: Tuning = style::SCROLL;
pub const BUMP: Tuning = style::BUMP;
pub const BUMP_PEAK: f32 = style::BUMP_PEAK;
pub const PRESS_DIP: f32 = style::PRESS_DIP;
pub const STATUS_FOR: Duration = Duration::from_secs(2);

/// What Back does with focus already on row 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackAtTop {
    /// Leave the shell: it is an ordinary app until the takeover makes it Home.
    Exit,
    /// Do nothing: the shell is Home, so there is nowhere to go back to.
    Stay,
}

/// What the shell must do after a key press.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    Launch(AppId),
    Exit,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TileView<'a> {
    pub app: &'a App,
    /// On screen: lifted, bumped and scrolled.
    pub rect: Rect,
    /// 0 resting, 1 fully lifted.
    pub lift: f32,
    pub focused: bool,
    /// Position in the launcher's list.
    pub index: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct View<'a> {
    /// The Top Shelf band, on screen.
    pub shelf: Rect,
    pub shelf_title: Option<&'a str>,
    /// The shelf's art: its hero, or the placeholder, and any crossfade.
    pub shelf_art: ShelfView<'a>,
    /// Tiles on screen, back to front: the focused tile comes last.
    pub tiles: Vec<TileView<'a>>,
    pub status: Option<&'a str>,
    /// There are no apps at all.
    pub empty: bool,
}

#[derive(Debug)]
pub struct Home {
    apps: Vec<App>,
    focus: GridFocus,
    lifts: Vec<Spring>,
    scroll: Spring,
    bump_x: Spring,
    bump_y: Spring,
    /// The tile the bump springs move: the one that was blocked, which keeps
    /// its nudge (and lets it settle) after focus moves on.
    bump_owner: usize,
    /// The row-0 tile the Top Shelf shows, kept while focus is lower down.
    shelf_index: usize,
    /// Which Phase 4 features are on.
    look: Look,
    /// The apps the owner made Top Shelf art for.
    owner_art: HashSet<AppId>,
    /// The hero for the tile the shelf shows, if the look has heroes.
    wanted: Option<Hero>,
    /// The shelf's art and its crossfade.
    shelf: Shelf,
    back_at_top: BackAtTop,
    status: Option<(String, Duration)>,
    now: Duration,
    /// Something changed that the last frame does not show.
    dirty: bool,
}

impl Home {
    /// Focus on the first tile, already lifted; the first frame is owed.
    pub fn new(apps: Vec<App>, back_at_top: BackAtTop) -> Self {
        let lifts = (0..apps.len())
            .map(|i| Spring::new(if i == 0 { 1.0 } else { 0.0 }, LIFT))
            .collect();
        Self {
            focus: GridFocus::new(apps.len(), layout::COLUMNS),
            apps,
            lifts,
            scroll: Spring::new(0.0, SCROLL),
            bump_x: Spring::new(0.0, BUMP),
            bump_y: Spring::new(0.0, BUMP),
            bump_owner: 0,
            shelf_index: 0,
            look: Look::PLAIN,
            owner_art: HashSet::new(),
            wanted: None,
            shelf: Shelf::default(),
            back_at_top,
            status: None,
            now: Duration::ZERO,
            dirty: true,
        }
    }

    /// The same home screen with the Phase 4 features `look` turns on;
    /// `new` has none of them, as the shell MVP drew it.
    pub fn with_look(mut self, look: Look) -> Self {
        self.look = look;
        self.update_wanted();
        self
    }

    pub fn look(&self) -> &Look {
        &self.look
    }

    pub fn apps(&self) -> &[App] {
        &self.apps
    }

    /// The apps the owner made Top Shelf art for, once the shell has listed
    /// them.
    pub fn set_owner_art(&mut self, apps: HashSet<AppId>) {
        self.owner_art = apps;
        self.update_wanted();
    }

    fn update_wanted(&mut self) {
        self.wanted = match self.apps.get(self.shelf_index) {
            Some(app) if self.look.hero => Some(hero_for(app, &self.owner_art)),
            _ => None,
        };
    }

    /// The hero for the tile the shelf shows: what it should show once its
    /// textures are in. `None` without heroes in the look, or apps.
    pub fn wanted_hero(&self) -> Option<&Hero> {
        self.wanted.as_ref()
    }

    /// What the shelf shows, or is fading to; `None` is the placeholder.
    pub fn shelf_target(&self) -> Option<&Hero> {
        self.shelf.target()
    }

    /// `hero`'s textures are in (`None`: the shelf is to show its
    /// placeholder): the shelf fades to it.
    pub fn show_hero(&mut self, hero: Option<Hero>) {
        if self.shelf.show(hero) {
            self.dirty = true;
        }
    }

    /// The oldest crossfade that finished since the last call, so its old
    /// hero's textures can go.
    pub fn take_faded(&mut self) -> Option<Faded> {
        self.shelf.take_faded()
    }

    /// Whether the shelf's art is fading from one to another.
    pub fn is_fading(&self) -> bool {
        self.shelf.is_animating()
    }

    pub fn focused(&self) -> Option<usize> {
        self.focus.index()
    }

    /// Applies a key press at `now`, after stepping any motion in flight up to
    /// `now` so it is retargeted from where it really is.
    #[must_use]
    pub fn handle(&mut self, press: Press, now: Duration) -> Option<Effect> {
        self.advance_to(now);
        match press.action {
            Action::Move(dir) => {
                self.move_focus(dir, press.repeat);
                None
            }
            Action::Select => self.select(),
            Action::Back => self.back(),
        }
    }

    fn move_focus(&mut self, dir: Dir, repeat: bool) {
        let Some(from) = self.focus.index() else {
            return;
        };
        match self.focus.move_focus(dir) {
            Move::Moved => self.focus_changed(from),
            // A repeat that reaches an edge stops there; only a fresh press bumps.
            Move::Blocked if !repeat => self.bump(dir),
            Move::Blocked => {}
        }
    }

    fn focus_changed(&mut self, from: usize) {
        let Some(to) = self.focus.index() else {
            return;
        };
        self.lifts[from].set_target(0.0);
        self.lifts[to].set_target(1.0);
        let row = self.focus.row();
        self.scroll.set_target(layout::scroll_for_row(row));
        if row == 0 && self.shelf_index != to {
            self.shelf_index = to;
            self.update_wanted();
        }
        self.dirty = true;
    }

    /// Nudges the focused tile toward `dir`. A nudge already under way only
    /// gets the kick it has room for, so mashing a key never pushes a tile
    /// past `BUMP_PEAK`.
    fn bump(&mut self, dir: Dir) {
        let Some(focused) = self.focus.index() else {
            return;
        };
        if self.bump_owner != focused {
            self.bump_x.snap();
            self.bump_y.snap();
            self.bump_owner = focused;
        }
        let (spring, sign) = match dir {
            Dir::Left => (&mut self.bump_x, -1.0),
            Dir::Right => (&mut self.bump_x, 1.0),
            Dir::Up => (&mut self.bump_y, -1.0),
            Dir::Down => (&mut self.bump_y, 1.0),
        };
        let already = (spring.value() * sign).max(0.0);
        let room = (1.0 - already / BUMP_PEAK).max(0.0);
        spring.set_velocity(sign * room * kick_for_peak(BUMP_PEAK, BUMP.omega));
        self.dirty = true;
    }

    fn select(&mut self) -> Option<Effect> {
        let i = self.focus.index()?;
        self.lifts[i].set_velocity(-kick_for_peak(PRESS_DIP, LIFT.omega));
        self.dirty = true;
        Some(Effect::Launch(self.apps[i].id.clone()))
    }

    fn back(&mut self) -> Option<Effect> {
        if let Some(from) = self.focus.index()
            && self.focus.row() > 0
        {
            self.focus.set(self.focus.column());
            self.focus_changed(from);
            return None;
        }
        match self.back_at_top {
            BackAtTop::Exit => Some(Effect::Exit),
            BackAtTop::Stay => None,
        }
    }

    /// Reports how a launch went, in the status line.
    pub fn launched(&mut self, id: &AppId, result: &Result<(), LaunchError>, now: Duration) {
        let title = self
            .apps
            .iter()
            .find(|app| &app.id == id)
            .map_or(id.as_str(), |app| app.title.as_str());
        let text = match result {
            Ok(()) => format!("Opening {title}"),
            Err(_) => format!("Could not open {title}"),
        };
        self.show_status(text, now);
    }

    /// Shows `text` in the status line from `now` for `STATUS_FOR`.
    pub fn show_status(&mut self, text: String, now: Duration) {
        self.advance_to(now);
        self.status = Some((text, self.now + STATUS_FOR));
        self.dirty = true;
    }

    /// Moves the clock to `now` and steps every animation. Time never runs
    /// backwards here: an earlier `now` steps nothing. A step that moves
    /// anything owes a frame, including the one that snaps it to rest.
    pub fn advance_to(&mut self, now: Duration) {
        let dt = now.saturating_sub(self.now).as_secs_f32();
        self.now = self.now.max(now);
        if self.is_animating() {
            for spring in self.springs_mut() {
                spring.step(dt);
            }
            self.shelf.step(dt);
            self.dirty = true;
        }
        if let Some((_, until)) = &self.status
            && *until <= self.now
        {
            self.status = None;
            self.dirty = true;
        }
    }

    fn springs_mut(&mut self) -> impl Iterator<Item = &mut Spring> {
        self.lifts
            .iter_mut()
            .chain([&mut self.scroll, &mut self.bump_x, &mut self.bump_y])
    }

    pub fn is_animating(&self) -> bool {
        self.shelf.is_animating()
            || !self
                .lifts
                .iter()
                .chain([&self.scroll, &self.bump_x, &self.bump_y])
                .all(Spring::is_settled)
    }

    /// Nothing moves and the last frame drawn is still right: stop drawing.
    pub fn is_settled(&self) -> bool {
        !self.dirty && !self.is_animating()
    }

    /// The shell drew a frame of the current state.
    pub fn frame_drawn(&mut self) {
        self.dirty = false;
    }

    /// The last frame was lost (the window came back): draw again.
    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    /// When the screen next changes by itself: the status line's expiry.
    pub fn next_deadline(&self) -> Option<Duration> {
        self.status.as_ref().map(|(_, until)| *until)
    }

    /// Ends every animation on its target, for an app going to the background.
    pub fn settle_now(&mut self) {
        if self.is_animating() {
            self.dirty = true;
        }
        for spring in self.springs_mut() {
            spring.snap();
        }
        self.shelf.settle_now();
    }

    pub fn view(&self) -> View<'_> {
        let scroll = self.scroll.value();
        let focused = self.focus.index();
        // At most four rows show at once; a frame mid-scroll can touch five.
        let mut tiles = Vec::with_capacity(5 * layout::COLUMNS);
        let mut front = None;
        for (i, app) in self.apps.iter().enumerate() {
            let lift = self.lifts[i].value();
            let mut rect = layout::lifted(layout::tile_rect(i), lift).offset(0.0, -scroll);
            if i == self.bump_owner {
                rect = rect.offset(self.bump_x.value(), self.bump_y.value());
            }
            if !layout::tile_on_screen(rect, lift) {
                continue;
            }
            let tile = TileView {
                app,
                rect,
                lift,
                focused: focused == Some(i),
                index: i,
            };
            if tile.focused {
                front = Some(tile);
            } else {
                tiles.push(tile);
            }
        }
        tiles.extend(front);
        View {
            shelf: layout::TOP_SHELF.offset(0.0, -scroll),
            shelf_title: self.apps.get(self.shelf_index).map(|a| a.title.as_str()),
            shelf_art: self.shelf.view(&self.look),
            tiles,
            status: self.status.as_ref().map(|(text, _)| text.as_str()),
            empty: self.apps.is_empty(),
        }
    }
}

#[cfg(test)]
mod tests {
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
        /// Where the Top Shelf was in the last frame drawn.
        drawn_shelf_y: Option<f32>,
    }

    impl Sim {
        fn new(apps: Vec<App>, back_at_top: BackAtTop) -> Self {
            let mut sim = Self {
                home: Home::new(apps, back_at_top),
                now: Duration::ZERO,
                drawn_shelf_y: None,
            };
            sim.draw_if_needed();
            sim
        }

        fn mock() -> Self {
            Self::new(mock_apps(), BackAtTop::Exit)
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

        fn draw_if_needed(&mut self) -> bool {
            if self.home.is_settled() {
                return false;
            }
            self.drawn_shelf_y = Some(self.home.view().shelf.y);
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

        fn tile(&self, title: &str) -> TileView<'_> {
            *self
                .home
                .view()
                .tiles
                .iter()
                .find(|t| t.app.title == title)
                .expect("tile on screen")
        }

        fn focused_view(&self) -> TileView<'_> {
            let view = self.home.view();
            let last = *view.tiles.last().expect("a tile is on screen");
            assert!(last.focused, "the focused tile is drawn last");
            last
        }
    }

    const RIGHT: Action = Action::Move(Dir::Right);
    const LEFT: Action = Action::Move(Dir::Left);
    const DOWN: Action = Action::Move(Dir::Down);

    #[test]
    fn starts_on_the_first_tile_lifted_and_owes_one_frame() {
        let mut home = Home::new(mock_apps(), BackAtTop::Exit);
        assert!(!home.is_settled(), "the first frame is owed");
        assert!(!home.is_animating());
        home.frame_drawn();
        assert!(home.is_settled());
        let view = home.view();
        let tile = view.tiles.last().unwrap();
        assert!(tile.focused && tile.lift == 1.0);
        assert_eq!(tile.rect, layout::lifted(layout::tile_rect(0), 1.0));
        assert_eq!(view.shelf_title, Some("Netflix"));
        assert_eq!(view.shelf, layout::TOP_SHELF);
        assert_eq!(view.tiles.len(), 10, "rows 0 and 1 are on screen");
    }

    #[test]
    fn moving_right_hands_the_lift_to_the_next_tile() {
        let mut sim = Sim::mock();
        assert_eq!(sim.press(RIGHT), None);
        assert!(!sim.home.is_settled());
        let frames = sim.settle();
        assert!((15..30).contains(&frames), "{frames} frames");
        assert_eq!(sim.home.focused(), Some(1));
        let view = sim.home.view();
        assert_eq!(view.shelf_title, Some("Prime Video"));
        assert_eq!(sim.tile("Netflix").lift, 0.0);
        assert_eq!(sim.focused_view().lift, 1.0);
    }

    #[test]
    fn the_first_press_after_a_long_idle_still_animates() {
        let mut sim = Sim::mock();
        sim.now = Duration::from_secs(30);
        sim.press(RIGHT);
        assert!(sim.home.is_animating());
        sim.frame();
        let lift = sim.focused_view().lift;
        assert!(lift > 0.0 && lift < 0.5, "one frame in: {lift}");
    }

    #[test]
    fn the_last_frame_drawn_is_the_settled_one() {
        let mut sim = Sim::mock();
        sim.press(DOWN);
        sim.settle();
        assert_eq!(sim.drawn_shelf_y, Some(-layout::TOP_SHELF.h));
    }

    #[test]
    fn row_one_scrolls_the_top_shelf_off_and_keeps_its_title() {
        let mut sim = Sim::mock();
        sim.press(RIGHT);
        sim.press(RIGHT);
        sim.press(DOWN);
        sim.settle();
        assert_eq!(sim.home.focused(), Some(7));
        let view = sim.home.view();
        assert_eq!(view.shelf.bottom(), 0.0);
        assert_eq!(view.shelf_title, Some("YouTube"));
        let tile = sim.focused_view();
        let centre_y = tile.rect.y + tile.rect.h / 2.0;
        assert!((centre_y - (layout::FOCUS_ROW_Y + layout::TILE_H / 2.0)).abs() < 1e-3);
    }

    #[test]
    fn back_from_a_lower_row_returns_to_row_zero_in_the_same_column() {
        let mut sim = Sim::mock();
        for _ in 0..3 {
            sim.press(RIGHT);
        }
        sim.press(DOWN);
        sim.press(DOWN);
        assert_eq!(sim.home.focused(), Some(11), "row 2 is short");
        assert_eq!(sim.press(Action::Back), None);
        assert_eq!(sim.home.focused(), Some(1));
        sim.settle();
        assert_eq!(sim.home.view().shelf, layout::TOP_SHELF);
    }

    #[test]
    fn back_on_row_zero_exits_or_stays_by_policy() {
        let mut sim = Sim::mock();
        sim.press(RIGHT);
        assert_eq!(sim.press(Action::Back), Some(Effect::Exit));
        let mut stay = Sim::new(mock_apps(), BackAtTop::Stay);
        assert_eq!(stay.press(Action::Back), None);
        assert!(stay.home.is_settled(), "nothing to redraw");
    }

    #[test]
    fn a_fresh_press_at_an_edge_bumps_and_springs_back() {
        let mut sim = Sim::mock();
        let rest = sim.focused_view().rect;
        assert_eq!(sim.press(LEFT), None);
        assert_eq!(sim.home.focused(), Some(0));
        for _ in 0..3 {
            sim.frame();
        }
        let nudged = sim.focused_view().rect;
        assert!(
            (rest.x - nudged.x - BUMP_PEAK).abs() < 0.5,
            "nudged {}",
            rest.x - nudged.x
        );
        assert_eq!(nudged.y, rest.y);
        sim.settle();
        assert_eq!(sim.focused_view().rect, rest);
    }

    #[test]
    fn mashing_an_edge_never_nudges_past_the_peak() {
        let mut sim = Sim::mock();
        let rest = sim.focused_view().rect.x;
        let mut furthest = 0.0f32;
        for i in 0..120 {
            if i % 2 == 0 {
                sim.press(LEFT);
            }
            sim.frame();
            furthest = furthest.max(rest - sim.focused_view().rect.x);
        }
        assert!(furthest <= BUMP_PEAK + 0.5, "nudged {furthest} px");
        assert!(furthest > BUMP_PEAK - 1.0);
    }

    #[test]
    fn a_bumped_tile_settles_smoothly_after_focus_moves_on() {
        let mut sim = Sim::mock();
        for _ in 0..4 {
            sim.press(RIGHT);
        }
        sim.settle();
        sim.press(RIGHT);
        sim.frame();
        sim.frame();
        sim.press(LEFT);
        let mut last = sim.tile("Disney+").rect.x;
        while sim.frame() {
            let x = sim.tile("Disney+").rect.x;
            assert!((x - last).abs() < 8.0, "jumped from {last} to {x}");
            last = x;
        }
        assert_eq!(sim.tile("Disney+").rect, layout::tile_rect(4));
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
    fn select_launches_the_focused_app_with_a_press_pulse() {
        let mut sim = Sim::mock();
        sim.press(RIGHT);
        sim.settle();
        let effect = sim.press(Action::Select);
        assert_eq!(effect, Some(Effect::Launch(AppId::new("amazon"))));
        let mut lowest = 1.0f32;
        while sim.frame() {
            lowest = lowest.min(sim.focused_view().lift);
        }
        assert!((lowest - PRESS_DIP).abs() < 0.05, "dipped to {lowest}");
        assert_eq!(sim.focused_view().lift, 1.0);
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
        assert_eq!(sim.home.next_deadline(), None);

        let err = Err(LaunchError("boom".to_owned()));
        sim.home.launched(&id, &err, at + STATUS_FOR);
        assert_eq!(sim.home.view().status, Some("Could not open Netflix"));
    }

    #[test]
    fn no_apps_shows_the_empty_state_and_back_still_leaves() {
        let mut sim = Sim::new(Vec::new(), BackAtTop::Exit);
        for action in [RIGHT, DOWN, Action::Select] {
            assert_eq!(sim.press(action), None);
        }
        assert!(sim.home.is_settled());
        let view = sim.home.view();
        assert!(view.empty && view.tiles.is_empty());
        assert_eq!(view.shelf_title, None);
        assert_eq!(sim.press(Action::Back), Some(Effect::Exit));
    }

    #[test]
    fn a_single_short_row_bumps_down_and_stops_at_its_end() {
        let apps: Vec<App> = ["a", "b", "c"]
            .iter()
            .map(|id| App::new(id, id, AppKind::App))
            .collect();
        let mut sim = Sim::new(apps, BackAtTop::Exit);
        let rest = sim.focused_view().rect;
        sim.press(DOWN);
        sim.frame();
        assert!(sim.focused_view().rect.y > rest.y, "bumped down");
        sim.press(RIGHT);
        sim.press(RIGHT);
        sim.press(RIGHT);
        assert_eq!(sim.home.focused(), Some(2));
    }

    #[test]
    fn rows_entirely_off_screen_are_not_in_the_view() {
        let apps: Vec<App> = mock_apps().into_iter().cycle().take(40).collect();
        let mut sim = Sim::new(apps, BackAtTop::Exit);
        for _ in 0..3 {
            sim.press(DOWN);
        }
        sim.settle();
        let view = sim.home.view();
        assert_eq!(view.tiles.len(), 20, "rows 2 to 5");
        for tile in &view.tiles {
            assert!(tile.rect.bottom() > 0.0 && tile.rect.y < layout::SCREEN_H);
        }
    }

    #[test]
    fn settle_now_ends_every_animation_and_owes_a_frame() {
        let mut sim = Sim::mock();
        sim.press(DOWN);
        sim.press(LEFT);
        sim.frame();
        assert!(sim.home.is_animating());
        sim.home.settle_now();
        assert!(!sim.home.is_animating());
        assert!(!sim.home.is_settled());
        assert_eq!(sim.home.view().shelf.bottom(), 0.0);
    }

    #[test]
    fn time_running_backwards_steps_nothing() {
        let mut sim = Sim::mock();
        sim.press(RIGHT);
        sim.home.advance_to(Duration::from_millis(100));
        let lift = sim.focused_view().lift;
        sim.home.advance_to(Duration::from_millis(50));
        assert_eq!(sim.focused_view().lift, lift);
    }

    const PLEX: Hero = Hero::Named("hero-plex");

    /// A `Sim` with every Phase 4 feature on.
    fn full(apps: Vec<App>) -> Sim {
        let mut sim = Sim {
            home: Home::new(apps, BackAtTop::Exit).with_look(Look::FULL),
            now: Duration::ZERO,
            drawn_shelf_y: None,
        };
        sim.draw_if_needed();
        sim
    }

    #[test]
    fn the_plain_home_wants_no_hero_and_shows_the_placeholder() {
        let mut sim = Sim::mock();
        assert_eq!(sim.home.look(), &Look::PLAIN);
        assert_eq!(sim.home.wanted_hero(), None);
        for _ in 0..3 {
            sim.press(RIGHT);
        }
        assert_eq!(sim.home.wanted_hero(), None);
        assert_eq!(sim.home.view().shelf_art, ShelfView::PLACEHOLDER);
    }

    #[test]
    fn the_wanted_hero_follows_the_tile_the_shelf_shows() {
        let mut sim = full(mock_apps());
        // Netflix has no colour in the mock: the ambient hero.
        assert_eq!(sim.home.wanted_hero(), Some(&Hero::Ambient));
        for _ in 0..3 {
            sim.press(RIGHT);
        }
        assert_eq!(sim.home.wanted_hero(), Some(&PLEX));
        // Lower rows keep the shelf's tile, and so its hero.
        sim.press(DOWN);
        assert_eq!(sim.home.focused(), Some(8));
        assert_eq!(sim.home.wanted_hero(), Some(&PLEX));
        // Nothing is shown until the shell says the textures are in.
        assert_eq!(sim.home.shelf_target(), None);
        assert_eq!(sim.home.view().shelf_art, ShelfView::PLACEHOLDER);
    }

    #[test]
    fn the_owners_art_changes_the_wanted_hero() {
        let mut sim = full(mock_apps());
        let owner: HashSet<AppId> = [AppId::new("netflix")].into_iter().collect();
        sim.home.set_owner_art(owner);
        assert_eq!(
            sim.home.wanted_hero(),
            Some(&Hero::Owner(AppId::new("netflix")))
        );
    }

    #[test]
    fn the_view_carries_the_crossfade_and_the_screen_settles_after_it() {
        let mut sim = full(mock_apps());
        sim.settle();
        sim.home.show_hero(Some(Hero::Ambient));
        assert!(!sim.home.is_settled(), "the crossfade owes frames");
        assert!(sim.home.is_fading());
        sim.frame();
        sim.frame();
        let art = sim.home.view().shelf_art;
        // The layered hero is the base; the placeholder fades out over it.
        assert_eq!(art.base, Some(&Hero::Ambient));
        let (over, alpha) = art.over.expect("mid crossfade");
        assert_eq!(over, None);
        assert!(alpha > 0.0 && alpha < 1.0, "{alpha}");
        let frames = sim.settle();
        assert!((25..50).contains(&frames), "{frames} frames");
        assert!(sim.home.is_settled());
        let art = sim.home.view().shelf_art;
        assert_eq!(
            (art.base, art.over, art.placeholder),
            (Some(&Hero::Ambient), None, 0.0)
        );
        assert_eq!(
            sim.home.take_faded(),
            Some(Faded {
                from: None,
                to: Some(Hero::Ambient)
            })
        );
        // The same hero again changes nothing.
        sim.home.show_hero(Some(Hero::Ambient));
        assert!(sim.home.is_settled());
    }

    #[test]
    fn settle_now_ends_a_crossfade() {
        let mut sim = full(mock_apps());
        sim.home.show_hero(Some(PLEX));
        sim.frame();
        sim.home.settle_now();
        assert!(!sim.home.is_fading());
        assert_eq!(sim.home.shelf_target(), Some(&PLEX));
        assert_eq!(sim.home.view().shelf_art.base, Some(&PLEX));
    }

    /// A fixed-seed random walk over keys, repeat flags, frame lengths and
    /// app counts: nothing panics, every rect stays finite, and every screen
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
        let actions = [
            Action::Move(Dir::Up),
            Action::Move(Dir::Down),
            LEFT,
            RIGHT,
            Action::Select,
            Action::Back,
        ];
        for _ in 0..200 {
            let count = next(14) as usize;
            let apps = mock_apps().into_iter().cycle().take(count).collect();
            let mut sim = Sim::new(apps, BackAtTop::Stay);
            for _ in 0..30 {
                let press = Press {
                    action: actions[next(6) as usize],
                    repeat: next(3) == 0,
                };
                if matches!(sim.home.handle(press, sim.now), Some(Effect::Launch(_))) {
                    sim.home.launched(&AppId::new("x"), &Ok(()), sim.now);
                }
                sim.now += Duration::from_millis(1 + next(400));
                sim.home.advance_to(sim.now);
                for tile in sim.home.view().tiles {
                    let r = tile.rect;
                    assert!(
                        [r.x, r.y, r.w, r.h, tile.lift]
                            .iter()
                            .all(|v| v.is_finite())
                    );
                }
                sim.draw_if_needed();
            }
            sim.now += STATUS_FOR;
            sim.settle();
            assert!(sim.home.is_settled());
        }
    }
}
