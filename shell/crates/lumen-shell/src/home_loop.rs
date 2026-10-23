//! The home loop (GLOSSARY.md). It reads the remote, lets
//! `lumen_core::home::Home` decide what happens, draws only while something
//! changes or moves, and waits in `SDL_WaitEventTimeout` once the screen has
//! settled (docs/specs/shell-mvp.md, "Repainting"). Every number it used to
//! show as hello-gles (GL renderer, frame times, key codes) goes to the log.

use std::sync::Arc;
use std::time::{Duration, Instant};

use lumen_core::apps::Launcher;
use lumen_core::home::Home;
use lumen_core::input::Input;
use lumen_core::script::Script;

use crate::app::{Options, launcher, on_tv, open_surface};
use crate::capture;
use crate::draw;
use crate::faults;
use crate::frame_stats::{BurstEnd, FrameStats};
use crate::gl::{self, Gl};
use crate::icons::Icons;
use crate::info;
use crate::luna;
use crate::render::{Batch, Renderer, fit_viewport};
use crate::sdl::{Event, Sdl};
use crate::timing::{FrameSplit, Phase, StartTimes};
use crate::watch;

mod clock;
mod shell;

use clock::Clock;
use shell::Shell;

/// When `--screenshot` takes its frame.
const SCREENSHOT_AFTER: Duration = Duration::from_millis(1500);
/// How often a settled screen looks for icons while the loader has some.
const ICON_POLL: Duration = Duration::from_millis(16);
/// How long the screen stays settled before glyphs are made ahead: three
/// refreshes at 60 Hz, so the burst's last frames, at most two still queued
/// on the B9, have gone out and the GPU is done with the atlas.
const WARM_AFTER: Duration = Duration::from_millis(50);

pub(crate) fn run_surface(sdl: &Sdl, opts: &Options, mut start: StartTimes) -> Result<(), String> {
    let surface = open_surface(sdl, opts)?;
    let gl = Gl::load(|name| sdl.gl_proc_address(name))?;
    let vsync = sdl.gl_set_swap_interval(1);
    // SAFETY: the surface's window is live for this whole function.
    let mut drawable = unsafe { sdl.drawable_size(surface.window) };
    // Motion is paced only with vsync and the real clock, from the first idle
    // on: the rate is asked for there, not before the first frame, since
    // LG's call may ask luna. A fixed clock asks SDL nothing, so its log
    // stays as it was.
    let mut refresh_due = vsync && opts.fixed_dt.is_none();
    start.mark(Phase::Gl, Instant::now());

    info!(
        "GL vendor {:?} renderer {:?} version {:?} glsl {:?}",
        gl.string(gl::VENDOR),
        gl.string(gl::RENDERER),
        gl.string(gl::VERSION),
        gl.string(gl::SHADING_LANGUAGE_VERSION)
    );
    info!(
        "drawable {}x{}, vsync {}",
        drawable.0,
        drawable.1,
        if vsync { "on" } else { "refused" }
    );

    let renderer = Renderer::new(gl)?;
    start.mark(Phase::Renderer, Instant::now());
    // A fixed clock wants the same frames every run, so its icons load at once.
    let mut assets = if opts.fixed_dt.is_some() {
        draw::Assets::new()?
    } else {
        draw::Assets::threaded()?
    };
    start.mark(Phase::Assets, Instant::now());
    let mut batch = Batch::default();
    let mut stats = FrameStats::new();
    let mut clock = Clock::new(opts.fixed_dt, None);
    let mut screenshot = opts.screenshot.clone();
    let mut settled_shot = opts.screenshot_when_settled.clone();
    let mut script = opts.keys.clone();
    if let Some(step) = opts.fixed_dt {
        info!("fixed clock: {:.3} ms a frame", step.as_secs_f64() * 1000.0);
    }
    // Every title's glyphs are made once (`warm`), while visible, when the
    // screen has stayed settled `WARM_AFTER` past a burst; a press before
    // then puts it off to the next settle. Not under a fixed clock: warming
    // changes where glyphs sit in the atlas, and so the frames.
    let mut warmed = clock.is_fixed();
    // When, in real time, it is due.
    let mut warm_at: Option<Duration> = None;
    let mut shell = Shell::new(launcher(), clock.now());
    start.mark(Phase::Apps, Instant::now());
    shell.can_hide_pointer = sdl.can_hide_pointer();
    shell.stamps_from = (!clock.is_fixed()).then_some(clock.start);
    // The luna watch (power state, the app in front, screensaver requests)
    // runs on its own thread for the life of the loop and stops when this
    // drops, after the loop. Nothing to watch off the TV, and nothing under
    // a fixed clock, whose "last input" times are not real time.
    let _watch = (on_tv() && !clock.is_fixed())
        .then(|| watch::start(Arc::clone(&shell.shared), luna::SELF_ID, clock.start));
    // Logged once, with the first frame or with why there was none.
    let mut start_line = Some(start);

    loop {
        // A settled screenshot that is ready must not wait for an event; one
        // that cannot be drawn yet (window hidden, icons on their way) must
        // not spin either.
        let shot_ready = settled_shot.is_some()
            && shell.visible
            && keys_done(script.as_ref(), &shell.input)
            && !assets.icons.any_loading();
        // A pass that starts with an idle wait: the GPU has most likely
        // finished with the atlas, so icons may be placed in it.
        let waited = !(shell.wants_frame() || shot_ready);
        let first = if !waited {
            sdl.poll_event()
        } else {
            shell.nothing_to_draw(Instant::now());
            // Drawing stopped: the burst is over, and with it the `fps` window.
            let over = stats.burst_over(BurstEnd::Idle, batch.quad_count(), clock.take_skipped());
            for line in &over.lines {
                info!("{line}");
            }
            clock.idle();
            if std::mem::take(&mut refresh_due) {
                // SAFETY: the surface's window is live for this whole
                // function.
                let refresh = unsafe { sdl.refresh(surface.window) };
                info!("{}", refresh.line());
                clock.pace(refresh.period().0);
            }
            if !warmed && !over.lines.is_empty() {
                warm_at = Some(clock.real() + WARM_AFTER);
            }
            if shell.visible && warm_at.is_some_and(|at| clock.real() >= at) {
                warm(&shell.home, &mut assets, &renderer);
                warmed = true;
                warm_at = None;
            }
            if over.read_gl_error {
                gl_error(&renderer);
            }
            // Deadlines on the loop's clock; --exit-after is in real time.
            let next = [
                shell.input.next_deadline(),
                shell.home.next_deadline(),
                // A launch waiting for its press's dip, should the screen
                // settle first.
                shell.launch_due_at(),
                script.as_ref().and_then(Script::next_deadline),
                // Only while it can be drawn: a hidden shell must not spin.
                screenshot
                    .as_ref()
                    .filter(|_| shell.visible)
                    .map(|_| SCREENSHOT_AFTER),
                // The same for icons; showing the shell again wakes the loop.
                (shell.visible && assets.icons.any_loading()).then(|| clock.now() + ICON_POLL),
                // The idle heartbeat. Not under a fixed clock, which would
                // jump 30 s at a time through it: there it only comes if the
                // keys or frames take the clock past it.
                (!clock.is_fixed()).then(|| shell.idle.next_deadline()),
                // Glyphs made ahead, once the burst's frames have gone out.
                warm_at.filter(|_| shell.visible),
            ]
            .into_iter()
            .flatten()
            .min();
            let until_exit = opts.exit_after.map(|e| e.saturating_sub(clock.real()));
            match next {
                Some(t) if clock.is_fixed() => {
                    clock.jump_to(t);
                    sdl.poll_event()
                }
                Some(t) => {
                    // By real time: a paced clock may run ahead of it, and
                    // waiting by that would wake early and spin.
                    let wait = t.saturating_sub(clock.real());
                    sdl.wait_event(Some(until_exit.map_or(wait, |e| e.min(wait))))
                }
                None => sdl.wait_event(until_exit),
            }
        };
        let pass = clock.pass();
        let now = pass.now;
        if let Some(script) = script.as_mut() {
            for key in script.due(now) {
                let event = if key.down {
                    Event::KeyDown {
                        scancode: key.scancode,
                        keycode: 0,
                        repeat: false,
                    }
                } else {
                    Event::KeyUp {
                        scancode: key.scancode,
                    }
                };
                shell.event(event, now, Instant::now());
            }
        }
        if let Some((event, at)) = first {
            shell.event(event, now, at);
        }
        while let Some((event, at)) = sdl.poll_event() {
            shell.event(event, now, at);
        }
        // After the events, which fired the repeats due by their own stamps
        // (so a key let go just now fires none after), the repeats due by the
        // pass's input time. They act at `now`, as presses do.
        for press in shell.input.poll(clock.input_time(pass)) {
            shell.press(press, now);
        }
        if shell.take_pointer_hide() && sdl.set_pointer_visible(false) {
            shell.pointer_hidden();
        }
        if let Some(line) = shell.heartbeat(now) {
            info!("{line}");
        }
        if !shell.visible
            && let Some(start) = start_line.take()
        {
            info!("{}", start.line_without_frame(shell.hidden_why));
        }
        if shell.quit {
            break;
        }
        if opts.exit_after.is_some_and(|limit| clock.real() >= limit) {
            info!("exit-after reached");
            break;
        }
        if screenshot.is_some() && now >= SCREENSHOT_AFTER {
            shell.home.mark_dirty();
        }
        if icons_arrived(&mut assets.icons, waited) {
            shell.home.mark_dirty();
        }
        shell.home.advance_to(clock.shows(now));
        // The settled screenshot: the keys are all played and released,
        // nothing moves and every icon is in, so draw the settled frame once
        // more and keep it.
        let take_settled = settled_shot.is_some()
            && keys_done(script.as_ref(), &shell.input)
            && !assets.icons.any_loading()
            && !shell.wants_frame();
        if take_settled {
            shell.home.mark_dirty();
        }
        if !shell.wants_frame() {
            // No frame to draw: a launch goes once its time has come, and
            // until then it is one of the idle deadlines.
            launch_pending(&mut shell, &mut stats, batch.quad_count(), now, &mut clock);
            continue;
        }

        // SAFETY: the window is live; see above.
        drawable = unsafe { sdl.drawable_size(surface.window) };
        let batch_from = Instant::now();
        draw::home(&mut batch, &shell.home.view(), &mut assets);
        let upload_from = Instant::now();
        let uploads = assets.atlas.take_uploads();
        renderer.upload(&uploads);
        let upload_count = uploads.len();
        let upload_bytes = uploads.iter().map(|u| u.rgba.len()).sum();
        // Freed here, inside the upload's time, not in the next frame's.
        drop(uploads);
        let submit_from = Instant::now();
        renderer.draw(&batch, drawable);
        let submitted = Instant::now();
        if now >= SCREENSHOT_AFTER
            && let Some(path) = screenshot.take()
        {
            save_screenshot(&renderer, drawable, &path);
        }
        if take_settled && let Some(path) = settled_shot.take() {
            save_screenshot(&renderer, drawable, &path);
            info!("settled at {:.3} s on the loop's clock", now.as_secs_f64());
            shell.quit = true;
        }
        let swap_from = Instant::now();
        // SAFETY: the window is live and its context is current.
        unsafe { sdl.gl_swap_window(surface.window) };
        let shown = Instant::now();
        // The main thread's page faults so far: `stats.frame` sets the
        // split's to the change since the last frame.
        let faults_now = faults::thread_faults();
        let split = FrameSplit {
            batch: upload_from.saturating_duration_since(batch_from),
            upload: submit_from.saturating_duration_since(upload_from),
            submit: submitted.saturating_duration_since(submit_from),
            swap: shown.saturating_duration_since(swap_from),
            uploads: upload_count,
            upload_bytes,
            glyphs: assets.text.take_rasterised(),
            faults: None,
        };
        if let Some(start) = start_line.take() {
            info!("{}", start.line(shown));
        }
        shell.frame_shown(shown);
        shell.home.frame_drawn();
        clock.frame_drawn(pass);
        if shell.quit {
            break;
        }

        let said = stats.frame(shown, split, faults_now, batch.quad_count());
        for line in &said.lines {
            info!("{line}");
        }
        if said.read_gl_error {
            gl_error(&renderer);
        }
        // The frame just drawn is on screen, showing `clock.shows(now)`: once
        // that is past the dip's deepest point, the launch may block the loop.
        launch_pending(
            &mut shell,
            &mut stats,
            batch.quad_count(),
            clock.shows(now),
            &mut clock,
        );
    }
    shell.leaving(Instant::now());
    if let Some(start) = start_line.take() {
        info!("{}", start.line_without_frame("the shell is leaving"));
    }
    let over = stats.burst_over(BurstEnd::Exit, batch.quad_count(), clock.take_skipped());
    for line in &over.lines {
        info!("{line}");
    }
    drop(surface);
    Ok(())
}

/// Logs GL's error flag, which holds until read, if it is set.
fn gl_error(renderer: &Renderer) {
    let err = renderer.gl().error();
    if err != gl::NO_ERROR {
        info!("GL error {err:#x}");
    }
}

/// Launches what a press asked for, if it is due by `now`. The burst, with
/// its `fps` window, and the pacer's burst end first, so the time luna takes
/// to answer is not counted as a frame, nor as vsyncs skipped. Returns
/// whether it launched.
fn launch_pending<L: Launcher>(
    shell: &mut Shell<L>,
    stats: &mut FrameStats,
    quads: usize,
    now: Duration,
    clock: &mut Clock,
) -> bool {
    if !shell.launch_due_at().is_some_and(|due| due <= now) {
        return false;
    }
    let over = stats.burst_over(BurstEnd::Launch, quads, clock.take_skipped());
    for line in &over.lines {
        info!("{line}");
    }
    clock.idle();
    shell.launch_if_due(now, || clock.now())
}

/// Makes every title's glyphs (`draw::warm`) and uploads them once the screen
/// has stayed settled for `WARM_AFTER`, so no burst waits on an upload and
/// the GPU is most likely done with the atlas. Any wait left lands in the
/// `warm` line's time, once, outside every burst.
fn warm(home: &Home, assets: &mut draw::Assets, renderer: &Renderer) {
    let from = Instant::now();
    draw::warm::warm(home.apps(), assets);
    let uploads = assets.atlas.take_uploads();
    renderer.upload(&uploads);
    let glyphs = assets.text.take_rasterised();
    info!("{}", draw::warm::line(glyphs, &uploads, from.elapsed()));
}

/// Takes in the icons the loader has finished, but only on a pass that began
/// with an idle wait: placed mid-burst, an icon would be uploaded into the
/// atlas that the frames in flight still read. (`receive` only keeps the
/// decoded image; it goes into the atlas when a frame draws it.) True if
/// any came.
fn icons_arrived(icons: &mut Icons, waited: bool) -> bool {
    waited && icons.receive()
}

/// Every scripted key has been pressed and released.
fn keys_done(script: Option<&Script>, input: &Input) -> bool {
    script.is_none_or(Script::is_done) && !input.is_holding()
}

/// Reads the back buffer before the swap, cropped to the 16:9 picture.
fn save_screenshot(renderer: &Renderer, drawable: (i32, i32), path: &std::path::Path) {
    let (x, y, w, h) = fit_viewport(drawable.0, drawable.1);
    let pixels = renderer.gl().read_rgba(x, y, w, h);
    match capture::save_gl_frame(path, w as u32, h as u32, &pixels) {
        Ok(()) => info!("screenshot {w}x{h} saved to {}", path.display()),
        Err(e) => info!("screenshot to {} failed: {e}", path.display()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    use lumen_core::apps::AppId;
    use lumen_core::input::scancode;

    use shell::tests::{at, focused_lift, key, ms, said, shell};

    #[test]
    fn icons_that_arrive_mid_burst_wait_for_the_next_idle() {
        // An icon is read only at a path that resolves to itself.
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../assets/mock-icons/play.png")
            .canonicalize()
            .expect("the simulator's icons are in the repo");
        let (mut icons, mut atlas) = (Icons::threaded(), crate::atlas::Atlas::default());
        assert!(icons.get(&path, &mut atlas).is_none(), "on its way");
        // Mid-burst, however long the loader has had, nothing is taken in.
        let started = Instant::now();
        while started.elapsed() < ms(50) {
            assert!(!icons_arrived(&mut icons, false));
            std::thread::sleep(ms(1));
        }
        assert!(icons.is_loading(&path), "held back");
        // After an idle wait it comes in.
        while !icons_arrived(&mut icons, true) {
            assert!(started.elapsed() < Duration::from_secs(10), "never came");
            std::thread::sleep(ms(1));
        }
        assert!(!icons.is_loading(&path));
        assert!(icons.get(&path, &mut atlas).is_some());
    }

    #[test]
    fn keys_are_done_once_played_and_released() {
        let mut script = Script::parse("R").unwrap();
        let mut input = Input::new();
        assert!(keys_done(None, &input));
        assert!(!keys_done(Some(&script), &input));
        assert_eq!(script.due(Duration::from_secs(5)).len(), 2);
        assert!(keys_done(Some(&script), &input));
        let _ = input.key_down(scancode::LEFT, false, ms(0));
        assert!(!keys_done(Some(&script), &input), "a key is still held");
    }

    #[test]
    fn the_launch_ends_the_burst_first() {
        let mut shell = shell();
        key(&mut shell, scancode::RETURN, 0);
        let mut stats = FrameStats::new();
        let t0 = Instant::now();
        let _ = stats.frame(t0, FrameSplit::default(), None, 10);
        let _ = stats.frame(t0 + ms(16), FrameSplit::default(), None, 10);
        let mut clock = Clock::new(None, None);
        assert!(
            said(|| assert!(!launch_pending(
                &mut shell,
                &mut stats,
                10,
                ms(16),
                &mut clock
            )))
            .is_empty(),
            "not due: the burst goes on"
        );
        let lines = said(|| {
            assert!(launch_pending(
                &mut shell,
                &mut stats,
                10,
                ms(34),
                &mut clock
            ));
        });
        assert_eq!(
            lines[0],
            "drew 2 frames in 16 ms (avg 16.00 ms, worst 16.00 ms, 10 quads); first 16.00 ms; idle"
        );
        assert!(lines[1].starts_with("launch netflix: answered in "));
        // The `fps` window starts again with it too (frame_stats.rs's tests).
        assert!(
            stats.burst_over(BurstEnd::Exit, 0, None).lines.is_empty(),
            "the wait for luna is no frame"
        );
    }

    #[test]
    fn the_launch_waits_for_a_frame_that_shows_the_dip() {
        let mut shell = shell();
        let mut stats = FrameStats::new();
        let mut clock = Clock::new(None, None);
        key(&mut shell, scancode::RETURN, 0);
        // Frames as the loop draws them at 60 Hz: springs stepped to the
        // pass's time, the frame shown, then the launch if it is due.
        let mut lift_before_launch = None;
        for n in 0..10_u64 {
            let now = Duration::from_micros(n * 16_667);
            shell.home.advance_to(now);
            assert!(shell.wants_frame(), "frame {n}");
            let lift = focused_lift(&shell);
            shell.frame_shown(at(n * 17));
            shell.home.frame_drawn();
            if launch_pending(&mut shell, &mut stats, 0, now, &mut clock) {
                lift_before_launch = Some((n, lift));
                break;
            }
            assert!(shell.launcher.launched().is_empty(), "frame {n}");
        }
        let (n, lift) = lift_before_launch.expect("launched within 10 frames");
        assert_eq!(n, 2, "the frame at 33.3 ms");
        assert!(lift < 0.8, "the last frame before the launch dips: {lift}");
        assert_eq!(shell.launcher.launched(), &[AppId::new("netflix")]);
    }
}
