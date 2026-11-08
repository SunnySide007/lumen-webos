//! The home screen's state: focus, motion, the clock and the status line
//! (docs/specs/look-and-feel.md, "The home"). It takes key presses, hands
//! back effects for the shell to carry out (launch an app, or leave), says
//! whether anything still moves, and lists what to draw. Nothing here draws
//! or reads a clock: every call that can change the screen takes the current
//! time, and the time of day comes in from the shell.
//!
//! The home is one page over the art: one row of wide tiles (the contract's
//! apps, then "Other apps") and a slim line of input chips under it. Up and
//! Down move between the two, to the item nearest across. "Other apps" turns
//! to a page of its own, a grid of the rest, and Back turns back. Back on
//! the row returns to what was playing, if anything was. Up from the row
//! reaches the gear at the top right, which opens the Control Center
//! (`control`): the Privacy panel, which only reads, and Reduce Motion.

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use crate::anim::{Spring, Tuning, kick_for_peak};
use crate::apps::{App, AppId, LaunchError};
use crate::arrange::{Arrangement, OTHERS_ID, OTHERS_TITLE, arrange};
use crate::art::{Hero, Look, hero_for};
use crate::control::{Control, ControlView, Reduced};
use crate::focus::{Dir, GridFocus, Move};
use crate::input::{Action, Press};
use crate::layout::{self, Rect};
use crate::privacy::Reading;
use crate::shelf::{Faded, Shelf, ShelfView};
use crate::style;

// The springs' values live in `style`.
pub const LIFT: Tuning = style::LIFT;
pub const SCROLL: Tuning = style::SCROLL;
pub const BUMP: Tuning = style::BUMP;
pub const BUMP_PEAK: f32 = style::BUMP_PEAK;
pub const PRESS_DIP: f32 = style::PRESS_DIP;
pub const STATUS_FOR: Duration = Duration::from_secs(2);

/// What Back does on the row when nothing was playing.
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
    /// The Control Center opened: read the Privacy panel's state
    /// (`Home::set_privacy`).
    ReadPrivacy,
    /// Reduce Motion changed: keep it for the next start.
    SaveMotion(Reduced),
}

/// The gear on screen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GearView {
    pub rect: Rect,
    pub lift: f32,
    pub focused: bool,
    pub alpha: f32,
}

/// A tile on screen: in the row, or on the "Other apps" page.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TileView<'a> {
    pub app: &'a App,
    /// On screen: lifted, bumped, slid and scrolled.
    pub rect: Rect,
    /// 0 resting, 1 fully lifted.
    pub lift: f32,
    pub focused: bool,
    /// Its app's place in `Home::apps`, which picks a colour for an app
    /// that brings none.
    pub index: usize,
    /// Its lean, in radians: about its upright axis (positive leans its
    /// right edge back) and its level one (positive leans its lower edge
    /// back).
    pub tilt: [f32; 2],
    /// The light crossing it, if it is.
    pub sweep: Option<SweepView>,
    /// How much of it shows as its page turns, 0 to 1.
    pub alpha: f32,
    /// On the "Other apps" page, the focused tile's name goes below it.
    pub named: bool,
}

/// An input chip on screen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChipView<'a> {
    pub app: &'a App,
    pub rect: Rect,
    pub lift: f32,
    pub focused: bool,
    /// It is what was playing: a dot beside its name.
    pub playing: bool,
    pub alpha: f32,
}

/// The light crossing a newly focused tile: the middle of its band, as a
/// fraction of the tile along `dir` (below 0 before it enters, above 1 after
/// it leaves), and the way it travels, the way focus moved.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SweepView {
    pub at: f32,
    pub dir: Dir,
}

/// A sweep under way: which app's tile, which way, and when it began.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Sweep {
    tile: usize,
    dir: Dir,
    from: Duration,
}

#[derive(Debug, Clone, PartialEq)]
pub struct View<'a> {
    /// The art behind everything: its hero, or the gradient, and any
    /// crossfade.
    pub art: ShelfView<'a>,
    /// How far a layered hero's layers have drifted against the last move,
    /// across and down, 1 at most: each layer moves its own
    /// `style::PARALLAX_PX` times this.
    pub drift: [f32; 2],
    /// How far the "Other apps" page has turned in: 0 on the home, 1 on it.
    pub page: f32,
    /// How far the pages slide as they turn, in pixels: none without the
    /// look's slide.
    pub page_slide: f32,
    /// The focused item's name, above the row, and the line under it.
    pub title: Option<&'a str>,
    pub subtitle: Option<String>,
    /// The "Other apps" page's heading: where its baseline is, scrolled.
    pub heading_y: f32,
    /// The time of day, `9:41`, once the shell has said it.
    pub clock: Option<String>,
    /// The name of what was playing, under the clock.
    pub playing: Option<&'a str>,
    /// Tiles on screen, back to front: the focused tile comes last.
    pub tiles: Vec<TileView<'a>>,
    pub chips: Vec<ChipView<'a>>,
    pub status: Option<&'a str>,
    /// There are no apps at all.
    pub empty: bool,
    /// The effects that are on, for those drawing alone decides: the
    /// resting tiles' shadows and the focused tile's sheen.
    pub look: Look,
    /// The gear that opens the Control Center, on the home page.
    pub gear: Option<GearView>,
    /// The Control Center, while it shows.
    pub control: Option<ControlView>,
}

/// Where focus is: the row's tile, a chip, or a tile on the "Other apps"
/// page, by place in its line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Spot {
    Row(usize),
    Chip(usize),
    Grid(usize),
    /// The gear: no app's.
    Gear,
}

#[derive(Debug)]
pub struct Home {
    /// The apps listed, then the "Other apps" tile if it has any.
    apps: Vec<App>,
    /// Where each goes; the row ends with the "Other apps" tile.
    lines: Arrangement,
    /// Where focus is; `None` with no apps.
    spot: Option<Spot>,
    /// The "Other apps" page's focus, over `lines.others`.
    grid: GridFocus,
    /// Each app's lift.
    lifts: Vec<Spring>,
    /// Each app's lean, as `TileView::tilt`.
    tilts: Vec<[Spring; 2]>,
    /// The layered hero's drift, as `View::drift`.
    drift: [Spring; 2],
    sweep: Option<Sweep>,
    /// 0 on the home, 1 on the "Other apps" page.
    page: Spring,
    /// The "Other apps" page's scroll.
    scroll: Spring,
    bump_x: Spring,
    bump_y: Spring,
    /// The app the bump springs move: the one that was blocked, which keeps
    /// its nudge (and lets it settle) after focus moves on.
    bump_owner: usize,
    /// Which of the look's effects are on: the launch look with Reduce
    /// Motion's taken away.
    look: Look,
    /// The launch parameters' look, and what Reduce Motion turned off.
    launch_look: Look,
    reduced: Reduced,
    control: Control,
    gear_lift: Spring,
    /// The last app focus was on, whose art stays while focus is on the
    /// gear.
    last_app: Option<usize>,
    /// The apps the owner made art for.
    owner_art: HashSet<AppId>,
    /// The hero for where focus is, if the look has heroes.
    wanted: Option<Hero>,
    /// The art and its crossfade.
    shelf: Shelf,
    back_at_top: BackAtTop,
    /// What was playing when the shell came in front: Back on the row
    /// returns to it.
    playing: Option<AppId>,
    /// When each app was last opened, on this clock.
    opened: HashMap<AppId, Duration>,
    /// The time of day, hours 0 to 23 and minutes.
    clock: Option<(u8, u8)>,
    status: Option<(String, Duration)>,
    now: Duration,
    /// Something changed that the last frame does not show.
    dirty: bool,
}

impl Home {
    /// Focus on the row's first tile (the first chip if the row is empty),
    /// already lifted; the first frame is owed.
    pub fn new(listed: Vec<App>, back_at_top: BackAtTop) -> Self {
        let mut apps = listed;
        let mut lines = arrange(&apps);
        if !lines.others.is_empty() {
            lines.row.push(apps.len());
            apps.push(
                App::new(OTHERS_ID, OTHERS_TITLE, crate::apps::AppKind::App)
                    .with_color(style::OTHERS_TILE),
            );
        }
        let spot = if !lines.row.is_empty() {
            Some(Spot::Row(0))
        } else if !lines.chips.is_empty() {
            Some(Spot::Chip(0))
        } else {
            None
        };
        let upright = Spring::new(0.0, style::TILT);
        let mut home = Self {
            grid: GridFocus::new(lines.others.len(), layout::GRID_COLUMNS),
            lifts: vec![Spring::new(0.0, LIFT); apps.len()],
            tilts: vec![[upright; 2]; apps.len()],
            drift: [Spring::new(0.0, style::PARALLAX); 2],
            sweep: None,
            page: Spring::new(0.0, style::PAGE),
            scroll: Spring::new(0.0, SCROLL),
            bump_x: Spring::new(0.0, BUMP),
            bump_y: Spring::new(0.0, BUMP),
            bump_owner: 0,
            look: Look::PLAIN,
            launch_look: Look::PLAIN,
            reduced: Reduced::default(),
            control: Control::default(),
            gear_lift: Spring::new(0.0, LIFT),
            last_app: None,
            owner_art: HashSet::new(),
            wanted: None,
            shelf: Shelf::default(),
            back_at_top,
            playing: None,
            opened: HashMap::new(),
            clock: None,
            status: None,
            now: Duration::ZERO,
            dirty: true,
            apps,
            lines,
            spot,
        };
        if let Some(app) = home.focused() {
            home.lifts[app] = Spring::new(1.0, LIFT);
            home.last_app = Some(app);
        }
        home
    }

    /// The same home screen with the effects `look` turns on; `new` has
    /// none of them.
    pub fn with_look(mut self, look: Look) -> Self {
        self.launch_look = look;
        self.look = self.reduced.apply(look);
        self.update_wanted();
        self
    }

    /// The same home screen with the effects Reduce Motion turned off, as
    /// saved.
    pub fn with_reduced(mut self, reduced: Reduced) -> Self {
        self.set_reduced(reduced);
        self
    }

    /// Reduce Motion as saved, in place: the effects it turns off stop.
    pub fn set_reduced(&mut self, reduced: Reduced) {
        self.reduced = reduced;
        self.apply_reduced();
        self.update_wanted();
        self.dirty = true;
    }

    /// What Reduce Motion has turned off.
    pub fn reduced(&self) -> Reduced {
        self.reduced
    }

    /// Whether the Control Center is open, or opening.
    pub fn control_open(&self) -> bool {
        self.control.is_open()
    }

    /// The Privacy panel's state, as the shell read it (`Effect::ReadPrivacy`).
    pub fn set_privacy(&mut self, reading: Reading) {
        self.control.set_reading(reading);
        self.dirty = true;
    }

    pub fn look(&self) -> &Look {
        &self.look
    }

    /// The apps listed, then the "Other apps" tile if there is one.
    pub fn apps(&self) -> &[App] {
        &self.apps
    }

    /// The apps the owner made art for, once the shell has listed them.
    pub fn set_owner_art(&mut self, apps: HashSet<AppId>) {
        self.owner_art = apps;
        self.update_wanted();
    }

    /// The app focus is on, by its place in `apps`.
    pub fn focused(&self) -> Option<usize> {
        match self.spot? {
            Spot::Row(i) => self.lines.row.get(i).copied(),
            Spot::Chip(i) => self.lines.chips.get(i).copied(),
            Spot::Grid(i) => self.lines.others.get(i).copied(),
            Spot::Gear => None,
        }
    }

    /// Whether the "Other apps" page is in front, or turning to.
    pub fn on_others(&self) -> bool {
        self.page.target() > 0.5
    }

    fn update_wanted(&mut self) {
        // The "Other apps" page keeps its tile's art while focus moves on
        // it: the art stays still behind the grid.
        let app = if self.on_others() {
            self.lines.row.last().copied()
        } else {
            self.focused().or(self.last_app)
        };
        self.wanted = match app.and_then(|i| self.apps.get(i)) {
            Some(app) if self.look.hero => Some(hero_for(app, &self.owner_art)),
            _ => None,
        };
    }

    /// The hero for where focus is: what the art should show once its
    /// textures are in. `None` without heroes in the look, or apps.
    pub fn wanted_hero(&self) -> Option<&Hero> {
        self.wanted.as_ref()
    }

    /// What the art shows, or is fading to; `None` is the gradient.
    pub fn shelf_target(&self) -> Option<&Hero> {
        self.shelf.target()
    }

    /// `hero`'s textures are in (`None`: the art is to show the gradient):
    /// it fades to it.
    pub fn show_hero(&mut self, hero: Option<Hero>) {
        if self.shelf.show(hero) {
            // Without the look's fade the art cuts: the crossfade ends on
            // its first frame, and the old hero can go at once.
            if !self.look.fade {
                self.shelf.settle_now();
            }
            self.dirty = true;
        }
    }

    /// The oldest crossfade that finished since the last call, so its old
    /// hero's textures can go.
    pub fn take_faded(&mut self) -> Option<Faded> {
        self.shelf.take_faded()
    }

    /// Whether the art is fading from one hero to another.
    pub fn is_fading(&self) -> bool {
        self.shelf.is_animating()
    }

    /// What was playing when the shell came in front, which Back on the row
    /// returns to; `None` for nothing. The shell says so from the app
    /// manager's foreground app.
    pub fn set_playing(&mut self, app: Option<AppId>) {
        if self.playing != app {
            self.playing = app;
            self.dirty = true;
        }
    }

    pub fn playing(&self) -> Option<&AppId> {
        self.playing.as_ref()
    }

    /// The time of day, from the shell: it owes a frame only when the
    /// minute shown changes.
    pub fn set_time(&mut self, hour: u8, minute: u8) {
        let time = Some((hour % 24, minute % 60));
        if self.clock != time {
            self.clock = time;
            self.dirty = true;
        }
    }

    /// Applies a key press at `now`, after stepping any motion in flight up to
    /// `now` so it is retargeted from where it really is.
    #[must_use]
    pub fn handle(&mut self, press: Press, now: Duration) -> Option<Effect> {
        self.advance_to(now);
        if self.control.is_open() {
            return self.control_press(press);
        }
        match press.action {
            Action::Move(dir) => {
                self.move_focus(dir, press.repeat);
                None
            }
            Action::Select => self.select(),
            Action::Back => self.back(),
        }
    }

    /// Where `dir` takes focus from `spot`, if anywhere.
    fn step(&mut self, spot: Spot, dir: Dir) -> Option<Spot> {
        let (row, chips) = (self.lines.row.len(), self.lines.chips.len());
        match (spot, dir) {
            (Spot::Row(i), Dir::Left) | (Spot::Chip(i), Dir::Left) if i == 0 => None,
            (Spot::Row(i), Dir::Left) => Some(Spot::Row(i - 1)),
            (Spot::Row(i), Dir::Right) => (i + 1 < row).then_some(Spot::Row(i + 1)),
            (Spot::Row(i), Dir::Down) => {
                let x = layout::row_tile(i).centre_x();
                layout::nearest(chips, x, layout::chip_rect).map(Spot::Chip)
            }
            (Spot::Row(_), Dir::Up) => Some(Spot::Gear),
            (Spot::Chip(_), Dir::Down) => None,
            (Spot::Chip(i), Dir::Left) => Some(Spot::Chip(i - 1)),
            (Spot::Chip(i), Dir::Right) => (i + 1 < chips).then_some(Spot::Chip(i + 1)),
            (Spot::Chip(i), Dir::Up) => {
                let x = layout::chip_rect(i).centre_x();
                // With no row, the gear is next up.
                Some(layout::nearest(row, x, layout::row_tile).map_or(Spot::Gear, Spot::Row))
            }
            (Spot::Gear, Dir::Down) => self.below_gear(),
            (Spot::Gear, _) => None,
            (Spot::Grid(_), _) => match self.grid.move_focus(dir) {
                Move::Moved => self.grid.index().map(Spot::Grid),
                Move::Blocked => None,
            },
        }
    }

    /// Where Down from the gear lands: the row's tile nearest below it,
    /// else the nearest chip.
    fn below_gear(&self) -> Option<Spot> {
        let x = layout::GEAR.centre_x();
        layout::nearest(self.lines.row.len(), x, layout::row_tile)
            .map(Spot::Row)
            .or_else(|| {
                layout::nearest(self.lines.chips.len(), x, layout::chip_rect).map(Spot::Chip)
            })
    }

    fn move_focus(&mut self, dir: Dir, repeat: bool) {
        let Some(spot) = self.spot else {
            return;
        };
        let from = self.focused();
        match self.step(spot, dir) {
            Some(to) => {
                self.spot = Some(to);
                self.focus_changed(from, dir);
            }
            // A repeat that reaches an edge stops there; only a fresh press bumps.
            None if !repeat => self.bump(dir),
            None => {}
        }
    }

    /// Focus went from app `from` (`None`: the gear) the way `dir` points:
    /// the new tile lifts, leans toward the move and the light crosses it
    /// (chips and the gear only lift).
    fn focus_changed(&mut self, from: Option<usize>, dir: Dir) {
        if let Some(from) = from {
            self.lifts[from].set_target(0.0);
        }
        let on_gear = self.spot == Some(Spot::Gear);
        self.gear_lift.set_target(if on_gear { 1.0 } else { 0.0 });
        let Some(to) = self.focused() else {
            self.sweep = None;
            self.dirty = true;
            return;
        };
        self.last_app = Some(to);
        self.lifts[to].set_target(1.0);
        let tile = !matches!(self.spot, Some(Spot::Chip(_)));
        if tile {
            self.lean(to, dir);
        }
        self.drift(dir);
        self.sweep = (tile && self.look.light).then_some(Sweep {
            tile: to,
            dir,
            from: self.now,
        });
        if let Some(Spot::Grid(i)) = self.spot {
            self.scroll
                .set_target(layout::grid_scroll_for_row(i / layout::GRID_COLUMNS));
        }
        self.update_wanted();
        self.dirty = true;
    }

    /// Nudges the focused tile toward `dir`. A nudge already under way only
    /// gets the kick it has room for, so mashing a key never pushes a tile
    /// past `BUMP_PEAK`.
    fn bump(&mut self, dir: Dir) {
        let Some(focused) = self.focused() else {
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
        // A blocked tile leans too, as if it met the edge; focus stays, so
        // no light crosses it.
        if !matches!(self.spot, Some(Spot::Chip(_))) {
            self.lean(focused, dir);
        }
        self.drift(dir);
        self.dirty = true;
    }

    /// Drifts a layered hero's layers against a move toward `dir`, with
    /// the tile's lean, and lets them spring back, if the look has parallax
    /// and the art, in full view on the home, shows such a hero. With the
    /// lean's room, a held key never drifts them past their
    /// `style::PARALLAX_PX`.
    fn drift(&mut self, dir: Dir) {
        let layered = self
            .shelf
            .target()
            .is_some_and(|hero| hero.is_layered(&self.look));
        if !self.look.tilt || !layered || self.on_others() {
            return;
        }
        let (axis, sign) = match dir {
            Dir::Left => (0, 1.0),
            Dir::Right => (0, -1.0),
            Dir::Up => (1, 1.0),
            Dir::Down => (1, -1.0),
        };
        let spring = &mut self.drift[axis];
        let room = (1.0 - (spring.value() * sign).max(0.0)).max(0.0);
        spring.set_velocity(sign * room * kick_for_peak(1.0, style::PARALLAX.omega));
    }

    /// Leans app `tile`'s tile toward `dir` and lets it spring upright, if
    /// the look tilts. Like the bump, a lean under way only gets the kick it
    /// has room for, so a key held or mashed never leans a tile past
    /// `style::TILT_DEGREES`.
    fn lean(&mut self, tile: usize, dir: Dir) {
        if !self.look.tilt {
            return;
        }
        let (axis, sign) = match dir {
            Dir::Left => (0, -1.0),
            Dir::Right => (0, 1.0),
            Dir::Up => (1, -1.0),
            Dir::Down => (1, 1.0),
        };
        let peak = style::TILT_DEGREES.to_radians();
        let spring = &mut self.tilts[tile][axis];
        let already = (spring.value() * sign).max(0.0);
        let room = (1.0 - already / peak).max(0.0);
        spring.set_velocity(sign * room * kick_for_peak(peak, style::TILT.omega));
    }

    /// Moves focus to the row's first tile, as a Home press with the shell
    /// in front does, turning back from the "Other apps" page if need be,
    /// with the lift of any move and the art following. False, and nothing
    /// changed, if focus is there already or there are no tiles.
    pub fn focus_first(&mut self, now: Duration) -> bool {
        self.advance_to(now);
        let closed = self.control.is_open();
        if closed {
            self.control.close();
            self.dirty = true;
        }
        let first = if self.lines.row.is_empty() {
            Spot::Chip(0)
        } else {
            Spot::Row(0)
        };
        if self.spot.is_none() || self.spot == Some(first) {
            return closed;
        }
        let from = self.focused();
        let dir = match self.spot {
            Some(Spot::Row(_)) => Dir::Left,
            Some(Spot::Gear) => Dir::Down,
            _ => Dir::Up,
        };
        self.turn_home();
        self.spot = Some(first);
        self.focus_changed(from, dir);
        true
    }

    /// Turns the page back to the home, if it is on "Other apps".
    fn turn_home(&mut self) {
        if self.on_others() {
            self.page.set_target(0.0);
        }
    }

    fn select(&mut self) -> Option<Effect> {
        if self.spot == Some(Spot::Gear) {
            self.control.open();
            self.dirty = true;
            return Some(Effect::ReadPrivacy);
        }
        let i = self.focused()?;
        if self.apps[i].id.as_str() == OTHERS_ID && !self.on_others() {
            // The page turns; focus starts on its first tile, its grid at
            // the top.
            self.page.set_target(1.0);
            self.grid.set(0);
            self.scroll.set_target(0.0);
            self.scroll.snap();
            self.lifts[i].set_target(0.0);
            self.spot = Some(Spot::Grid(0));
            if let Some(first) = self.focused() {
                self.lifts[first] = Spring::new(1.0, LIFT);
            }
            self.sweep = None;
            self.update_wanted();
            self.dirty = true;
            return None;
        }
        self.lifts[i].set_velocity(-kick_for_peak(PRESS_DIP, LIFT.omega));
        self.dirty = true;
        Some(Effect::Launch(self.apps[i].id.clone()))
    }

    /// A press while the Control Center is open: Up and Down move along
    /// Reduce Motion's rows, OK turns the focused effect off or on again
    /// (not one the launch look keeps off), Back closes it.
    fn control_press(&mut self, press: Press) -> Option<Effect> {
        match press.action {
            Action::Move(Dir::Up) => {
                self.control.step(-1);
            }
            Action::Move(Dir::Down) => {
                self.control.step(1);
            }
            Action::Move(_) => return None,
            Action::Select => {
                let motion = self.control.focused();
                let on_in_launch = {
                    let mut only = Reduced::default();
                    only.set(motion, true);
                    only.apply(self.launch_look) != self.launch_look
                };
                if !on_in_launch {
                    return None;
                }
                let off = !self.reduced.is_off(motion);
                self.reduced.set(motion, off);
                self.apply_reduced();
                self.dirty = true;
                return Some(Effect::SaveMotion(self.reduced));
            }
            Action::Back => self.control.close(),
        }
        self.dirty = true;
        None
    }

    /// The look after Reduce Motion changed: an effect turned off stops
    /// where it is, upright and still.
    fn apply_reduced(&mut self) {
        self.look = self.reduced.apply(self.launch_look);
        if !self.look.tilt {
            for tilt in self.tilts.iter_mut().flatten() {
                *tilt = Spring::new(0.0, style::TILT);
            }
        }
        if !self.look.tilt || !self.look.parallax {
            self.drift = [Spring::new(0.0, style::PARALLAX); 2];
        }
        if !self.look.light {
            self.sweep = None;
        }
    }

    fn back(&mut self) -> Option<Effect> {
        if self.spot == Some(Spot::Gear) {
            self.spot = self.below_gear();
            self.focus_changed(None, Dir::Down);
            return None;
        }
        let from = self.focused()?;
        match self.spot? {
            Spot::Grid(_) => {
                // Back to the home, onto the "Other apps" tile.
                self.page.set_target(0.0);
                self.lifts[from] = Spring::new(0.0, LIFT);
                self.spot = Some(Spot::Row(self.lines.row.len() - 1));
                if let Some(to) = self.focused() {
                    self.lifts[to].set_target(1.0);
                }
                self.sweep = None;
                self.update_wanted();
                self.dirty = true;
                None
            }
            Spot::Chip(i) if !self.lines.row.is_empty() => {
                let x = layout::chip_rect(i).centre_x();
                self.spot =
                    layout::nearest(self.lines.row.len(), x, layout::row_tile).map(Spot::Row);
                self.focus_changed(Some(from), Dir::Up);
                None
            }
            // On the row (or chips with no row): back to what was playing.
            _ => match (&self.playing, self.back_at_top) {
                (Some(id), _) => Some(Effect::Launch(id.clone())),
                (None, BackAtTop::Exit) => Some(Effect::Exit),
                (None, BackAtTop::Stay) => None,
            },
        }
    }

    /// Reports how a launch went, in the status line; an app that opened is
    /// what plays now, and when it was opened is kept.
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
        if result.is_ok() {
            self.opened.insert(id.clone(), self.now);
            self.set_playing(Some(id.clone()));
        }
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
        if self
            .sweep
            .is_some_and(|s| self.now >= s.from + style::SWEEP_TIME)
        {
            self.sweep = None;
            self.dirty = true;
        }
    }

    fn springs_mut(&mut self) -> impl Iterator<Item = &mut Spring> {
        self.lifts
            .iter_mut()
            .chain(self.tilts.iter_mut().flatten())
            .chain(&mut self.drift)
            .chain([
                &mut self.page,
                &mut self.scroll,
                &mut self.bump_x,
                &mut self.bump_y,
                &mut self.gear_lift,
            ])
            .chain(self.control.springs_mut())
    }

    pub fn is_animating(&self) -> bool {
        self.shelf.is_animating()
            || self.sweep.is_some()
            || !self
                .lifts
                .iter()
                .chain(self.tilts.iter().flatten())
                .chain(&self.drift)
                .chain([
                    &self.page,
                    &self.scroll,
                    &self.bump_x,
                    &self.bump_y,
                    &self.gear_lift,
                ])
                .chain(self.control.springs())
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
    /// The clock's minute is the shell's to watch (`set_time`).
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
        self.sweep = None;
        self.shelf.settle_now();
    }

    /// Where the light is on app `tile`'s tile, if it is crossing it: its
    /// band's middle runs from one band's width before the tile to one
    /// after, easing in and out.
    fn sweep_on(&self, tile: usize) -> Option<SweepView> {
        let sweep = self.sweep.filter(|s| s.tile == tile)?;
        let t = (self.now.saturating_sub(sweep.from).as_secs_f32()
            / style::SWEEP_TIME.as_secs_f32())
        .clamp(0.0, 1.0);
        let eased = t * t * (3.0 - 2.0 * t);
        let w = style::SWEEP_WIDTH;
        Some(SweepView {
            at: -w / 2.0 + (1.0 + w) * eased,
            dir: sweep.dir,
        })
    }

    /// The line under the focused item's name: what Lumen itself knows of
    /// it, never anything from the internet.
    fn subtitle(&self, app: &App) -> Option<String> {
        if app.id.as_str() == OTHERS_ID {
            let n = self.lines.others.len();
            return Some(format!("{n} more {}", if n == 1 { "app" } else { "apps" }));
        }
        if self.playing.as_ref() == Some(&app.id) {
            return Some("Playing now".to_owned());
        }
        let at = self.opened.get(&app.id)?;
        Some(opened_ago(self.now.saturating_sub(*at)))
    }

    /// A tile of `app` at `rest`, as it is now: lifted, bumped and leaning.
    fn tile_view(&self, app: usize, rest: Rect, alpha: f32, named: bool) -> TileView<'_> {
        let lift = self.lifts[app].value();
        let mut rect = layout::lifted(rest, lift);
        if app == self.bump_owner {
            rect = rect.offset(self.bump_x.value(), self.bump_y.value());
        }
        TileView {
            app: &self.apps[app],
            rect,
            lift,
            focused: self.focused() == Some(app),
            index: app,
            tilt: self.tilts[app].map(|s| s.value()),
            sweep: self.sweep_on(app),
            alpha,
            named,
        }
    }

    pub fn view(&self) -> View<'_> {
        let page = self.page.value().clamp(0.0, 1.0);
        let (home_alpha, others_alpha) = page_fades(page);
        let slide = if self.look.slide {
            style::PAGE_SLIDE
        } else {
            0.0
        };
        let mut tiles = Vec::new();
        let mut chips = Vec::new();
        if home_alpha > 0.001 {
            let dy = -slide * page;
            for (k, &app) in self.lines.row.iter().enumerate() {
                let rest = layout::row_tile(k).offset(0.0, dy);
                tiles.push(self.tile_view(app, rest, home_alpha, false));
            }
            for (k, &app) in self.lines.chips.iter().enumerate() {
                let lift = self.lifts[app].value();
                let mut rect = layout::chip_lifted(layout::chip_rect(k).offset(0.0, dy), lift);
                if app == self.bump_owner {
                    rect = rect.offset(self.bump_x.value(), self.bump_y.value());
                }
                chips.push(ChipView {
                    app: &self.apps[app],
                    rect,
                    lift,
                    focused: self.focused() == Some(app),
                    playing: self.playing.as_ref() == Some(&self.apps[app].id),
                    alpha: home_alpha,
                });
            }
        }
        let grid_dy = slide * (1.0 - page) - self.scroll.value();
        if others_alpha > 0.001 {
            for (k, &app) in self.lines.others.iter().enumerate() {
                let rest = layout::grid_tile(k).offset(0.0, grid_dy);
                let tile = self.tile_view(app, rest, others_alpha, true);
                if layout::tile_on_screen(tile.rect, tile.lift) {
                    tiles.push(tile);
                }
            }
        }
        // The focused tile goes last, over its neighbours.
        if let Some(at) = tiles.iter().position(|t| t.focused) {
            let focused = tiles.remove(at);
            tiles.push(focused);
        }
        let focused = self.focused().map(|i| &self.apps[i]);
        let on_home = !self.on_others();
        let on_gear = self.spot == Some(Spot::Gear);
        let gear_lift = self.gear_lift.value();
        let gear = (home_alpha > 0.001).then(|| GearView {
            rect: layout::GEAR
                .scaled(1.0 + (layout::GEAR_FOCUS_SCALE - 1.0) * gear_lift)
                .offset(0.0, -slide * page),
            lift: gear_lift,
            focused: on_gear,
            alpha: home_alpha,
        });
        let (title, subtitle) = if on_gear {
            (Some(CONTROL_TITLE), Some(CONTROL_SUBTITLE.to_owned()))
        } else {
            (
                focused.filter(|_| on_home).map(|a| a.title.as_str()),
                focused.filter(|_| on_home).and_then(|a| self.subtitle(a)),
            )
        };
        View {
            art: self.shelf.view(&self.look),
            drift: self.drift.map(|s| s.value()),
            page,
            page_slide: slide,
            title,
            subtitle,
            heading_y: layout::HEADING_BASELINE + grid_dy,
            clock: self.clock.map(|(h, m)| clock_text(h, m)),
            playing: self.playing.as_ref().map(|id| {
                self.apps
                    .iter()
                    .find(|app| &app.id == id)
                    .map_or(id.as_str(), |app| app.title.as_str())
            }),
            tiles,
            chips,
            status: self.status.as_ref().map(|(text, _)| text.as_str()),
            empty: self.apps.is_empty(),
            look: self.look,
            gear,
            control: self.control.view(self.launch_look, self.reduced, self.look),
        }
    }
}

/// The name above the row while focus is on the gear, and the line under it.
pub const CONTROL_TITLE: &str = "Control Center";
pub const CONTROL_SUBTITLE: &str = "Privacy and motion";

/// How much of the home and of the "Other apps" page shows at `page` (0 the
/// home, 1 the other page): the home goes over the first 60% of the turn
/// and the other page comes over the last 60%, so the two pages' tiles
/// cross only faintly, not as one muddle.
pub fn page_fades(page: f32) -> (f32, f32) {
    let page = page.clamp(0.0, 1.0);
    let home = ((0.6 - page) / 0.6).clamp(0.0, 1.0);
    let others = ((page - 0.4) / 0.6).clamp(0.0, 1.0);
    (home, others)
}

/// `9:41`: twelve hours, no leading zero, as a TV's corner shows it.
pub fn clock_text(hour: u8, minute: u8) -> String {
    let h = match hour % 12 {
        0 => 12,
        h => h,
    };
    format!("{h}:{minute:02}")
}

/// How long ago an app was opened, in the subtitle's words.
pub fn opened_ago(ago: Duration) -> String {
    let minutes = ago.as_secs() / 60;
    let hours = minutes / 60;
    let days = hours / 24;
    match (minutes, hours, days) {
        (0, _, _) => "Opened just now".to_owned(),
        (1, _, _) => "Opened a minute ago".to_owned(),
        (m, 0, _) => format!("Opened {m} minutes ago"),
        (_, 1, _) => "Opened an hour ago".to_owned(),
        (_, h, 0) => format!("Opened {h} hours ago"),
        (_, _, 1) => "Opened yesterday".to_owned(),
        (_, _, d) => format!("Opened {d} days ago"),
    }
}

#[cfg(test)]
mod tests;
