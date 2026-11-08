//! Paired devices and pairing (remote.md, R4; decision 12). Only lumen-ctl
//! writes them, as root, in its own folder (`/var/lib/lumen/remote/`, mode
//! 0700), so `lumen-remote`, which runs unprivileged in front of it, can
//! neither read the hashes nor give itself owner rights:
//!
//! * `devices.json`: each device's id, name, role, when it paired, and the
//!   SHA-256 of its token. The token itself goes to the phone once, in
//!   `pair.complete`'s reply, and is never kept.
//! * `pairing.json`: the open pairing window, if any: the six-digit code,
//!   when it ends (two minutes after `pair.open`) and the tries left (five).
//!   A right code closes it, and so does the fifth wrong one.
//!
//! Every change happens under `devices.lock` (an exclusive `flock`), since
//! the PC's session and `lumen-remote`'s run as two processes, and lands by
//! writing a new file and renaming it over the old, so a reader never sees
//! half of one. Files are read only when they are regular files owned by
//! lumen-ctl's own user, without following a link, as `privacy.state` reads
//! its own.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

use crate::privacy;
use crate::protocol::{ErrorCode, Refusal, Role};
use crate::sha256;
use crate::sys;

pub const STATE_DIR: &str = "/var/lib/lumen/remote";
/// How long a pairing window stays open.
pub const PAIR_SECONDS: u64 = 120;
/// Wrong codes before the window closes.
pub const MAX_ATTEMPTS: u64 = 5;
pub const MAX_DEVICES: usize = 16;
/// A token is 32 random bytes, sent as 64 hex digits.
pub const TOKEN_HEX: usize = 64;
const MAX_FILE: u64 = 64 << 10;

/// One paired device, as `devices.json` holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    pub id: String,
    pub name: String,
    pub role: Role,
    /// The SHA-256 of its token, in hex.
    pub hash: String,
    /// Unix seconds.
    pub paired: u64,
}

impl Device {
    /// What a client may see: never the hash.
    pub fn to_json(&self) -> Value {
        json!({ "id": self.id, "name": self.name, "role": self.role.as_str(), "paired": privacy::iso8601(self.paired) })
    }

    fn to_file(&self) -> Value {
        json!({ "id": self.id, "name": self.name, "role": self.role.as_str(), "sha256": self.hash, "paired": self.paired })
    }

    fn from_file(v: &Value) -> Option<Self> {
        let s = |k: &str| v.get(k).and_then(Value::as_str);
        let id = s("id").filter(|id| is_device_id(id))?;
        let hash = s("sha256").filter(|h| is_hex(h, 64))?;
        Some(Self {
            id: id.to_owned(),
            name: s("name")?.to_owned(),
            role: Role::parse(s("role")?)?,
            hash: hash.to_owned(),
            paired: v.get("paired").and_then(Value::as_u64)?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Pairing {
    code: String,
    expires: u64,
    attempts_left: u64,
}

/// A device id: eight lower-case hex digits.
pub fn is_device_id(id: &str) -> bool {
    is_hex(id, 8)
}

pub fn is_hex(s: &str, len: usize) -> bool {
    s.len() == len && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// Compares in time that depends only on the lengths.
fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn failed(e: impl std::fmt::Display) -> Refusal {
    Refusal::new(ErrorCode::Failed, e.to_string())
}

/// Where the files are, whose they must be, and the clock.
pub struct Store {
    dir: PathBuf,
    /// The owner the files must have: lumen-ctl's own user.
    owner: Option<u32>,
    clock: fn() -> u64,
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// What `stamp` compares: each file's inode, size and change time.
pub type Stamp = [Option<(u64, u64, i64, i64)>; 2];

impl Store {
    pub fn new(dir: &Path) -> Self {
        Self {
            dir: dir.to_owned(),
            owner: Some(sys::os::euid()),
            clock: unix_now,
        }
    }

    #[cfg(test)]
    pub fn with_clock(dir: &Path, clock: fn() -> u64) -> Self {
        Self {
            dir: dir.to_owned(),
            owner: Some(sys::os::euid()),
            clock,
        }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }

    /// Changes whenever either file is replaced or removed.
    pub fn stamp(&self) -> Stamp {
        ["devices.json", "pairing.json"].map(|name| {
            fs::symlink_metadata(self.path(name))
                .ok()
                .map(|m| (m.ino(), m.len(), m.mtime(), m.mtime_nsec()))
        })
    }

    /// Holds the lock until the guard drops.
    fn lock(&self) -> Result<File, Refusal> {
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .mode(0o600)
            .custom_flags(sys::os::OPEN_NO_FOLLOW)
            .open(self.path("devices.lock"))
            .map_err(|e| failed(format!("devices.lock: {e}")))?;
        file.lock()
            .map_err(|e| failed(format!("devices.lock: {e}")))?;
        Ok(file)
    }

    /// The file's JSON, `None` when it isn't there, an error when it is but
    /// can't be taken (not lumen-ctl's, a link, too big, not JSON).
    fn read_json(&self, name: &str) -> Result<Option<Value>, Refusal> {
        let path = self.path(name);
        match fs::symlink_metadata(&path) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(failed(format!("{name}: {e}"))),
            Ok(_) => {}
        }
        let text = privacy::read_owned(&path, self.owner, MAX_FILE)
            .ok_or_else(|| failed(format!("{name} is not lumen-ctl's own file")))?;
        serde_json::from_slice(&text)
            .map(Some)
            .map_err(|_| failed(format!("{name} is not JSON")))
    }

    fn write_json(&self, name: &str, value: &Value) -> Result<(), Refusal> {
        let path = self.path(name);
        let new = self.path(&format!("{name}.new"));
        let _ = fs::remove_file(&new);
        let write = || -> io::Result<()> {
            let mut file = OpenOptions::new()
                .create_new(true)
                .write(true)
                .mode(0o600)
                .custom_flags(sys::os::OPEN_NO_FOLLOW)
                .open(&new)?;
            file.write_all(value.to_string().as_bytes())?;
            file.write_all(b"\n")?;
            file.sync_all()?;
            fs::rename(&new, &path)
        };
        write().map_err(|e| {
            let _ = fs::remove_file(&new);
            failed(format!("{name}: {e}"))
        })
    }

    fn remove(&self, name: &str) -> Result<(), Refusal> {
        match fs::remove_file(self.path(name)) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(failed(format!("{name}: {e}"))),
            _ => Ok(()),
        }
    }

    pub fn devices(&self) -> Result<Vec<Device>, Refusal> {
        let Some(v) = self.read_json("devices.json")? else {
            return Ok(Vec::new());
        };
        v.get("devices")
            .and_then(Value::as_array)
            .and_then(|list| list.iter().map(Device::from_file).collect())
            .ok_or_else(|| failed("devices.json has a device it can't read"))
    }

    fn save_devices(&self, devices: &[Device]) -> Result<(), Refusal> {
        let list: Vec<Value> = devices.iter().map(Device::to_file).collect();
        self.write_json("devices.json", &json!({ "devices": list }))
    }

    fn pairing(&self) -> Result<Option<Pairing>, Refusal> {
        let Some(v) = self.read_json("pairing.json")? else {
            return Ok(None);
        };
        let code = v.get("code").and_then(Value::as_str);
        let expires = v.get("expires").and_then(Value::as_u64);
        let left = v.get("attemptsLeft").and_then(Value::as_u64);
        match (code, expires, left) {
            (Some(c), Some(expires), Some(attempts_left))
                if c.len() == 6 && c.bytes().all(|b| b.is_ascii_digit()) =>
            {
                Ok(Some(Pairing {
                    code: c.to_owned(),
                    expires,
                    attempts_left,
                }))
            }
            _ => Err(failed("pairing.json can't be read")),
        }
    }

    fn save_pairing(&self, p: &Pairing) -> Result<(), Refusal> {
        self.write_json(
            "pairing.json",
            &json!({ "code": p.code, "expires": p.expires, "attemptsLeft": p.attempts_left }),
        )
    }

    /// Opens a window with a new code, replacing any open one. Returns the
    /// code and how many seconds it stands.
    pub fn open_pairing(&self) -> Result<(String, u64), Refusal> {
        let _lock = self.lock()?;
        let code = random_code().map_err(|e| failed(format!("no random code: {e}")))?;
        self.save_pairing(&Pairing {
            code: code.clone(),
            expires: (self.clock)() + PAIR_SECONDS,
            attempts_left: MAX_ATTEMPTS,
        })?;
        Ok((code, PAIR_SECONDS))
    }

    pub fn close_pairing(&self) -> Result<(), Refusal> {
        let _lock = self.lock()?;
        self.remove("pairing.json")
    }

    /// A code from a phone: on the right one, a new remote-only device and
    /// its token; on a wrong one, a try fewer.
    pub fn complete(&self, code: &str, name: &str) -> Result<(Device, String), Refusal> {
        let _lock = self.lock()?;
        let closed = || {
            Refusal::new(
                ErrorCode::Unavailable,
                "Pairing is closed. Choose Pair a phone in Lumen Remote on the PC.",
            )
        };
        let Some(mut p) = self.pairing()? else {
            return Err(closed());
        };
        if (self.clock)() >= p.expires || p.attempts_left == 0 {
            self.remove("pairing.json")?;
            return Err(closed());
        }
        if !same(code.as_bytes(), p.code.as_bytes()) {
            p.attempts_left -= 1;
            if p.attempts_left == 0 {
                self.remove("pairing.json")?;
                return Err(Refusal::new(
                    ErrorCode::Forbidden,
                    "That code is wrong, and that was the fifth: pairing is closed. Open it again from the PC.",
                ));
            }
            self.save_pairing(&p)?;
            return Err(Refusal::new(
                ErrorCode::Forbidden,
                format!(
                    "That code is wrong. {} of {MAX_ATTEMPTS} tries left.",
                    p.attempts_left
                ),
            ));
        }
        let mut devices = self.devices()?;
        if devices.len() >= MAX_DEVICES {
            self.remove("pairing.json")?;
            return Err(Refusal::new(
                ErrorCode::Unavailable,
                format!("{MAX_DEVICES} devices are paired already: revoke one from the PC."),
            ));
        }
        let token = random_hex(TOKEN_HEX / 2).map_err(|e| failed(format!("no token: {e}")))?;
        let id = loop {
            let id = random_hex(4).map_err(|e| failed(format!("no id: {e}")))?;
            if !devices.iter().any(|d| d.id == id) {
                break id;
            }
        };
        let device = Device {
            id,
            name: name.to_owned(),
            role: Role::Remote,
            hash: sha256::hex(&sha256::digest(token.as_bytes())),
            paired: (self.clock)(),
        };
        devices.push(device.clone());
        self.save_devices(&devices)?;
        self.remove("pairing.json")?;
        Ok((device, token))
    }

    /// The device whose token this is, if any.
    pub fn auth(&self, token: &str) -> Result<Option<Device>, Refusal> {
        if !is_hex(token, TOKEN_HEX) {
            return Ok(None);
        }
        let hash = sha256::hex(&sha256::digest(token.as_bytes()));
        let mut found = None;
        // Every entry is compared, so the time taken says nothing of where.
        for d in self.devices()? {
            if same(d.hash.as_bytes(), hash.as_bytes()) {
                found = Some(d);
            }
        }
        Ok(found)
    }

    /// `devices.list`'s answer, and the `devices` event's: the devices
    /// without their hashes, and whether pairing is open (never its code).
    pub fn list(&self) -> Result<Value, Refusal> {
        let devices: Vec<Value> = self.devices()?.iter().map(Device::to_json).collect();
        let now = (self.clock)();
        let pairing = match self.pairing()? {
            Some(p) if p.expires > now && p.attempts_left > 0 => json!({
                "open": true,
                "secondsLeft": p.expires - now,
                "attemptsLeft": p.attempts_left,
            }),
            _ => json!({ "open": false, "secondsLeft": 0, "attemptsLeft": 0 }),
        };
        Ok(json!({ "devices": devices, "pairing": pairing }))
    }

    fn change(&self, id: &str, f: impl FnOnce(&mut Vec<Device>, usize)) -> Result<(), Refusal> {
        let _lock = self.lock()?;
        let mut devices = self.devices()?;
        let at = devices
            .iter()
            .position(|d| d.id == id)
            .ok_or_else(|| Refusal::new(ErrorCode::NotFound, format!("no device {id}")))?;
        f(&mut devices, at);
        self.save_devices(&devices)
    }

    pub fn revoke(&self, id: &str) -> Result<(), Refusal> {
        self.change(id, |devices, at| {
            devices.remove(at);
        })
    }

    pub fn set_role(&self, id: &str, role: Role) -> Result<(), Refusal> {
        self.change(id, |devices, at| devices[at].role = role)
    }
}

/// `n` bytes from the kernel's generator.
fn random(buf: &mut [u8]) -> io::Result<()> {
    File::open("/dev/urandom")?.read_exact(buf)
}

fn random_hex(n: usize) -> io::Result<String> {
    let mut buf = vec![0u8; n];
    random(&mut buf)?;
    Ok(sha256::hex(&buf))
}

/// Six digits, each equally likely: 4·10⁹ is a multiple of 10⁶, so a draw
/// below it, taken modulo 10⁶, is uniform.
fn random_code() -> io::Result<String> {
    loop {
        let mut b = [0u8; 4];
        random(&mut b)?;
        let n = u32::from_le_bytes(b);
        if n < 4_000_000_000 {
            return Ok(format!("{:06}", n % 1_000_000));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    struct Dir(PathBuf);
    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn dir(name: &str) -> Dir {
        let d =
            std::env::temp_dir().join(format!("lumen-ctl-devices-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        Dir(d)
    }

    static NOW: AtomicU64 = AtomicU64::new(1_000_000);
    fn clock() -> u64 {
        NOW.load(Ordering::SeqCst)
    }

    fn code_of(store: &Store) -> String {
        store.pairing().unwrap().unwrap().code
    }

    #[test]
    fn pairs_with_the_right_code_and_keeps_only_the_hash() {
        let d = dir("pair");
        let store = Store::new(&d.0);
        let (code, secs) = store.open_pairing().unwrap();
        assert_eq!((code.len(), secs), (6, 120));
        assert!(code.bytes().all(|b| b.is_ascii_digit()));
        assert_eq!(store.list().unwrap()["pairing"]["open"], json!(true));
        let (device, token) = store.complete(&code, "Pixel").unwrap();
        assert_eq!(device.role, Role::Remote);
        assert!(is_hex(&token, 64) && is_device_id(&device.id));
        // The window closed with the right code.
        assert!(!d.0.join("pairing.json").exists());
        assert_eq!(store.list().unwrap()["pairing"]["open"], json!(false));
        // The token is nowhere on disk; its hash is.
        let text = fs::read_to_string(d.0.join("devices.json")).unwrap();
        assert!(!text.contains(&token), "{text}");
        assert!(text.contains(&sha256::hex(&sha256::digest(token.as_bytes()))));
        let mode = fs::metadata(d.0.join("devices.json")).unwrap().mode() & 0o777;
        assert_eq!(mode, 0o600);
        // And it authenticates, while a wrong or malformed one doesn't.
        assert_eq!(store.auth(&token).unwrap(), Some(device.clone()));
        let mut other = token.clone();
        other.replace_range(0..1, if &token[..1] == "0" { "1" } else { "0" });
        assert_eq!(store.auth(&other).unwrap(), None);
        assert_eq!(store.auth("nope").unwrap(), None);
        assert_eq!(store.auth(&token.to_uppercase()).unwrap(), None);
        // The list carries no hash.
        let listed = store.list().unwrap();
        assert_eq!(listed["devices"][0]["name"], json!("Pixel"));
        assert!(listed["devices"][0].get("sha256").is_none());
    }

    #[test]
    fn five_wrong_codes_close_pairing() {
        let d = dir("wrong");
        let store = Store::new(&d.0);
        store.open_pairing().unwrap();
        let code = code_of(&store);
        let wrong = if code == "000000" { "000001" } else { "000000" };
        for left in (1..5).rev() {
            let e = store.complete(wrong, "x").unwrap_err();
            assert_eq!(e.code, ErrorCode::Forbidden);
            assert!(
                e.message.contains(&format!("{left} of 5 tries left")),
                "{}",
                e.message
            );
            assert_eq!(
                store.list().unwrap()["pairing"]["attemptsLeft"],
                json!(left)
            );
        }
        let e = store.complete(wrong, "x").unwrap_err();
        assert!(e.message.contains("pairing is closed"), "{}", e.message);
        // Closed: even the right code is refused now.
        let e = store.complete(&code, "x").unwrap_err();
        assert_eq!(e.code, ErrorCode::Unavailable);
        assert!(store.devices().unwrap().is_empty());
    }

    #[test]
    fn a_code_stands_two_minutes() {
        let d = dir("expiry");
        let store = Store::with_clock(&d.0, clock);
        NOW.store(1_000_000, Ordering::SeqCst);
        store.open_pairing().unwrap();
        let code = code_of(&store);
        NOW.store(1_000_000 + 119, Ordering::SeqCst);
        assert_eq!(store.list().unwrap()["pairing"]["secondsLeft"], json!(1));
        NOW.store(1_000_000 + 120, Ordering::SeqCst);
        assert_eq!(store.list().unwrap()["pairing"]["open"], json!(false));
        let e = store.complete(&code, "x").unwrap_err();
        assert_eq!(e.code, ErrorCode::Unavailable);
        // A new window gets a new code, with five tries again.
        store.open_pairing().unwrap();
        assert_eq!(store.list().unwrap()["pairing"]["attemptsLeft"], json!(5));
    }

    #[test]
    fn revokes_and_changes_roles() {
        let d = dir("revoke");
        let store = Store::new(&d.0);
        let mut tokens = Vec::new();
        for name in ["Pixel", "Tablet"] {
            store.open_pairing().unwrap();
            tokens.push(store.complete(&code_of(&store), name).unwrap());
        }
        let (pixel, pixel_token) = &tokens[0];
        store.set_role(&pixel.id, Role::Owner).unwrap();
        assert_eq!(store.auth(pixel_token).unwrap().unwrap().role, Role::Owner);
        store.revoke(&pixel.id).unwrap();
        assert_eq!(store.auth(pixel_token).unwrap(), None);
        // The other device stays.
        assert!(store.auth(&tokens[1].1).unwrap().is_some());
        assert_eq!(
            store.revoke(&pixel.id).unwrap_err().code,
            ErrorCode::NotFound
        );
        assert_eq!(
            store.set_role("00000000", Role::Owner).unwrap_err().code,
            ErrorCode::NotFound
        );
    }

    #[test]
    fn the_stamp_changes_with_every_write() {
        let d = dir("stamp");
        let store = Store::new(&d.0);
        let before = store.stamp();
        store.open_pairing().unwrap();
        let open = store.stamp();
        assert_ne!(before, open);
        store.close_pairing().unwrap();
        assert_ne!(store.stamp(), open);
    }

    #[test]
    fn refuses_files_it_did_not_write() {
        let d = dir("foreign");
        let store = Store::new(&d.0);
        fs::write(d.0.join("devices.json"), "not json").unwrap();
        assert_eq!(store.devices().unwrap_err().code, ErrorCode::Failed);
        // A broken store authenticates nobody and pairs nobody.
        assert!(store.auth(&"a".repeat(64)).is_err());
        fs::remove_file(d.0.join("devices.json")).unwrap();
        std::os::unix::fs::symlink("/etc/passwd", d.0.join("devices.json")).unwrap();
        assert!(store.devices().is_err());
    }

    #[test]
    fn codes_are_six_digits_and_vary() {
        let codes: std::collections::HashSet<String> =
            (0..50).map(|_| random_code().unwrap()).collect();
        assert!(codes.iter().all(|c| c.len() == 6));
        assert!(codes.len() > 40);
    }
}
