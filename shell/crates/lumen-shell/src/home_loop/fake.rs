//! The home loop's adapters for tests: a wall clock that moves only when the
//! fakes move it, an event queue whose events fall due at set times, and a
//! screen that paints with the real draw code into the CPU atlas and records
//! what the loop asked of it, uploads included, instead of calling GL. Each
//! fake writes what it was asked to do, in order, into one shared `Acts`.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::path::Path;
use std::rc::Rc;
use std::time::{Duration, Instant};

use lumen_core::apps::{App, AppId, LaunchError, Launcher, MockLauncher};
use lumen_core::home::Home;

use crate::draw;
use crate::gl;
use crate::render::Batch;
use crate::sdl::{Event, Rate, Refresh};
use crate::timing::FrameSplit;

use super::{Events, Frame, Screen, WallClock};

/// One frame at 60 Hz.
pub const FRAME: Duration = Duration::from_nanos(16_666_667);
/// How far a test may run on the clock before it is taken to be stuck.
const LIMIT: Duration = Duration::from_secs(3600);
/// How many polls the loop may make with the clock standing still before it
/// is taken to spin: a pass that neither waits nor draws moves no clock.
const SPIN: u32 = 10_000;

/// Wall time that moves only when a fake moves it: a wait to its deadline or
/// to the next event, a swap by the frame's time, a launch by luna's.
#[derive(Debug, Clone)]
pub struct ManualClock {
    zero: Instant,
    now: Rc<Cell<Instant>>,
}

impl ManualClock {
    pub fn new() -> Self {
        let zero = Instant::now();
        Self {
            zero,
            now: Rc::new(Cell::new(zero)),
        }
    }

    /// `ms` milliseconds after the clock's zero.
    pub fn at(&self, ms: u64) -> Instant {
        self.zero + Duration::from_millis(ms)
    }

    /// How far the clock has moved from its zero.
    pub fn elapsed(&self) -> Duration {
        self.now.get().saturating_duration_since(self.zero)
    }

    fn advance(&self, by: Duration) {
        self.now.set(self.now.get() + by);
    }

    fn move_to(&self, t: Instant) {
        self.now.set(self.now.get().max(t));
    }
}

impl WallClock for ManualClock {
    fn now(&self) -> Instant {
        self.now.get()
    }
}

/// What the loop asked of the fakes, each at the clock's time then.
#[derive(Debug, Clone, PartialEq)]
pub enum Act {
    /// An idle wait, with its timeout.
    Wait(Duration, Option<Duration>),
    Paint(Duration),
    Screenshot(Duration),
    Swap(Duration),
    /// A look for finished icons, and whether any came.
    TakeIcons(Duration, bool),
    Warm(Duration),
    Refresh(Duration),
    GlRead(Duration),
    HidePointer(Duration),
    /// A launch, when luna was asked.
    Launch(Duration),
}

pub type Acts = Rc<RefCell<Vec<Act>>>;

fn act(acts: &Acts, clock: &ManualClock, act: impl FnOnce(Duration) -> Act) {
    acts.borrow_mut().push(act(clock.elapsed()));
}

/// Events that fall due at set times. Each is stamped when it happened and
/// read from when it arrives, at its stamp or later, as SDL's queue holds an
/// event the loop has not read yet.
pub struct FakeEvents {
    clock: ManualClock,
    acts: Acts,
    queue: VecDeque<Queued>,
    can_hide_pointer: bool,
    /// When the clock last stood, and the polls made since.
    still: (Instant, u32),
}

struct Queued {
    arrives: Instant,
    stamp: Instant,
    event: Event,
}

impl FakeEvents {
    pub fn new(clock: &ManualClock, acts: &Acts) -> Self {
        Self {
            clock: clock.clone(),
            acts: Rc::clone(acts),
            queue: VecDeque::new(),
            can_hide_pointer: false,
            still: (clock.now(), 0),
        }
    }

    /// `event` happens `ms` in.
    pub fn at(self, ms: u64, event: Event) -> Self {
        self.late(ms, ms, event)
    }

    /// `event` happens `stamp` ms in, but reaches the queue at `arrives`.
    pub fn late(mut self, stamp: u64, arrives: u64, event: Event) -> Self {
        let queued = Queued {
            arrives: self.clock.at(arrives),
            stamp: self.clock.at(stamp),
            event,
        };
        let at = self.queue.partition_point(|q| q.arrives <= queued.arrives);
        self.queue.insert(at, queued);
        self
    }

    /// A key pressed and let go `ms` in.
    pub fn tap(self, ms: u64, scancode: i32) -> Self {
        self.at(ms, down(scancode))
            .at(ms, Event::KeyUp { scancode })
    }

    /// The first queued event, if it has arrived.
    fn take_due(&mut self) -> Option<(Event, Instant)> {
        let due = self.queue.front()?.arrives <= self.clock.now();
        due.then(|| self.queue.pop_front())
            .flatten()
            .map(|q| (q.event, q.stamp))
    }
}

/// A key-down as SDL sends a fresh press.
pub fn down(scancode: i32) -> Event {
    Event::KeyDown {
        scancode,
        keycode: 0,
        repeat: false,
    }
}

impl Events for FakeEvents {
    fn poll(&mut self) -> Option<(Event, Instant)> {
        let now = self.clock.now();
        if self.still.0 == now {
            self.still.1 += 1;
            assert!(
                self.still.1 < SPIN,
                "the loop spins at {:?}",
                self.clock.elapsed()
            );
        } else {
            self.still = (now, 0);
        }
        self.take_due()
    }

    fn wait(&mut self, timeout: Option<Duration>) -> Option<(Event, Instant)> {
        act(&self.acts, &self.clock, |at| Act::Wait(at, timeout));
        let next = self.queue.front().map(|q| q.arrives);
        let until = timeout.map(|t| self.clock.now() + t);
        match (next, until) {
            (Some(next), Some(until)) if next > until => self.clock.move_to(until),
            (Some(next), _) => self.clock.move_to(next),
            (None, Some(until)) => self.clock.move_to(until),
            (None, None) => panic!("the loop waits forever with no event to come"),
        }
        assert!(self.clock.elapsed() < LIMIT, "the loop never ended");
        self.take_due()
    }

    fn can_hide_pointer(&self) -> bool {
        self.can_hide_pointer
    }

    fn hide_pointer(&mut self) -> bool {
        act(&self.acts, &self.clock, Act::HidePointer);
        self.can_hide_pointer
    }
}

/// A frame the fake screen showed.
#[derive(Debug, Clone, PartialEq)]
pub struct Painted {
    /// When its swap returned, from the clock's zero.
    pub shown: Duration,
    /// The focused tile's place in the list and its lift.
    pub focused: Option<(usize, f32)>,
    /// The atlas uploads it made.
    pub uploads: usize,
}

/// A screen that paints with the real draw code into the CPU atlas, and
/// takes the frame time the test gives each swap.
pub struct FakeScreen {
    clock: ManualClock,
    acts: Acts,
    assets: draw::Assets,
    batch: Batch,
    /// The frame painted last and its upload bytes, until its swap.
    painting: Option<(Painted, usize)>,
    /// What each swap takes.
    frame_time: Duration,
    /// When the one icon on its way arrives, if one is.
    icon_at: Option<Instant>,
    pub frames: Vec<Painted>,
    /// The uploads `warm` made.
    pub warm_uploads: usize,
}

impl FakeScreen {
    pub fn new(clock: &ManualClock, acts: &Acts) -> Self {
        Self {
            clock: clock.clone(),
            acts: Rc::clone(acts),
            assets: draw::Assets::new().expect("the built-in font parses"),
            batch: Batch::default(),
            painting: None,
            frame_time: FRAME,
            icon_at: None,
            frames: Vec::new(),
            warm_uploads: 0,
        }
    }

    /// Every swap takes `took`.
    pub fn frame_time(mut self, took: Duration) -> Self {
        self.frame_time = took;
        self
    }

    /// An icon is on its way and arrives `ms` in.
    pub fn icon_at(mut self, ms: u64) -> Self {
        self.icon_at = Some(self.clock.at(ms));
        self
    }
}

impl Screen for FakeScreen {
    fn paint(&mut self, home: &Home) {
        act(&self.acts, &self.clock, Act::Paint);
        let view = home.view();
        draw::home(&mut self.batch, &view, &mut self.assets);
        let uploads = self.assets.atlas.take_uploads();
        let bytes = uploads.iter().map(|u| u.rgba.len()).sum();
        let painted = Painted {
            shown: Duration::ZERO,
            focused: view
                .tiles
                .iter()
                .find(|tile| tile.focused)
                .map(|tile| (tile.index, tile.lift)),
            uploads: uploads.len(),
        };
        self.painting = Some((painted, bytes));
    }

    fn screenshot(&mut self, _: &Path) {
        act(&self.acts, &self.clock, Act::Screenshot);
    }

    fn swap(&mut self) -> Frame {
        self.clock.advance(self.frame_time);
        act(&self.acts, &self.clock, Act::Swap);
        let (mut painted, upload_bytes) = self.painting.take().expect("a swap after a paint");
        painted.shown = self.clock.elapsed();
        let frame = Frame {
            shown: self.clock.now(),
            split: FrameSplit {
                swap: self.frame_time,
                uploads: painted.uploads,
                upload_bytes,
                glyphs: self.assets.text.take_rasterised(),
                ..FrameSplit::default()
            },
            faults: None,
            quads: self.batch.quad_count(),
        };
        self.frames.push(painted);
        frame
    }

    fn take_icons(&mut self) -> bool {
        let came = self.icon_at.is_some_and(|at| at <= self.clock.now());
        if came {
            self.icon_at = None;
        }
        act(&self.acts, &self.clock, |at| Act::TakeIcons(at, came));
        came
    }

    fn icons_loading(&self) -> bool {
        self.icon_at.is_some()
    }

    fn warm(&mut self, apps: &[App]) {
        act(&self.acts, &self.clock, Act::Warm);
        draw::warm::warm(apps, &mut self.assets);
        let uploads = self.assets.atlas.take_uploads();
        self.warm_uploads += uploads.len();
        let glyphs = self.assets.text.take_rasterised();
        crate::info!("{}", draw::warm::line(glyphs, &uploads, Duration::ZERO));
    }

    fn refresh(&mut self) -> Refresh {
        act(&self.acts, &self.clock, Act::Refresh);
        Refresh {
            webos: Rate::Absent,
            mode: Rate::Hz(60),
            took: Duration::ZERO,
        }
    }

    fn gl_error(&mut self) -> u32 {
        act(&self.acts, &self.clock, Act::GlRead);
        gl::NO_ERROR
    }
}

/// The mock's apps, shared with the test, which reads what was launched
/// after the loop has taken the launcher. A launch moves the clock on by
/// `takes`, as waiting for luna's answer does.
#[derive(Clone)]
pub struct SharedLauncher {
    mock: Rc<RefCell<MockLauncher>>,
    clock: ManualClock,
    acts: Acts,
    takes: Duration,
}

impl SharedLauncher {
    pub fn new(mock: MockLauncher, clock: &ManualClock, acts: &Acts) -> Self {
        Self {
            mock: Rc::new(RefCell::new(mock)),
            clock: clock.clone(),
            acts: Rc::clone(acts),
            takes: Duration::ZERO,
        }
    }

    pub fn taking(mut self, takes: Duration) -> Self {
        self.takes = takes;
        self
    }

    pub fn launched(&self) -> Vec<AppId> {
        self.mock.borrow().launched().to_vec()
    }
}

impl Launcher for SharedLauncher {
    fn list(&mut self) -> Result<Vec<App>, LaunchError> {
        self.mock.borrow_mut().list()
    }

    fn launch(&mut self, id: &AppId) -> Result<(), LaunchError> {
        act(&self.acts, &self.clock, Act::Launch);
        self.clock.advance(self.takes);
        self.mock.borrow_mut().launch(id)
    }
}
