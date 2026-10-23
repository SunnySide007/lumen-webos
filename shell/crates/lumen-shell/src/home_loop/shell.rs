//! The home loop's state apart from SDL and GL (`Shell`): the home screen,
//! the keys, the launch a press asked for and whether the shell is in front.
//! It turns events into presses and effects, and logs the timing lines.

use std::sync::Arc;
use std::time::{Duration, Instant};

use lumen_core::apps::{AppId, Launcher};
use lumen_core::home::{BackAtTop, Effect, Home, LIFT};
use lumen_core::input::{Action, Input, Press, action_for, scancode};

use crate::app::on_tv;
use crate::info;
use crate::sdl::{self, Event};
use crate::timing::{self, Foreground, Idle, KeyTimes, Pointer, SinceKey};
use crate::watch;

/// How long after Select its launch waits: the press pulse's dip is at its
/// deepest 1/ω of the lift spring after the kick, 33 ms.
const LAUNCH_AFTER: Duration = Duration::from_nanos((1e9 / LIFT.omega as f64) as u64);

/// Everything the loop decides, apart from SDL and GL, so it can be tested.
pub(super) struct Shell<L: Launcher> {
    pub(super) home: Home,
    pub(super) input: Input,
    pub(super) launcher: L,
    /// False while the window is hidden or the app is in the background:
    /// nothing is drawn then, not even an animation in flight.
    pub(super) visible: bool,
    /// Why the shell was last hidden, for lines about what it never showed.
    pub(super) hidden_why: &'static str,
    pub(super) quit: bool,
    /// An app to launch, and when on the loop's clock: once the press
    /// pulse's dip is at its deepest (`LAUNCH_AFTER`) and on screen.
    launch_due: Option<(AppId, Duration)>,
    /// The real clock's start, to put SDL's stamps on the loop's clock; none
    /// under a fixed clock, where an event counts at its pass's time.
    pub(super) stamps_from: Option<Instant>,
    /// Presses on their way to the screen (`key …: shown after …` lines).
    keys: KeyTimes,
    /// Coming back to the front, on its way to a frame.
    foreground: Foreground,
    /// What arrived since the remote's last key, for focus and lifecycle lines.
    since_key: SinceKey,
    /// Whether the window has keyboard focus, from SDL's focus events, for
    /// the heartbeat line.
    focused: bool,
    /// The Magic Remote's pointer, from the 484 and 485 keys.
    pointer: Pointer,
    /// Whether this SDL can hide the pointer (`SDL_webOSCursorVisibility`).
    pub(super) can_hide_pointer: bool,
    /// An arrow came while the pointer showed: the loop hides it.
    hide_pointer_due: bool,
    /// The last input and the next heartbeat (docs/specs/shell-mvp.md,
    /// "Input diagnostics").
    pub(super) idle: Idle,
    /// What the luna watch thread reads and writes.
    pub(super) shared: Arc<watch::Shared>,
}

impl<L: Launcher> Shell<L> {
    pub(super) fn new(mut launcher: L, now: Duration) -> Self {
        let (apps, failed) = match launcher.list() {
            Ok(apps) => (apps, None),
            Err(e) => (Vec::new(), Some(e)),
        };
        info!("{} apps", apps.len());
        let mut home = Home::new(apps, BackAtTop::Exit);
        if let Some(e) = failed {
            info!("listing apps failed: {e}");
            home.show_status(format!("Could not list apps: {e}"), now);
        }
        // The watch counts "last input" from the same moment the heartbeat
        // does: the start, as if input had just come.
        let shared = watch::Shared::new();
        shared.set_last_input(now);
        Self {
            home,
            input: Input::new(),
            launcher,
            visible: true,
            hidden_why: "the window was hidden",
            quit: false,
            launch_due: None,
            stamps_from: None,
            keys: KeyTimes::default(),
            foreground: Foreground::default(),
            since_key: SinceKey::default(),
            focused: false,
            pointer: Pointer::default(),
            can_hide_pointer: false,
            hide_pointer_due: false,
            idle: Idle::new(now),
            shared,
        }
    }

    /// Any input at `now`: a key (the pointer codes included) or a mouse
    /// event. The heartbeat and the watch's lines count from it.
    fn input(&mut self, now: Duration) {
        self.idle.input(now);
        self.shared.set_last_input(now);
    }

    /// The idle heartbeat line, if one is due at `now`. The power state is
    /// fetched (a lock and a copy) only when one is.
    pub(super) fn heartbeat(&mut self, now: Duration) -> Option<String> {
        if now < self.idle.next_deadline() {
            return None;
        }
        let power = self.shared.power();
        self.idle
            .beat(now, self.focused, self.pointer, power.as_deref())
    }

    /// Whether the loop should hide the pointer now (an arrow came while it
    /// showed); asked once per arrow.
    pub(super) fn take_pointer_hide(&mut self) -> bool {
        std::mem::take(&mut self.hide_pointer_due)
    }

    /// The loop hid the pointer. LG's 485, if it comes, says so again; if it
    /// never comes, the state is still right and no arrow asks again.
    pub(super) fn pointer_hidden(&mut self) {
        self.pointer = Pointer::Hidden;
    }

    /// When SDL stamped an event, on the loop's clock: never after `now`, and
    /// `now` itself under a fixed clock.
    fn stamp(&self, at: Instant, now: Duration) -> Duration {
        self.stamps_from
            .map_or(now, |start| at.saturating_duration_since(start).min(now))
    }

    pub(super) fn press(&mut self, press: Press, now: Duration) {
        match self.home.handle(press, now) {
            // The press pulse goes on screen first: the launch waits until
            // the dip is at its deepest and a frame shows it
            // (`launch_if_due`), since luna may take a third of a second to
            // answer and the loop cannot draw meanwhile. A second Select
            // before then launches nothing more.
            Some(Effect::Launch(id)) => {
                self.launch_due.get_or_insert((id, now + LAUNCH_AFTER));
            }
            Some(Effect::Exit) => {
                info!("back on the top row: leaving");
                self.quit = true;
            }
            None => {}
        }
    }

    /// When the launch a press asked for is due, on the loop's clock.
    pub(super) fn launch_due_at(&self) -> Option<Duration> {
        self.launch_due.as_ref().map(|&(_, due)| due)
    }

    /// Launches the app a press asked for, if it is due by `now`, the time
    /// of the frame just shown (or of the pass, when nothing is drawn).
    /// `answered_at` reads the loop's clock once luna has answered, so the
    /// status line's time starts after a launch that took seconds. Returns
    /// whether it launched.
    pub(super) fn launch_if_due(
        &mut self,
        now: Duration,
        answered_at: impl FnOnce() -> Duration,
    ) -> bool {
        let Some((id, _)) = self.launch_due.take_if(|(_, due)| *due <= now) else {
            return false;
        };
        let started = Instant::now();
        let result = self.launcher.launch(&id);
        let answered = Instant::now();
        info!(
            "{}",
            timing::launch_line(&id, answered.duration_since(started), &result)
        );
        self.keys.launched(started, answered);
        self.home.launched(&id, &result, answered_at());
        true
    }

    /// A launch asked for will not run: the shell was hidden, or is leaving.
    fn drop_launch(&mut self, why: &str) {
        if let Some((id, _)) = self.launch_due.take() {
            info!("{}", timing::launch_dropped_line(&id, why));
        }
    }

    /// `at` is when SDL stamped the event, for the timing lines.
    pub(super) fn event(&mut self, event: Event, now: Duration, at: Instant) {
        match event {
            Event::Quit => self.quit = true,
            Event::KeyDown {
                scancode,
                keycode,
                repeat,
            } => {
                let action = action_for(scancode);
                if !repeat {
                    info!("key down scancode {scancode} keycode {keycode:#x} ({action:?})");
                }
                self.heard_key(scancode, at);
                self.input(now);
                // LG sends the pointer codes as key-downs with no key-up, so
                // SDL may mark a later one as a repeat: the state decides,
                // not the flag. A line goes out when the state changes and
                // on every fresh press, so LG's own resends still show.
                if let Some(state) = pointer_state(scancode) {
                    if self.pointer != state || !repeat {
                        self.pointer = state;
                        info!("pointer {}", state.name());
                    }
                    return;
                }
                // Keys only reach the app in front, so a real press means the
                // screen is showing (SDL repeats do not count).
                if !repeat && action.is_some() {
                    self.show("a key press", at);
                }
                // An OK pressed while the pointer shows arrives as a click
                // (B9, 2026-10-22), so an arrow puts the pointer away where
                // the SDL allows it, as moonlight-tv does.
                if !repeat
                    && matches!(action, Some(Action::Move(_)))
                    && self.pointer == Pointer::Shown
                    && self.can_hide_pointer
                {
                    self.hide_pointer_due = true;
                }
                // Held keys and their repeats go by SDL's stamp: a press
                // first fires the repeats due before it; an SDL repeat read
                // late, after a stall, still keeps its key alive. Every press
                // acts at the pass's time, `now`.
                for press in self.input.key_down(scancode, repeat, self.stamp(at, now)) {
                    // Only presses are timed, not SDL's repeats or the shell's.
                    if !press.repeat {
                        self.keys.pressed(scancode, at);
                    }
                    self.press(press, now);
                }
            }
            Event::KeyUp { scancode } => {
                // The key-up SDL makes for a pointer code when the window
                // loses focus is not input (LG sends none itself), but it
                // still counts as an event in the focus lines, as before.
                if pointer_state(scancode).is_some() {
                    self.since_key.other();
                    return;
                }
                self.input(now);
                self.heard_key(scancode, at);
                // Only the repeats due before the release fire.
                for press in self.input.key_up(scancode, self.stamp(at, now)) {
                    self.press(press, now);
                }
            }
            Event::MouseButton { button, down, x, y } => {
                self.mouse_button(button, down, x, y, now, at)
            }
            Event::MouseMotion | Event::MouseWheel => {
                self.input(now);
                self.since_key.other();
            }
            Event::Window(id) => self.window(id, at),
            Event::Background => {
                info!("{}", self.since_key.line("app background", at));
                self.since_key.other();
                self.hide("the app went to the background", at);
            }
            Event::Foreground => {
                info!("{}", self.since_key.line("app foreground", at));
                self.since_key.other();
                self.show("did enter foreground", at);
            }
            Event::Other(_) => self.since_key.other(),
        }
    }

    /// Counts a key event toward the focus lines: a remote key the shell acts
    /// on resets them, anything else (pointer codes) is one more event.
    fn heard_key(&mut self, scancode: i32, at: Instant) {
        if action_for(scancode).is_some() {
            self.since_key.key(at);
        } else {
            self.since_key.other();
        }
    }

    /// A pointer button. Button 1 down is OK, through OK's own path: it is
    /// timed as key 40, counts as a key for the focus lines, and launches
    /// once a frame shows the press pulse's dip. Other buttons are only
    /// logged.
    fn mouse_button(&mut self, button: u8, down: bool, x: i32, y: i32, now: Duration, at: Instant) {
        self.input(now);
        let stamp = self.stamp(at, now);
        if !down {
            info!("mouse button {button} up at {x},{y}");
            self.since_key.other();
            if button == 1 {
                for press in self.input.key_up(scancode::RETURN, stamp) {
                    self.press(press, now);
                }
            }
            return;
        }
        if button != 1 {
            info!("mouse button {button} down at {x},{y}: ignored");
            self.since_key.other();
            return;
        }
        info!("mouse button 1 down at {x},{y}: taken as OK");
        self.since_key.key(at);
        self.show("a click", at);
        for press in self.input.key_down(scancode::RETURN, false, stamp) {
            if !press.repeat {
                self.keys.pressed(scancode::RETURN, at);
            }
            self.press(press, now);
        }
    }

    fn window(&mut self, id: u8, at: Instant) {
        use sdl::window::*;
        if let Some(name) = window_name(id) {
            info!("{}", self.since_key.line(&format!("window {name}"), at));
        }
        self.since_key.other();
        match id {
            FOCUS_GAINED => self.focused = true,
            FOCUS_LOST => self.focused = false,
            _ => {}
        }
        match id {
            HIDDEN | MINIMIZED => self.hide("the window was hidden", at),
            SHOWN => self.show("shown", at),
            RESTORED => self.show("restored", at),
            // Exposed means visible again (a hidden shell must come back), and
            // its last frame may be gone either way.
            EXPOSED => {
                self.show("exposed", at);
                self.home.mark_dirty();
            }
            RESIZED | SIZE_CHANGED => self.home.mark_dirty(),
            // On the TV, losing focus means another app is in front. On a PC
            // an unfocused window is still on screen, so it keeps drawing.
            FOCUS_LOST if on_tv() => self.hide("the window lost focus", at),
            FOCUS_LOST => self.input.release_all(),
            FOCUS_GAINED if on_tv() => self.show("focus gained", at),
            _ => {}
        }
    }

    fn hide(&mut self, why: &'static str, at: Instant) {
        if self.visible {
            info!("{why}: drawing stops");
            // The reason that hid it, not a later one while it stays hidden.
            self.hidden_why = why;
        }
        self.visible = false;
        self.input.release_all();
        self.home.settle_now();
        for line in self.keys.not_shown(at, why) {
            info!("{line}");
        }
        if let Some(line) = self.foreground.not_shown(at, why) {
            info!("{line}");
        }
        // Launched now, the app would open over whatever hid the shell.
        self.drop_launch(why);
    }

    fn show(&mut self, cause: &'static str, at: Instant) {
        if !self.visible {
            info!("in front again ({cause}): drawing resumes");
            self.visible = true;
            self.home.mark_dirty();
            self.foreground.came_back(at);
        }
    }

    /// A frame finished swapping at `at`: the presses and the return to the
    /// front it showed.
    pub(super) fn frame_shown(&mut self, at: Instant) {
        for line in self.keys.shown(at) {
            info!("{line}");
        }
        if let Some(line) = self.foreground.shown(at) {
            info!("{line}");
        }
    }

    /// The screen settled with presses that changed nothing on it.
    pub(super) fn nothing_to_draw(&mut self, at: Instant) {
        for line in self.keys.not_shown(at, "nothing to draw") {
            info!("{line}");
        }
    }

    /// The loop is ending; no frame will show what still waits, and no
    /// launch still waiting runs.
    pub(super) fn leaving(&mut self, at: Instant) {
        for line in self.keys.not_shown(at, "the shell is leaving") {
            info!("{line}");
        }
        self.drop_launch("the shell is leaving");
    }

    /// Whether the next pass of the loop draws a frame.
    pub(super) fn wants_frame(&self) -> bool {
        self.visible && !self.home.is_settled()
    }
}

/// The pointer state a scancode announces: 484 shown, 485 hidden.
fn pointer_state(scancode: i32) -> Option<Pointer> {
    match scancode {
        scancode::WEBOS_CURSOR_SHOW => Some(Pointer::Shown),
        scancode::WEBOS_CURSOR_HIDE => Some(Pointer::Hidden),
        _ => None,
    }
}

/// The window events that get a focus line. Exposed and resized are left out:
/// a PC sends them while a window is dragged.
fn window_name(id: u8) -> Option<&'static str> {
    use sdl::window::*;
    match id {
        FOCUS_LOST => Some("focus lost"),
        FOCUS_GAINED => Some("focus gained"),
        HIDDEN => Some("hidden"),
        SHOWN => Some("shown"),
        MINIMIZED => Some("minimized"),
        RESTORED => Some("restored"),
        _ => None,
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;

    use lumen_core::apps::{App, LaunchError, MockLauncher};

    pub(in crate::home_loop) fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    pub(in crate::home_loop) fn shell() -> Shell<MockLauncher> {
        let mut shell = Shell::new(MockLauncher::default(), Duration::ZERO);
        shell.home.frame_drawn();
        shell
    }

    /// `n` ms after one moment shared by every test, as SDL would stamp it.
    pub(in crate::home_loop) fn at(n: u64) -> Instant {
        static ZERO: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
        *ZERO.get_or_init(Instant::now) + ms(n)
    }

    /// An event `n` ms in, on the loop's clock and SDL's alike.
    fn send<L: Launcher>(shell: &mut Shell<L>, event: Event, n: u64) {
        shell.event(event, ms(n), at(n));
    }

    fn down(code: i32) -> Event {
        Event::KeyDown {
            scancode: code,
            keycode: 0,
            repeat: false,
        }
    }

    pub(in crate::home_loop) fn key<L: Launcher>(shell: &mut Shell<L>, code: i32, n: u64) {
        send(shell, down(code), n);
        send(shell, Event::KeyUp { scancode: code }, n);
    }

    /// The repeats the loop fires after a pass's events, `n` ms in.
    fn poll<L: Launcher>(shell: &mut Shell<L>, n: u64) {
        for press in shell.input.poll(ms(n)) {
            shell.press(press, ms(n));
        }
    }

    #[test]
    fn keys_move_focus_and_ok_launches_through_the_launcher() {
        let mut shell = shell();
        key(&mut shell, scancode::RIGHT, 0);
        assert_eq!(shell.home.focused(), Some(1));
        key(&mut shell, scancode::RETURN, 100);
        assert!(
            !shell.launch_if_due(ms(120), || ms(120)),
            "not before the dip"
        );
        assert!(shell.launch_if_due(ms(134), || ms(134)));
        assert_eq!(shell.launcher.launched(), &[AppId::new("amazon")]);
        assert_eq!(shell.home.view().status, Some("Opening Prime Video"));
        assert!(!shell.quit);
    }

    #[test]
    fn ok_shows_the_pulse_before_launching() {
        let mut shell = shell();
        key(&mut shell, scancode::RETURN, 0);
        assert!(shell.launcher.launched().is_empty(), "not before a frame");
        assert!(shell.wants_frame(), "the press pulse is drawn first");
        shell.frame_shown(at(17));
        assert!(!shell.keys.is_waiting(), "the press is on screen");
        assert!(
            !shell.launch_if_due(ms(17), || ms(17)),
            "the dip is not deepest yet"
        );
        assert!(shell.launch_if_due(ms(34), || ms(34)));
        assert_eq!(shell.launcher.launched(), &[AppId::new("netflix")]);
        assert!(
            !shell.launch_if_due(ms(50), || ms(50)),
            "one launch per press"
        );
    }

    #[test]
    fn a_second_ok_before_the_launch_launches_once_on_the_first_ones_time() {
        let mut shell = shell();
        key(&mut shell, scancode::RETURN, 0);
        key(&mut shell, scancode::RETURN, 5);
        assert!(shell.launch_if_due(ms(34), || ms(34)));
        assert!(!shell.launch_if_due(ms(50), || ms(50)));
        assert_eq!(shell.launcher.launched(), &[AppId::new("netflix")]);
    }

    #[test]
    fn back_on_the_top_row_quits() {
        let mut shell = shell();
        key(&mut shell, scancode::WEBOS_BACK, 0);
        assert!(shell.quit);
    }

    #[test]
    fn back_on_the_top_row_settles_the_press_as_leaving() {
        let mut shell = shell();
        key(&mut shell, scancode::WEBOS_BACK, 0);
        assert!(shell.keys.is_waiting());
        shell.leaving(at(3));
        assert!(!shell.keys.is_waiting());
    }

    #[test]
    fn a_failed_launch_says_so() {
        let mut shell = Shell::new(MockLauncher::default().failing_on("netflix"), ms(0));
        key(&mut shell, scancode::RETURN, 0);
        assert!(shell.launch_if_due(ms(34), || ms(34)));
        assert_eq!(shell.home.view().status, Some("Could not open Netflix"));
    }

    #[test]
    fn a_press_waits_for_a_frame_but_sdl_and_shell_repeats_do_not() {
        let mut shell = shell();
        send(&mut shell, down(scancode::RIGHT), 0);
        assert!(shell.keys.is_waiting());
        shell.frame_shown(at(17));
        assert!(!shell.keys.is_waiting());
        poll(&mut shell, 400);
        assert_eq!(shell.home.focused(), Some(2), "the shell repeated");
        assert!(!shell.keys.is_waiting(), "its repeats are not timed");
        let sdl_repeat = Event::KeyDown {
            scancode: scancode::RIGHT,
            keycode: 0,
            repeat: true,
        };
        send(&mut shell, sdl_repeat, 450);
        assert!(!shell.keys.is_waiting(), "nor are SDL's");
    }

    #[test]
    fn unmapped_keys_are_not_timed() {
        let mut shell = shell();
        // 484 arrives around Magic Remote pointer movement on the B9.
        key(&mut shell, 484, 0);
        assert!(!shell.keys.is_waiting());
    }

    fn click(button: u8, down: bool) -> Event {
        Event::MouseButton {
            button,
            down,
            x: 1408,
            y: 431,
        }
    }

    #[test]
    fn a_left_click_is_ok_through_oks_own_path() {
        let mut shell = shell();
        let lines = said(|| {
            send(&mut shell, click(1, true), 0);
            send(&mut shell, click(1, false), 150);
        });
        assert_eq!(
            lines,
            [
                "mouse button 1 down at 1408,431: taken as OK",
                "mouse button 1 up at 1408,431",
            ]
        );
        assert!(shell.wants_frame(), "the press pulse is drawn first");
        assert!(shell.launcher.launched().is_empty(), "not before the frame");
        assert!(shell.keys.is_waiting(), "timed like a press");
        let shown = said(|| shell.frame_shown(at(17)));
        assert_eq!(shown, ["key 40: shown after 17 ms"]);
        assert!(!shell.launch_if_due(ms(17), || ms(17)), "OK's own wait");
        assert!(shell.launch_if_due(ms(34), || ms(34)));
        assert_eq!(shell.launcher.launched(), &[AppId::new("netflix")]);
        assert!(!shell.input.is_holding(), "the up released it");
    }

    #[test]
    fn a_click_counts_as_a_key_for_the_focus_lines_and_shows_the_shell() {
        let mut shell = shell();
        send(&mut shell, Event::Window(sdl::window::HIDDEN), 0);
        let lines = said(|| {
            send(&mut shell, click(1, true), 1_000);
            send(&mut shell, Event::Window(sdl::window::FOCUS_LOST), 3_500);
        });
        assert_eq!(lines[0], "mouse button 1 down at 1408,431: taken as OK");
        assert_eq!(lines[1], "in front again (a click): drawing resumes");
        assert_eq!(
            lines[2],
            "window focus lost: last key 2.5 s ago, 0 other events since"
        );
    }

    #[test]
    fn other_buttons_are_logged_and_ignored() {
        let mut shell = shell();
        let lines = said(|| {
            send(&mut shell, click(3, true), 0);
            send(&mut shell, click(3, false), 100);
        });
        assert_eq!(
            lines,
            [
                "mouse button 3 down at 1408,431: ignored",
                "mouse button 3 up at 1408,431",
            ]
        );
        assert!(!shell.wants_frame());
        assert!(!shell.keys.is_waiting());
    }

    #[test]
    fn the_pointer_codes_track_the_pointer_and_their_key_ups_are_not_input() {
        let mut shell = shell();
        let lines = said(|| {
            send(&mut shell, down(scancode::WEBOS_CURSOR_SHOW), 1_000);
            send(&mut shell, down(scancode::WEBOS_CURSOR_HIDE), 6_000);
        });
        assert_eq!(lines, ["pointer shown", "pointer hidden"]);
        assert_eq!(shell.pointer, Pointer::Hidden);
        assert_eq!(shell.idle.last_input(), ms(6_000));
        // SDL's keyboard reset at a focus loss releases them; LG never does.
        // Such a key-up is not input, but it is still one more event in the
        // focus lines, as it was before the pointer was tracked.
        let up = Event::KeyUp {
            scancode: scancode::WEBOS_CURSOR_HIDE,
        };
        let lines = said(|| {
            send(&mut shell, up, 120_000);
            send(&mut shell, Event::Window(sdl::window::FOCUS_LOST), 120_000);
        });
        assert_eq!(shell.idle.last_input(), ms(6_000));
        assert!(!shell.input.is_holding());
        assert_eq!(
            lines[0],
            "window focus lost: no key yet, 3 other events since"
        );
        // LG sends no key-up for 484, so SDL may flag the next one as a
        // repeat: the code, not the flag, sets the state and counts as input.
        let repeated_show = Event::KeyDown {
            scancode: scancode::WEBOS_CURSOR_SHOW,
            keycode: 0,
            repeat: true,
        };
        let lines = said(|| send(&mut shell, repeated_show, 130_000));
        assert_eq!(lines, ["pointer shown"]);
        assert_eq!(shell.pointer, Pointer::Shown);
        assert_eq!(shell.idle.last_input(), ms(130_000));
        // A repeat that changes nothing logs nothing; a fresh 484 does.
        assert!(said(|| send(&mut shell, repeated_show, 131_000)).is_empty());
        assert_eq!(
            said(|| send(&mut shell, down(scancode::WEBOS_CURSOR_SHOW), 132_000)),
            ["pointer shown"]
        );
        assert!(!shell.keys.is_waiting(), "no action, nothing timed");
    }

    #[test]
    fn an_arrow_hides_a_shown_pointer_where_the_sdl_can() {
        let mut shell = shell();
        send(&mut shell, down(scancode::WEBOS_CURSOR_SHOW), 0);
        key(&mut shell, scancode::RIGHT, 100);
        assert!(!shell.take_pointer_hide(), "this SDL cannot hide it");
        shell.can_hide_pointer = true;
        key(&mut shell, scancode::RIGHT, 200);
        assert!(shell.take_pointer_hide());
        assert!(!shell.take_pointer_hide(), "asked once per arrow");
        // Not for OK, nor for a repeat.
        key(&mut shell, scancode::RETURN, 300);
        assert!(!shell.take_pointer_hide());
        let held_right = Event::KeyDown {
            scancode: scancode::RIGHT,
            keycode: 0,
            repeat: true,
        };
        send(&mut shell, held_right, 350);
        assert!(!shell.take_pointer_hide());
        // Once the loop has hidden it, no arrow asks again until LG shows it.
        shell.pointer_hidden();
        assert_eq!(shell.pointer, Pointer::Hidden);
        key(&mut shell, scancode::DOWN, 400);
        assert!(!shell.take_pointer_hide());
        send(&mut shell, down(scancode::WEBOS_CURSOR_SHOW), 450);
        key(&mut shell, scancode::DOWN, 500);
        assert!(shell.take_pointer_hide());
        // Nor once LG itself has hidden it.
        send(&mut shell, down(scancode::WEBOS_CURSOR_HIDE), 600);
        key(&mut shell, scancode::DOWN, 700);
        assert!(!shell.take_pointer_hide());
    }

    #[test]
    fn the_heartbeat_counts_from_the_last_input_of_any_kind() {
        let mut shell = shell();
        assert_eq!(shell.heartbeat(ms(29_000)), None);
        send(&mut shell, Event::MouseMotion, 10_000);
        assert_eq!(shell.heartbeat(ms(30_000)), None, "the motion was input");
        send(&mut shell, Event::Window(sdl::window::FOCUS_GAINED), 10_001);
        assert_eq!(
            shell.heartbeat(ms(40_000)).as_deref(),
            Some("no input for 30 s; focused yes; pointer hidden; power unknown")
        );
        assert_eq!(shell.idle.next_deadline(), ms(70_000));
        send(&mut shell, Event::Window(sdl::window::FOCUS_LOST), 50_000);
        assert_eq!(
            shell.heartbeat(ms(70_000)).as_deref(),
            Some("no input for 60 s; focused no; pointer hidden; power unknown")
        );
        // A key-up is input too, and so are an SDL repeat and the wheel.
        send(
            &mut shell,
            Event::KeyUp {
                scancode: scancode::RIGHT,
            },
            80_000,
        );
        assert_eq!(shell.idle.last_input(), ms(80_000));
        let held_right = Event::KeyDown {
            scancode: scancode::RIGHT,
            keycode: 0,
            repeat: true,
        };
        send(&mut shell, held_right, 81_000);
        assert_eq!(shell.idle.last_input(), ms(81_000));
        send(&mut shell, Event::MouseWheel, 82_000);
        assert_eq!(shell.idle.last_input(), ms(82_000));
        // The watch counts from the start too, as if input had just come.
        assert_eq!(
            Shell::new(MockLauncher::default(), ms(5))
                .shared
                .last_input(),
            ms(5)
        );
        assert_eq!(
            shell.shared.last_input(),
            ms(82_000),
            "shared with the watch"
        );
    }

    #[test]
    fn a_launch_then_background_leaves_the_press_unshown() {
        let mut shell = shell();
        key(&mut shell, scancode::RETURN, 0);
        // No frame shows the press before the launch; then the app is put away.
        assert!(shell.launch_if_due(ms(34), || ms(34)));
        assert!(shell.keys.is_waiting());
        send(&mut shell, Event::Background, 320);
        assert!(!shell.keys.is_waiting(), "settled as not shown");
    }

    #[test]
    fn a_press_with_nothing_to_draw_settles_at_idle() {
        let mut shell = Shell::new(Broken, ms(0));
        shell.home.frame_drawn();
        key(&mut shell, scancode::RIGHT, 0);
        assert!(!shell.wants_frame(), "an empty grid does not move");
        assert!(shell.keys.is_waiting());
        shell.nothing_to_draw(at(1));
        assert!(!shell.keys.is_waiting());
    }

    #[test]
    fn foreground_is_timed_from_the_event_that_brought_the_shell_back() {
        let mut shell = shell();
        assert!(
            !shell.foreground.is_waiting(),
            "the start-up is not a return"
        );
        send(&mut shell, Event::Background, 0);
        send(&mut shell, Event::Foreground, 100);
        assert!(shell.foreground.is_waiting());
        shell.frame_shown(at(140));
        assert!(!shell.foreground.is_waiting());
    }

    #[test]
    fn a_key_while_hidden_starts_the_foreground_clock() {
        let mut shell = shell();
        send(&mut shell, Event::Window(sdl::window::HIDDEN), 0);
        key(&mut shell, scancode::RIGHT, 10);
        assert!(shell.foreground.is_waiting());
        assert!(shell.keys.is_waiting());
        send(&mut shell, Event::Window(sdl::window::HIDDEN), 20);
        assert!(!shell.foreground.is_waiting() && !shell.keys.is_waiting());
    }

    /// The lines `f` logs, leaving out the raw `key down` lines.
    pub(in crate::home_loop) fn said(f: impl FnOnce()) -> Vec<String> {
        crate::log::capture(f)
            .into_iter()
            .filter(|line| !line.starts_with("key down"))
            .collect()
    }

    #[test]
    fn a_launch_reads_in_order_pulse_first() {
        let mut shell = shell();
        let lines = said(|| {
            key(&mut shell, scancode::RETURN, 0);
            shell.frame_shown(at(17));
            shell.launch_if_due(ms(34), || ms(34));
            send(&mut shell, Event::Background, 335);
        });
        assert_eq!(
            lines,
            [
                "key 40: shown after 17 ms",
                "launch netflix: answered in 0 ms",
                "app background: last key 0.3 s ago, 0 other events since",
                "the app went to the background: drawing stops",
            ]
        );
    }

    /// The mock's apps, launched after `ms` of waiting for an answer.
    struct Slow(MockLauncher, u64);

    impl Launcher for Slow {
        fn list(&mut self) -> Result<Vec<App>, LaunchError> {
            self.0.list()
        }
        fn launch(&mut self, id: &AppId) -> Result<(), LaunchError> {
            std::thread::sleep(ms(self.1));
            self.0.launch(id)
        }
    }

    #[test]
    fn a_press_waiting_through_a_slow_launch_names_it() {
        let mut shell = Shell::new(Slow(MockLauncher::default(), 5), ms(0));
        shell.home.frame_drawn();
        shell.event(down(scancode::RETURN), ms(0), Instant::now());
        let lines = said(|| {
            // No frame shows the press before the launch, which blocks 5 ms.
            shell.launch_if_due(ms(34), || ms(39));
            shell.event(Event::Background, ms(40), Instant::now());
        });
        let line = lines
            .iter()
            .find(|line| line.starts_with("key 40:"))
            .expect("a line for the press");
        assert!(
            line.contains(" ms in launch): the app went to the background"),
            "{line}"
        );
    }

    #[test]
    fn a_focus_line_counts_what_came_since_the_last_key() {
        let mut shell = shell();
        key(&mut shell, scancode::RIGHT, 0);
        let lines = said(|| {
            send(&mut shell, Event::Other(0x400), 100);
            // A pointer code is not a remote key.
            send(&mut shell, down(485), 200);
            send(&mut shell, Event::Window(sdl::window::FOCUS_LOST), 5_300);
        });
        assert_eq!(
            lines,
            [
                "pointer hidden",
                "window focus lost: last key 5.3 s ago, 2 other events since"
            ]
        );
    }

    #[test]
    fn coming_back_names_its_cause_and_the_first_hide_is_kept() {
        let mut shell = shell();
        send(&mut shell, Event::Background, 0);
        send(&mut shell, Event::Window(sdl::window::HIDDEN), 10);
        assert_eq!(shell.hidden_why, "the app went to the background");
        let lines = said(|| send(&mut shell, Event::Foreground, 20));
        assert_eq!(
            lines,
            [
                "app foreground: no key yet, 2 other events since",
                "in front again (did enter foreground): drawing resumes",
            ]
        );
    }

    #[test]
    fn focus_lines_name_only_the_window_events_that_matter() {
        use sdl::window::*;
        assert_eq!(window_name(FOCUS_LOST), Some("focus lost"));
        assert_eq!(window_name(RESTORED), Some("restored"));
        for noisy in [EXPOSED, RESIZED, SIZE_CHANGED] {
            assert_eq!(window_name(noisy), None);
        }
    }

    struct Broken;

    impl Launcher for Broken {
        fn list(&mut self) -> Result<Vec<App>, LaunchError> {
            Err(LaunchError("no luna".to_owned()))
        }
        fn launch(&mut self, _: &AppId) -> Result<(), LaunchError> {
            unreachable!("nothing to launch")
        }
    }

    #[test]
    fn a_failed_listing_shows_the_empty_state_and_the_error() {
        let shell = Shell::new(Broken, ms(0));
        let view = shell.home.view();
        assert!(view.empty);
        assert_eq!(view.status, Some("Could not list apps: no luna"));
    }

    #[test]
    fn going_to_the_background_stops_drawing_and_settles() {
        let mut shell = shell();
        key(&mut shell, scancode::DOWN, 0);
        assert!(shell.wants_frame());
        send(&mut shell, Event::Background, 10);
        assert!(!shell.visible && !shell.wants_frame());
        assert!(!shell.home.is_animating(), "springs jumped to rest");
        send(&mut shell, Event::Foreground, 20);
        assert!(shell.wants_frame(), "one fresh frame on return");
    }

    #[test]
    fn a_hidden_window_stops_drawing_and_drops_held_keys() {
        let mut shell = shell();
        send(&mut shell, down(scancode::RIGHT), 0);
        assert!(shell.input.is_holding());
        send(&mut shell, Event::Window(sdl::window::MINIMIZED), 10);
        assert!(!shell.visible && !shell.input.is_holding());
        send(&mut shell, Event::Window(sdl::window::RESTORED), 20);
        assert!(shell.visible && shell.wants_frame());
    }

    #[test]
    fn a_key_while_hidden_means_the_shell_is_in_front_again() {
        let mut shell = shell();
        send(&mut shell, Event::Window(sdl::window::HIDDEN), 0);
        key(&mut shell, scancode::RIGHT, 10);
        assert!(shell.visible);
        assert_eq!(shell.home.focused(), Some(1));
    }

    #[test]
    fn losing_focus_drops_keys_and_stops_drawing_only_on_the_tv() {
        let mut shell = shell();
        send(&mut shell, down(scancode::LEFT), 0);
        send(&mut shell, Event::Window(sdl::window::FOCUS_LOST), 10);
        assert!(!shell.input.is_holding());
        assert_eq!(shell.visible, !on_tv());
    }

    #[test]
    fn held_arrows_repeat_through_poll() {
        let mut shell = shell();
        send(&mut shell, down(scancode::RIGHT), 0);
        poll(&mut shell, 400);
        poll(&mut shell, 520);
        assert_eq!(shell.home.focused(), Some(3));
    }

    #[test]
    fn key_events_and_clicks_reach_input_by_their_stamps() {
        // Let go at 510, read in the pass at 525: the repeat due at 520 is
        // not owed, by a key-up or a click's release alike.
        for release in [
            Event::KeyUp {
                scancode: scancode::RIGHT,
            },
            click(1, false),
        ] {
            let mut shell = shell();
            shell.stamps_from = Some(at(0));
            send(&mut shell, down(scancode::RIGHT), 0);
            poll(&mut shell, 400);
            shell.event(release, ms(525), at(510));
            assert_eq!(shell.home.focused(), Some(2), "{release:?}");
        }
        // SDL's repeats, queued through a 3 s stall and read at its end,
        // keep the key alive by their own stamps.
        let mut shell = shell();
        shell.stamps_from = Some(at(0));
        send(&mut shell, down(scancode::RIGHT), 0);
        let held_right = Event::KeyDown {
            scancode: scancode::RIGHT,
            keycode: 0,
            repeat: true,
        };
        for t in (500..=2_950).step_by(50) {
            shell.event(held_right, ms(3_000), at(t));
        }
        assert!(shell.input.is_holding(), "not dropped as stuck");
    }

    #[test]
    fn a_press_that_comes_with_a_repeat_is_the_one_timed() {
        for ok in [down(scancode::RETURN), click(1, true)] {
            let mut shell = shell();
            send(&mut shell, down(scancode::DOWN), 0);
            shell.frame_shown(at(17));
            // Down's first repeat is due at 400: OK at 450 fires it first.
            send(&mut shell, ok, 450);
            assert_eq!(
                said(|| shell.frame_shown(at(467))),
                ["key 40: shown after 17 ms"],
                "{ok:?}"
            );
        }
    }

    #[test]
    fn a_stamp_after_the_pass_counts_at_the_pass() {
        let mut shell = shell();
        assert_eq!(shell.stamp(at(40), ms(25)), ms(25), "no clock: the pass");
        shell.stamps_from = Some(at(0));
        assert_eq!(shell.stamp(at(10), ms(25)), ms(10));
        assert_eq!(shell.stamp(at(40), ms(25)), ms(25), "clamped");
    }

    /// The focused tile's lift in a frame drawn now.
    pub(in crate::home_loop) fn focused_lift<L: Launcher>(shell: &Shell<L>) -> f32 {
        let view = shell.home.view();
        view.tiles
            .iter()
            .find(|tile| tile.focused)
            .expect("a focused tile")
            .lift
    }

    #[test]
    fn a_launch_asked_for_just_before_a_hide_is_dropped() {
        let mut shell = shell();
        let lines = said(|| {
            key(&mut shell, scancode::RETURN, 0);
            send(&mut shell, Event::Window(sdl::window::HIDDEN), 5);
        });
        assert!(
            lines.contains(&"launch netflix: dropped: the window was hidden".to_owned()),
            "{lines:?}"
        );
        assert!(!shell.launch_if_due(ms(100), || ms(100)));
        assert!(
            shell.launcher.launched().is_empty(),
            "nothing over LG's Home"
        );
    }

    #[test]
    fn a_launch_asked_for_just_before_leaving_is_dropped_with_a_line() {
        let mut shell = shell();
        key(&mut shell, scancode::RETURN, 0);
        key(&mut shell, scancode::WEBOS_BACK, 1);
        assert!(shell.quit, "BACK on the top row");
        let lines = said(|| shell.leaving(at(2)));
        assert_eq!(
            lines,
            [
                "key 40: not shown after 2 ms: the shell is leaving",
                "key 482: not shown after 1 ms: the shell is leaving",
                "launch netflix: dropped: the shell is leaving",
            ]
        );
        assert!(shell.launcher.launched().is_empty());
    }
}
