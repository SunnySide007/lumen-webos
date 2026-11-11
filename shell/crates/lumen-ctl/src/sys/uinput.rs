//! The kernel's side of the robot remote (`robot.rs`): a virtual keyboard
//! made through `/dev/uinput`, and the real remote's evdev nodes read without
//! grabbing them, so the TV's own input goes on exactly as before.
//!
//! The B9's kernel is 4.4, older than `UI_DEV_SETUP` (4.5), so the device is
//! described the old way: a `uinput_user_dev` written to the open file, then
//! `UI_DEV_CREATE`. Structures are built and read as bytes in the machine's
//! order; only the `ioctl` calls need `unsafe`.

use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

pub const EV_SYN: u16 = 0;
pub const EV_KEY: u16 = 1;
pub const EV_ABS: u16 = 3;
pub const EV_MSC: u16 = 4;
pub const ABS_X: u16 = 0;
pub const ABS_Y: u16 = 1;
const SYN_REPORT: u16 = 0;
/// `BUS_VIRTUAL`, as LG's own remotes' nodes are virtual too.
const BUS_VIRTUAL: u16 = 0x06;
const NAME_SIZE: usize = 80;
const ABS_CNT: usize = 64;

/// Linux's generic `ioctl` encoding, which ARM and x86 share:
/// direction, size, type `'U'`, number.
const fn ioc(dir: u32, nr: u32, size: u32) -> u32 {
    (dir << 30) | (size << 16) | ((b'U' as u32) << 8) | nr
}
const UI_DEV_CREATE: u32 = ioc(0, 1, 0);
const UI_DEV_DESTROY: u32 = ioc(0, 2, 0);
const UI_SET_EVBIT: u32 = ioc(1, 100, 4);
const UI_SET_KEYBIT: u32 = ioc(1, 101, 4);

/// `EVIOCGABS(axis)`: a read of type `'E'`, number `0x40 + axis`, into a
/// `struct input_absinfo` of six ints.
const fn eviocgabs(axis: u16) -> u32 {
    (2 << 30) | (ABSINFO_SIZE << 16) | ((b'E' as u32) << 8) | (0x40 + axis as u32)
}
const ABSINFO_SIZE: u32 = 24;

/// One absolute axis as the kernel holds it: where it is and may go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Axis {
    pub value: i32,
    pub min: i32,
    pub max: i32,
}

fn read_axis(file: &File, axis: u16) -> io::Result<Axis> {
    // value, minimum, maximum, fuzz, flat, resolution.
    let mut info = [0i32; 6];
    // SAFETY: the descriptor is open for the call's length (`file` is
    // borrowed), and `EVIOCGABS` writes one `struct input_absinfo`, 24
    // bytes, which is exactly `info`, alive and exclusively borrowed here.
    let rc = unsafe { libc::ioctl(file.as_raw_fd(), eviocgabs(axis) as _, info.as_mut_ptr()) };
    if rc < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(Axis {
        value: info[0],
        min: info[1],
        max: info[2],
    })
}

fn ioctl_int(file: &File, request: u32, value: libc::c_int) -> io::Result<()> {
    // SAFETY: the descriptor is open for the call's length (`file` is
    // borrowed); the three requests used take an int by value or nothing,
    // and pass no pointer the kernel could write through.
    let rc = unsafe { libc::ioctl(file.as_raw_fd(), request as _, value) };
    if rc < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// `struct uinput_user_dev`: the name, the id, `ff_effects_max` and four
/// zeroed `ABS_CNT` arrays.
fn user_dev(name: &str) -> Vec<u8> {
    let mut out = vec![0u8; NAME_SIZE];
    let bytes = name.as_bytes();
    let n = bytes.len().min(NAME_SIZE - 1);
    out[..n].copy_from_slice(&bytes[..n]);
    // bustype, vendor ("LU"), product, version.
    for v in [BUS_VIRTUAL, 0x4c55, 0x0001, 0x0001] {
        out.extend_from_slice(&v.to_ne_bytes());
    }
    out.extend_from_slice(&0u32.to_ne_bytes());
    out.resize(out.len() + 4 * ABS_CNT * 4, 0);
    out
}

/// The size of `struct timeval` here: 8 bytes on the TV, 16 on a 64-bit PC.
const TIMEVAL: usize = std::mem::size_of::<libc::timeval>();
/// `struct input_event`: a timeval, then type, code and value.
pub const EVENT_SIZE: usize = TIMEVAL + 8;

fn event_bytes(kind: u16, code: u16, value: i32) -> Vec<u8> {
    // The kernel stamps events written to uinput itself; the time is left 0.
    let mut out = vec![0u8; TIMEVAL];
    out.extend_from_slice(&kind.to_ne_bytes());
    out.extend_from_slice(&code.to_ne_bytes());
    out.extend_from_slice(&value.to_ne_bytes());
    out
}

/// One `input_event` read from a node, with the kernel's stamp.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InputEvent {
    /// Seconds on the kernel's input clock (`CLOCK_REALTIME` on the B9).
    pub at: f64,
    pub kind: u16,
    pub code: u16,
    pub value: i32,
}

pub fn parse_event(b: &[u8]) -> Option<InputEvent> {
    if b.len() != EVENT_SIZE {
        return None;
    }
    let half = TIMEVAL / 2;
    let word = |s: &[u8]| -> i64 {
        if half == 4 {
            i64::from(i32::from_ne_bytes(s.try_into().unwrap_or_default()))
        } else {
            i64::from_ne_bytes(s.try_into().unwrap_or_default())
        }
    };
    let sec = word(&b[..half]);
    let usec = word(&b[half..TIMEVAL]);
    let u16_at = |i: usize| u16::from_ne_bytes([b[i], b[i + 1]]);
    Some(InputEvent {
        at: sec as f64 + usec as f64 / 1e6,
        kind: u16_at(TIMEVAL),
        code: u16_at(TIMEVAL + 2),
        value: i32::from_ne_bytes(b[TIMEVAL + 4..TIMEVAL + 8].try_into().ok()?),
    })
}

/// Where the robot's key events go.
pub trait KeySink: Send {
    /// One key event (1 down, 0 up, 2 a repeat) and its `SYN_REPORT`.
    fn emit(&mut self, code: u16, value: i32) -> io::Result<()>;

    /// The pointer's X and Y axes, on a node that has a pointer.
    fn axes(&mut self) -> Option<(Axis, Axis)> {
        None
    }

    /// The pointer's place, in the axes' own units, and its `SYN_REPORT`.
    fn point(&mut self, _x: i32, _y: i32) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "this node has no pointer",
        ))
    }
}

fn point_and_sync(x: i32, y: i32) -> Vec<u8> {
    let mut bytes = event_bytes(EV_ABS, ABS_X, x);
    bytes.extend(event_bytes(EV_ABS, ABS_Y, y));
    bytes.extend(event_bytes(EV_SYN, SYN_REPORT, 0));
    bytes
}

fn key_and_sync(code: u16, value: i32) -> Vec<u8> {
    let mut bytes = event_bytes(EV_KEY, code, value);
    bytes.extend(event_bytes(EV_SYN, SYN_REPORT, 0));
    bytes
}

/// An existing input node opened for writing. evdev's write
/// (`input_inject_event`) hands the events to every reader of that device
/// as if the device had sent them, so a compositor that opened its nodes at
/// boot and never looks for new ones (LG's surface-manager on the B9) still
/// gets them.
pub struct Inject {
    file: File,
}

impl Inject {
    pub fn open(path: &Path) -> io::Result<Self> {
        let file = OpenOptions::new()
            .write(true)
            .custom_flags(libc::O_NONBLOCK | libc::O_CLOEXEC | libc::O_NOFOLLOW)
            .open(path)?;
        Ok(Self { file })
    }
}

impl KeySink for Inject {
    fn emit(&mut self, code: u16, value: i32) -> io::Result<()> {
        self.file.write_all(&key_and_sync(code, value))
    }

    fn axes(&mut self) -> Option<(Axis, Axis)> {
        let x = read_axis(&self.file, ABS_X).ok()?;
        let y = read_axis(&self.file, ABS_Y).ok()?;
        (x.max > x.min && y.max > y.min).then_some((x, y))
    }

    /// X, Y and the report in one write, so a reader never sees half.
    fn point(&mut self, x: i32, y: i32) -> io::Result<()> {
        self.file.write_all(&point_and_sync(x, y))
    }
}

impl KeySink for Uinput {
    fn emit(&mut self, code: u16, value: i32) -> io::Result<()> {
        Uinput::emit(self, code, value)
    }
}

/// A virtual keyboard with the given key codes; destroyed when dropped.
pub struct Uinput {
    file: File,
}

impl Uinput {
    pub fn create(name: &str, codes: &[u16]) -> io::Result<Self> {
        let mut file = OpenOptions::new()
            .write(true)
            .custom_flags(libc::O_NONBLOCK | libc::O_CLOEXEC)
            .open("/dev/uinput")?;
        ioctl_int(&file, UI_SET_EVBIT, libc::c_int::from(EV_KEY))?;
        for &code in codes {
            ioctl_int(&file, UI_SET_KEYBIT, libc::c_int::from(code))?;
        }
        file.write_all(&user_dev(name))?;
        ioctl_int(&file, UI_DEV_CREATE, 0)?;
        Ok(Self { file })
    }

    /// One key event (1 down, 0 up, 2 a repeat) and its `SYN_REPORT`, in one
    /// write so a reader never sees half.
    pub fn emit(&mut self, code: u16, value: i32) -> io::Result<()> {
        self.file.write_all(&key_and_sync(code, value))
    }
}

impl Drop for Uinput {
    fn drop(&mut self) {
        let _ = ioctl_int(&self.file, UI_DEV_DESTROY, 0);
    }
}

/// An evdev node opened read-only and non-blocking, never grabbed.
pub struct Evdev {
    file: File,
    pending: Vec<u8>,
}

impl Evdev {
    pub fn open(path: &Path) -> io::Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NONBLOCK | libc::O_CLOEXEC | libc::O_NOFOLLOW)
            .open(path)?;
        Ok(Self {
            file,
            pending: Vec::new(),
        })
    }

    /// Every whole event that has come since the last call.
    pub fn read_ready(&mut self) -> io::Result<Vec<InputEvent>> {
        let mut buf = [0u8; EVENT_SIZE * 32];
        loop {
            match self.file.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => self.pending.extend_from_slice(&buf[..n]),
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            }
        }
        let whole = self.pending.len() / EVENT_SIZE * EVENT_SIZE;
        let events = self.pending[..whole]
            .chunks(EVENT_SIZE)
            .filter_map(parse_event)
            .collect();
        self.pending.drain(..whole);
        Ok(events)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ioctl_numbers_are_linuxs() {
        assert_eq!(UI_DEV_CREATE, 0x5501);
        assert_eq!(UI_DEV_DESTROY, 0x5502);
        assert_eq!(UI_SET_EVBIT, 0x4004_5564);
        assert_eq!(UI_SET_KEYBIT, 0x4004_5565);
        assert_eq!(eviocgabs(ABS_X), 0x8018_4540);
        assert_eq!(eviocgabs(ABS_Y), 0x8018_4541);
    }

    #[test]
    fn a_pointer_move_is_x_y_and_a_report_in_one_write() {
        let b = point_and_sync(960, 540);
        assert_eq!(b.len(), 3 * EVENT_SIZE);
        let at = |i: usize| parse_event(&b[i * EVENT_SIZE..(i + 1) * EVENT_SIZE]).unwrap();
        assert_eq!((at(0).kind, at(0).code, at(0).value), (EV_ABS, ABS_X, 960));
        assert_eq!((at(1).kind, at(1).code, at(1).value), (EV_ABS, ABS_Y, 540));
        assert_eq!((at(2).kind, at(2).code, at(2).value), (EV_SYN, 0, 0));
    }

    #[test]
    fn the_device_description_has_the_kernels_size() {
        let dev = user_dev("Lumen robot remote");
        assert_eq!(dev.len(), 80 + 8 + 4 + 4 * 64 * 4);
        assert_eq!(&dev[..18], b"Lumen robot remote");
        assert_eq!(dev[18], 0);
        assert_eq!(u16::from_ne_bytes([dev[80], dev[81]]), BUS_VIRTUAL);
        let long = user_dev(&"x".repeat(200));
        assert_eq!(long[79], 0, "the name is always ended");
    }

    #[test]
    fn an_event_reads_back_as_written() {
        let mut b = event_bytes(EV_KEY, 106, 1);
        // A stamp of 12 s and 500,000 us.
        let half = TIMEVAL / 2;
        if half == 4 {
            b[..4].copy_from_slice(&12i32.to_ne_bytes());
            b[4..8].copy_from_slice(&500_000i32.to_ne_bytes());
        } else {
            b[..8].copy_from_slice(&12i64.to_ne_bytes());
            b[8..16].copy_from_slice(&500_000i64.to_ne_bytes());
        }
        assert_eq!(
            parse_event(&b),
            Some(InputEvent {
                at: 12.5,
                kind: EV_KEY,
                code: 106,
                value: 1
            })
        );
        assert_eq!(parse_event(&b[1..]), None);
    }
}
