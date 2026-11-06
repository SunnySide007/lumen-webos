//! LG's app and input icons copied into Lumen's own folder, where the
//! shell's jail can read them: `packaging/shell/lumen-icons.sh` (docs/specs/icons.md)
//! ported to Rust, so root no longer runs a script from `/media/developer`,
//! which LG's developer mode makes 0777 at every boot (b9.md, "Platform").
//!
//! It makes every check the script makes, in the same order, and keeps the
//! copy's layout (`lg-icons/` mirrors each icon's absolute path, with
//! `.sum` and `status`), so the shell reads it unchanged:
//!
//! 1. The three shared folders the IPK sets (`/media/developer/apps/usr`,
//!    `usr/palm`, `usr/palm/applications`) and Lumen's own are real
//!    folders, root's, writable by no one else. The walk to them is one
//!    name at a time with no link followed, and Lumen's folder is held open
//!    from then on: every write is relative to it.
//! 2. The lock is the script's: `lg-icons.lock`, a link to the holder's
//!    pid, stale unless that pid's command line names `lumen-icons.sh` or
//!    lumend, so the two never copy at once while the old shell still runs
//!    the script.
//! 3. A copy cut off between the swap's two renames is put back.
//! 4. `listLaunchPoints`, asked by lumend itself, at most 4 MB, with
//!    `returnValue: true`.
//! 5. Each `icon` and `largeIcon` string in it, and LG Home's fixed icon
//!    path if anything is there: kept only under one of four fixed roots,
//!    in a folder named as an app id, through no link out of that folder,
//!    in plain characters, a regular file ending in `.png`; at most 128.
//!    `/media/developer` is left out.
//! 6. Each `.png` in LG's input icon folder, the same checks, at most 64.
//! 7. Nothing is copied if lumend, the plan and each planned file's size
//!    and times are as at the last copy.
//! 8. Each file is read once, at most 1 MB + 1 byte, without following a
//!    link or waiting on a pipe, and kept only if it is at most 1 MB,
//!    starts with the PNG signature and keeps the copy within 8 MB: the
//!    bytes are checked, not the file, so a file swapped after the plan
//!    still can't put anything else into Lumen's folder. (The script's
//!    `head` could hang on a pipe swapped in; this can't.)
//! 9. The new copy replaces the old after it is on disk.
//!
//! Files are 0644 and folders 0755, root's.

use std::collections::BTreeSet;
use std::ffi::{OsStr, OsString};
use std::io::Write;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use lumend_proto::{Icons, IconsFault};
use serde_json::Value;

use crate::bus::{self, Bus};
use crate::fsat::{Dir, check_private, open_regular, read_capped};
use crate::paths::{APP_ID, DEV_APPS, Root, SHARED};

pub const OUT: &str = "lg-icons";
const NEW: &str = "lg-icons.new";
const OLD: &str = "lg-icons.old";
const LOCK: &str = "lg-icons.lock";
/// The script's scratch folder, removed if a run of it left one.
const WORK: &str = "lg-icons.work";
const PART: &str = ".lumend-part";

/// Developer-mode apps: the jail reads their icons where they are.
const DIRECT: &str = "/media/developer/";
/// Where `getAllInputStatus` points every input on the B9.
pub const INPUTS: &str = "/usr/palm/plugins/inputapps/assets/small";
/// LG's Home, which no launch point names (its appinfo says `visible:
/// false`), at the path `listApps` gives on the B9.
pub const LG_HOME_ICON: &str = "/usr/palm/applications/com.webos.app.home/assets/icon.png";
/// The folders `listApps` gives LG's and the store's apps on the B9.
const ROOTS: [&str; 4] = [
    "/usr/palm/applications",
    "/media/cryptofs/apps/usr/palm/applications",
    "/media/system/apps/usr/palm/applications",
    "/mnt/otncabi/usr/palm/applications",
];
/// The shell's own limit (icons.rs).
const MAX_FILE: usize = 1 << 20;
const MAX_TOTAL: usize = 8 << 20;
const MAX_APP_ICONS: usize = 128;
const MAX_INPUT_ICONS: usize = 64;
const MAX_NAME: usize = 255;
const PNG: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
/// Entries a tree removal may take: far more than a copy holds.
const REMOVE_BUDGET: usize = 20_000;

type Failure = (IconsFault, String);

fn folders(e: impl std::fmt::Display) -> Failure {
    (IconsFault::Folders, e.to_string())
}

fn io(e: impl std::fmt::Display) -> Failure {
    (IconsFault::Io, e.to_string())
}

/// An app id or one file name: letters, digits, `.`, `_`, `-`; not `.` or
/// `..`; at most 255 bytes.
fn name_ok(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && name.len() <= MAX_NAME
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
}

/// A PNG's path inside an app's folder, in the same letters plus `/`, with
/// no empty, `.` or `..` part.
fn rel_ok(rel: &str) -> bool {
    rel.len() <= MAX_NAME
        && !rel.is_empty()
        && rel
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._/-".contains(&b))
        && rel
            .split('/')
            .all(|p| !p.is_empty() && p != "." && p != "..")
        && (rel.ends_with(".png") || rel.ends_with(".PNG"))
}

/// One planned copy: where it goes under `lg-icons.new`, and the file.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Item {
    dest: Vec<String>,
    real: PathBuf,
}

/// Plans `dir/rel` if it stays inside `dir` through any links, is in plain
/// characters and is a regular file. `dest` is where its copy goes.
fn plan(dir: &Path, rel: &str, dest: Vec<String>) -> Option<Item> {
    let base = std::fs::canonicalize(dir).ok()?;
    let real = std::fs::canonicalize(dir.join(rel)).ok()?;
    let inside = real.strip_prefix(&base).ok()?;
    if inside.as_os_str().is_empty() {
        return None;
    }
    let text = real.to_str()?;
    if !text
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b"._/-".contains(&b))
    {
        return None;
    }
    if !std::fs::metadata(&real).ok()?.is_file() {
        return None;
    }
    Some(Item { dest, real })
}

fn parts(path: &str) -> impl Iterator<Item = String> + '_ {
    path.split('/').filter(|p| !p.is_empty()).map(str::to_owned)
}

/// Plans one icon luna named, if it is under a fixed root, in a folder
/// named as an app id.
fn app_icon(root: &Root, path: &str) -> Option<Item> {
    for r in ROOTS {
        let Some(rest) = path.strip_prefix(r).and_then(|s| s.strip_prefix('/')) else {
            continue;
        };
        let (id, rel) = rest.split_once('/')?;
        if id.starts_with('.') || !name_ok(id) || !rel_ok(rel) {
            return None;
        }
        let dest = parts(r).chain([id.to_owned()]).chain(parts(rel)).collect();
        return plan(&root.path(&format!("{r}/{id}")), rel, dest);
    }
    None
}

/// Every `icon` and `largeIcon` string in the reply, at any depth.
fn icon_strings(value: &Value, out: &mut BTreeSet<String>) {
    match value {
        Value::Object(map) => {
            for (key, v) in map {
                match (key.as_str(), v) {
                    ("icon" | "largeIcon", Value::String(s)) => {
                        out.insert(s.clone());
                    }
                    _ => icon_strings(v, out),
                }
            }
        }
        Value::Array(items) => items.iter().for_each(|v| icon_strings(v, out)),
        _ => {}
    }
}

/// The plan, and how many were refused while making it.
fn make_plan(root: &Root, reply: &Value) -> (Vec<Item>, u32) {
    let mut paths = BTreeSet::new();
    icon_strings(reply, &mut paths);
    if std::fs::symlink_metadata(root.path(LG_HOME_ICON)).is_ok() {
        paths.insert(LG_HOME_ICON.to_owned());
    }
    let mut items = Vec::new();
    let mut refused = 0;
    let mut n = 0;
    for path in &paths {
        if path.is_empty() || path.starts_with(DIRECT) {
            continue;
        }
        match (n < MAX_APP_ICONS).then(|| app_icon(root, path)).flatten() {
            Some(item) => {
                items.push(item);
                n += 1;
            }
            None => refused += 1,
        }
    }
    let inputs = root.path(INPUTS);
    let mut names: Vec<OsString> = std::fs::read_dir(&inputs)
        .map(|d| d.filter_map(|e| e.ok().map(|e| e.file_name())).collect())
        .unwrap_or_default();
    names.sort();
    let mut n = 0;
    for name in names {
        let bytes = name.as_encoded_bytes();
        if bytes.starts_with(b".") || !(bytes.ends_with(b".png") || bytes.ends_with(b".PNG")) {
            continue;
        }
        let planned = name
            .to_str()
            .filter(|s| n < MAX_INPUT_ICONS && name_ok(s))
            .and_then(|s| plan(&inputs, s, parts(INPUTS).chain([s.to_owned()]).collect()));
        match planned {
            Some(item) => {
                items.push(item);
                n += 1;
            }
            None => refused += 1,
        }
    }
    (items, refused)
}

/// FNV-1a, 64 bits: change detection, nothing more (anyone can compute it,
/// so `.sum` is trusted only in a copy only root can write).
fn fnv(bytes: &[u8], mut hash: u64) -> u64 {
    for b in bytes {
        hash ^= u64::from(*b);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

/// The sum of what a copy depends on: lumend's version, the plan, and each
/// planned file's mode, links, owner, size and times.
fn sum(items: &[Item]) -> String {
    let mut text = format!("lumend {} icons 1\n", env!("CARGO_PKG_VERSION"));
    for item in items {
        text.push_str(&format!(
            "{} {}\n",
            item.dest.join("/"),
            item.real.display()
        ));
    }
    for item in items {
        match std::fs::metadata(&item.real) {
            Ok(m) => text.push_str(&format!(
                "{:o} {} {} {} {} {} {}\n",
                m.mode(),
                m.nlink(),
                m.uid(),
                m.gid(),
                m.size(),
                m.mtime(),
                m.mtime_nsec()
            )),
            Err(_) => text.push_str("-\n"),
        }
    }
    let a = fnv(text.as_bytes(), 0xcbf2_9ce4_8422_2325);
    let b = fnv(text.as_bytes(), a ^ 0x9e37_79b9_7f4a_7c15);
    format!("{a:016x}{b:016x}")
}

/// Lumen's folder, held open after every folder above it the IPK sets has
/// passed its check.
fn pin(root: &Root, me: u32) -> Result<Dir, String> {
    let shown = |n: usize| format!("{DEV_APPS}/{}", SHARED[..n].join("/"));
    let mut dir = Dir::open_path(&root.path(DEV_APPS)).map_err(|e| format!("{DEV_APPS}: {e}"))?;
    for (n, name) in SHARED.iter().enumerate() {
        dir = dir
            .open_dir(OsStr::new(name))
            .map_err(|e| format!("{}: {e}", shown(n + 1)))?;
        check_private(&dir.meta().map_err(|e| e.to_string())?, me, &shown(n + 1))?;
    }
    let app = dir
        .open_dir(OsStr::new(APP_ID))
        .map_err(|e| format!("Lumen's folder: {e}"))?;
    check_private(
        &app.meta().map_err(|e| e.to_string())?,
        me,
        "Lumen's folder",
    )?;
    Ok(app)
}

/// Whether `pid` is a run of the script or of lumend, by its command line.
fn holder_alive(pid: &str) -> bool {
    if pid.is_empty() || !pid.bytes().all(|b| b.is_ascii_digit()) {
        return false;
    }
    std::fs::read(format!("/proc/{pid}/cmdline")).is_ok_and(|raw| {
        let line = String::from_utf8_lossy(&raw).replace('\0', " ");
        line.contains("lumen-icons.sh") || line.contains("lumend/lumend")
    })
}

/// The script's lock, held until dropped.
struct Lock<'a> {
    app: &'a Dir,
    pid: String,
}

impl<'a> Lock<'a> {
    fn holder(app: &Dir) -> Option<String> {
        app.readlink(OsStr::new(LOCK))
            .ok()
            .map(|p| p.to_string_lossy().into_owned())
    }

    /// The lock, or `None` while another run holds it.
    fn take(app: &'a Dir) -> Result<Option<Self>, Failure> {
        let name = OsStr::new(LOCK);
        let pid = std::process::id().to_string();
        // A folder in its place would take the link inside it.
        if app.stat_opt(name).map_err(io)?.is_some_and(|m| m.is_dir()) {
            app.remove_tree(name, &mut { REMOVE_BUDGET }).map_err(io)?;
        }
        if app.symlink(Path::new(&pid), name).is_ok() {
            return Ok(Some(Self { app, pid }));
        }
        let holder = Self::holder(app).unwrap_or_default();
        if holder_alive(&holder) {
            return Ok(None);
        }
        // Only the stale lock: another run may have just taken it over.
        if Self::holder(app).as_deref() == Some(holder.as_str()) {
            let _ = app.unlink(name);
        }
        Ok(app
            .symlink(Path::new(&pid), name)
            .ok()
            .map(|()| Self { app, pid }))
    }
}

impl Drop for Lock<'_> {
    fn drop(&mut self) {
        if Self::holder(self.app).as_deref() == Some(self.pid.as_str()) {
            let _ = self.app.unlink(OsStr::new(LOCK));
        }
    }
}

/// What every run leaves tidy: the scratch folders gone, and the old copy
/// gone once a new one is in place (never the last copy).
fn tidy(app: &Dir) {
    let mut budget = REMOVE_BUDGET;
    let _ = app.remove_tree(OsStr::new(WORK), &mut budget);
    let _ = app.remove_tree(OsStr::new(NEW), &mut budget);
    if app
        .stat_opt(OsStr::new(OUT))
        .ok()
        .flatten()
        .is_some_and(|m| m.is_dir())
    {
        let _ = app.remove_tree(OsStr::new(OLD), &mut budget);
    }
}

/// Whether the copy there was made from this very plan, by root.
fn unchanged(app: &Dir, sum: &str, me: u32) -> bool {
    let Ok(Some(meta)) = app.stat_opt(OsStr::new(OUT)) else {
        return false;
    };
    if check_private(&meta, me, OUT).is_err() {
        return false;
    }
    let Ok(out) = app.open_dir(OsStr::new(OUT)) else {
        return false;
    };
    match out.stat_opt(OsStr::new(".sum")) {
        Ok(Some(m)) if m.is_file() && m.uid == me && m.nlink == 1 && !m.shared_write() => {}
        _ => return false,
    }
    let Ok(mut file) = out.open_read(OsStr::new(".sum")) else {
        return false;
    };
    read_capped(&mut file, 128).is_ok_and(|(bytes, _)| bytes == format!("{sum}\n").as_bytes())
}

/// Copies one planned file under `new`, if what it reads is a small PNG
/// within the copy's total.
fn copy_one(new: &Dir, item: &Item, total: &mut usize) -> bool {
    let Ok(mut file) = open_regular(&item.real) else {
        return false;
    };
    let Ok((bytes, over)) = read_capped(&mut file, MAX_FILE) else {
        return false;
    };
    if over || *total + bytes.len() > MAX_TOTAL || !bytes.starts_with(PNG) {
        return false;
    }
    let Some((file_name, folders)) = item.dest.split_last() else {
        return false;
    };
    let mut held: Option<Dir> = None;
    for name in folders {
        let parent = held.as_ref().unwrap_or(new);
        let name = OsStr::new(name);
        let made = match parent.stat_opt(name) {
            Ok(None) => parent.mkdir(name, 0o755).is_ok(),
            Ok(Some(_)) => false,
            Err(_) => return false,
        };
        let Ok(dir) = parent.open_dir(name) else {
            return false;
        };
        if made && dir.chmod(0o755).is_err() {
            return false;
        }
        held = Some(dir);
    }
    let dir = held.as_ref().unwrap_or(new);
    let name = OsStr::new(file_name.as_str());
    if dir
        .stat_opt(name)
        .ok()
        .flatten()
        .is_some_and(|m| m.is_dir())
    {
        return false;
    }
    let part = OsStr::new(PART);
    let _ = dir.unlink(part);
    let written = dir.create(part, 0o644).and_then(|mut f| {
        f.write_all(&bytes)?;
        f.sync_all()
    });
    if written.is_err() || dir.rename(part, name).is_err() {
        let _ = dir.unlink(part);
        return false;
    }
    *total += bytes.len();
    true
}

fn write_small(dir: &Dir, name: &str, text: &str) -> Result<(), Failure> {
    let mut file = dir.create(OsStr::new(name), 0o644).map_err(io)?;
    file.write_all(text.as_bytes()).map_err(io)?;
    file.sync_all().map_err(io)
}

/// One run: the copy made, found unchanged, or refused, as the shell is
/// told it; the error's text is for the log.
pub fn copy(root: &Root, bus: &dyn Bus) -> Result<Icons, Failure> {
    let me = crate::sys::os::euid();
    if root.is_tv() && me != 0 {
        return Err(folders("not root"));
    }
    let app = pin(root, me).map_err(folders)?;
    let Some(_lock) = Lock::take(&app)? else {
        return Ok(Icons::Busy);
    };
    let result = copy_locked(root, bus, &app, me);
    tidy(&app);
    result
}

fn copy_locked(root: &Root, bus: &dyn Bus, app: &Dir, me: u32) -> Result<Icons, Failure> {
    let (out, old, new) = (OsStr::new(OUT), OsStr::new(OLD), OsStr::new(NEW));
    // A run cut off between the swap's two renames left the copy as OLD.
    if app.stat_opt(out).map_err(io)?.is_none()
        && app.stat_opt(old).map_err(io)?.is_some_and(|m| m.is_dir())
    {
        app.rename(old, out).map_err(io)?;
    }
    tidy(app);
    let (uri, payload) = bus::launch_points();
    let reply = bus::checked(bus, uri, &payload).map_err(|e| (IconsFault::Luna, e))?;
    let (items, mut refused) = make_plan(root, &reply);
    let sum = sum(&items);
    if unchanged(app, &sum, me) {
        return Ok(Icons::Unchanged);
    }
    app.mkdir(new, 0o755).map_err(io)?;
    let fresh = app.open_dir(new).map_err(io)?;
    fresh.chmod(0o755).map_err(io)?;
    let mut copied = 0u32;
    let mut total = 0usize;
    for item in &items {
        if copy_one(&fresh, item, &mut total) {
            copied += 1;
        } else {
            refused += 1;
        }
    }
    let line = format!("lumen-icons: changed, {copied} copied, {refused} refused");
    write_small(&fresh, ".sum", &format!("{sum}\n"))?;
    write_small(&fresh, "status", &format!("{line}\n"))?;
    fresh.sync().map_err(io)?;
    if app.stat_opt(out).map_err(io)?.is_some() {
        app.remove_tree(old, &mut { REMOVE_BUDGET }).map_err(io)?;
        app.rename(out, old).map_err(io)?;
    }
    app.rename(new, out).map_err(io)?;
    app.sync().map_err(io)?;
    Ok(Icons::Changed { copied, refused })
}

/// The undo: every `lg-icons*` entry in Lumen's folder removed. The shell
/// then shows the 26 letters again.
pub fn remove(root: &Root) -> Result<Vec<String>, String> {
    let me = crate::sys::os::euid();
    let app = pin(root, me)?;
    let mut lines = Vec::new();
    for name in [OUT, NEW, OLD, WORK, LOCK] {
        let name = OsStr::new(name);
        if app.stat_opt(name).map_err(|e| e.to_string())?.is_some() {
            app.remove_tree(name, &mut { REMOVE_BUDGET })
                .map_err(|e| e.to_string())?;
            lines.push(format!("icons: removed {}", name.to_string_lossy()));
        }
    }
    if lines.is_empty() {
        lines.push("icons: no copy to remove".to_owned());
    }
    Ok(lines)
}

/// The words for a result, as the script printed it.
pub fn describe(result: &Result<Icons, Failure>) -> String {
    match result {
        Ok(Icons::Changed { copied, refused }) => {
            format!("icons: changed, {copied} copied, {refused} refused")
        }
        Ok(Icons::Unchanged) => "icons: unchanged".to_owned(),
        Ok(Icons::Busy) => "icons: busy (another copy runs)".to_owned(),
        Ok(Icons::Off) => "icons: off".to_owned(),
        Ok(Icons::Failed(f)) | Err((f, _)) => {
            let why = result.as_ref().err().map_or("", |(_, w)| w.as_str());
            format!("icons: FAILED ({f:?}): {why}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bus::fake::FakeBus;
    use serde_json::json;
    use std::os::unix::fs::PermissionsExt;

    struct Tv {
        base: PathBuf,
        root: Root,
        bus: FakeBus,
    }

    impl Tv {
        fn new() -> Self {
            let base = crate::fsat::scratch("icons");
            let root = Root::at(&base);
            let app = root.path(&format!("{DEV_APPS}/usr/palm/applications/{APP_ID}"));
            std::fs::create_dir_all(&app).unwrap();
            for d in ["usr", "usr/palm", "usr/palm/applications"] {
                let p = root.path(&format!("{DEV_APPS}/{d}"));
                std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
            }
            std::fs::set_permissions(&app, std::fs::Permissions::from_mode(0o755)).unwrap();
            std::fs::create_dir_all(root.path(INPUTS)).unwrap();
            let tv = Self {
                base,
                root,
                bus: FakeBus::default(),
            };
            tv.points(&[]);
            tv
        }

        fn app(&self) -> PathBuf {
            self.root
                .path(&format!("{DEV_APPS}/usr/palm/applications/{APP_ID}"))
        }

        fn out(&self) -> PathBuf {
            self.app().join(OUT)
        }

        fn png(&self, abs: &str, body: &str) {
            let p = self.root.path(abs);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            let mut bytes = PNG.to_vec();
            bytes.extend_from_slice(body.as_bytes());
            std::fs::write(p, bytes).unwrap();
        }

        fn points(&self, paths: &[&str]) {
            let points: Vec<Value> = paths
                .iter()
                .map(|p| json!({"id": "x", "icon": p, "largeIcon": "", "params": {}}))
                .collect();
            self.bus.replies.lock().unwrap().clear();
            self.bus.reply(
                bus::LAUNCH_POINTS,
                Ok(json!({"returnValue": true, "launchPoints": points})),
            );
        }

        fn copied(&self) -> Vec<String> {
            fn walk(dir: &Path, base: &Path, out: &mut Vec<String>) {
                for e in std::fs::read_dir(dir).unwrap() {
                    let p = e.unwrap().path();
                    let m = std::fs::symlink_metadata(&p).unwrap();
                    if m.is_dir() {
                        walk(&p, base, out);
                    } else {
                        out.push(p.strip_prefix(base).unwrap().display().to_string());
                    }
                }
            }
            let mut out = Vec::new();
            walk(&self.out(), &self.out(), &mut out);
            out.sort();
            out
        }

        fn run(&self) -> Result<Icons, Failure> {
            copy(&self.root, &self.bus)
        }
    }

    impl Drop for Tv {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.base);
        }
    }

    #[test]
    fn copies_the_icons_the_b9s_launch_points_name_mirrored_by_path() {
        let tv = Tv::new();
        let fixture: Value = serde_json::from_str(include_str!(
            "../../lumen-shell/fixtures/luna/b9-launch-points.json"
        ))
        .unwrap();
        let mut named = BTreeSet::new();
        icon_strings(&fixture, &mut named);
        for p in named.iter().filter(|p| p.starts_with('/')) {
            tv.png(p, p);
        }
        tv.png(&format!("{INPUTS}/hometheater.png"), "h");
        tv.png(&format!("{INPUTS}/pc.png"), "p");
        tv.bus.replies.lock().unwrap().clear();
        tv.bus.reply(bus::LAUNCH_POINTS, Ok(fixture));
        assert_eq!(
            tv.run(),
            Ok(Icons::Changed {
                copied: 10,
                refused: 0
            })
        );
        assert_eq!(
            tv.copied(),
            [
                ".sum",
                "media/cryptofs/apps/usr/palm/applications/cdp-30/Plex_130x130.png",
                "media/cryptofs/apps/usr/palm/applications/cdp-30/Plex_80x80.png",
                "media/cryptofs/apps/usr/palm/applications/netflix/LARGE_APP_ICON.png",
                "media/cryptofs/apps/usr/palm/applications/netflix/SMALL_APP_ICON.png",
                "mnt/otncabi/usr/palm/applications/com.webos.app.discovery/asset/hd1080/lgstore.png",
                "mnt/otncabi/usr/palm/applications/com.webos.app.discovery/asset/hd1080/lgstore_130x130.png",
                "status",
                "usr/palm/applications/com.webos.app.photovideo/icon.png",
                "usr/palm/applications/com.webos.app.photovideo/icon_photovideo.png",
                "usr/palm/plugins/inputapps/assets/small/hometheater.png",
                "usr/palm/plugins/inputapps/assets/small/pc.png",
            ]
        );
        assert_eq!(
            std::fs::read_to_string(tv.out().join("status")).unwrap(),
            "lumen-icons: changed, 10 copied, 0 refused\n"
        );
        let mode = |p: PathBuf| std::fs::metadata(p).unwrap().permissions().mode() & 0o7777;
        assert_eq!(
            mode(
                tv.out()
                    .join("usr/palm/applications/com.webos.app.photovideo/icon.png")
            ),
            0o644
        );
        assert_eq!(
            mode(
                tv.out()
                    .join("usr/palm/applications/com.webos.app.photovideo")
            ),
            0o755
        );
        assert_eq!(mode(tv.out()), 0o755);
        let entries: Vec<String> = std::fs::read_dir(tv.app())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with("lg-icons"))
            .collect();
        assert_eq!(entries, [OUT], "nothing else left in Lumen's folder");
    }

    #[test]
    fn refuses_paths_outside_the_roots_odd_names_links_out_and_anything_not_a_small_png() {
        use std::os::unix::fs::symlink;
        let tv = Tv::new();
        let a = "/usr/palm/applications/a";
        let ap = tv.root.path(a);
        tv.png(&format!("{a}/good.png"), "g");
        tv.png(&format!("{a}/sub/i.png"), "i");
        symlink("sub/i.png", ap.join("inner.png")).unwrap();
        tv.png("/etc/secret.png", "a secret");
        symlink("../../../../etc/secret.png", ap.join("link.png")).unwrap();
        tv.png("/usr/palm/applications/b/i.png", "b");
        symlink("../b/i.png", ap.join("other.png")).unwrap();
        std::fs::write(ap.join("text.png"), "a secret").unwrap();
        tv.png(&format!("{a}/big.png"), &"x".repeat(MAX_FILE + 1));
        std::fs::create_dir_all(ap.join("dir.png")).unwrap();
        tv.png(&format!("{a}/i.jpg"), "j");
        tv.png("/usr/palm/applications/.hidden/i.png", "h");
        tv.png("/usr/palm/applications/a b/i.png", "s");
        tv.points(&[
            "/usr/palm/applications/a/good.png",
            "/usr/palm/applications/a/inner.png",
            "/etc/secret.png",
            "/usr/palm/applications/a/link.png",
            "/usr/palm/applications/a/other.png",
            "/usr/palm/applications/a/text.png",
            "/usr/palm/applications/a/big.png",
            "/usr/palm/applications/a/dir.png",
            "/usr/palm/applications/a/missing.png",
            "/usr/palm/applications/a/i.jpg",
            "/usr/palm/applications/a",
            "/usr/palm/applications/a/../b/i.png",
            "/usr/palm/applications/a/./good.png",
            "/usr/palm/applications/a//good.png",
            "/usr/palm/applications/.hidden/i.png",
            "/usr/palm/applications/a b/i.png",
            "usr/palm/applications/a/good.png",
            "/usr/palm/applicationsX/a/good.png",
            "/usr/palm/applications/a/q\"x.png",
            "/media/developer/apps/usr/palm/applications/x/icon.png",
        ]);
        assert_eq!(
            tv.run(),
            Ok(Icons::Changed {
                copied: 2,
                refused: 17
            })
        );
        assert_eq!(
            tv.copied(),
            [
                ".sum",
                "status",
                "usr/palm/applications/a/good.png",
                "usr/palm/applications/a/inner.png"
            ]
        );
        let all: String = tv
            .copied()
            .iter()
            .map(|f| std::fs::read_to_string(tv.out().join(f)).unwrap_or_default())
            .collect();
        assert!(!all.contains("a secret"));
    }

    #[test]
    fn a_pipe_swapped_in_is_refused_without_hanging() {
        let tv = Tv::new();
        let a = tv.root.path("/usr/palm/applications/a");
        std::fs::create_dir_all(&a).unwrap();
        // Made with mkfifo(1): the tests use no unsafe.
        let fifo = a.join("fifo.png");
        let made = crate::run::spawn(
            Path::new("/bin/sh"),
            &["-c", "mkfifo \"$0\"", fifo.to_str().unwrap()],
            crate::run::RUN_TIMEOUT,
        );
        if !made.is_ok_and(|o| o.success) {
            return;
        }
        let item = Item {
            dest: vec!["f.png".to_owned()],
            real: a.join("fifo.png"),
        };
        let dir = Dir::open_path(&tv.app()).unwrap();
        assert!(!copy_one(&dir, &item, &mut 0));
    }

    #[test]
    fn the_input_icons_are_lgs_own_png_files_nothing_else() {
        let tv = Tv::new();
        tv.png(&format!("{INPUTS}/HDMI_4.png"), "4");
        std::fs::write(tv.root.path(&format!("{INPUTS}/broken.png")), "not a png").unwrap();
        std::fs::write(tv.root.path(&format!("{INPUTS}/readme.txt")), "notes").unwrap();
        tv.png("/etc/secret.png", "a secret");
        std::os::unix::fs::symlink(
            "../../../../../../etc/secret.png",
            tv.root.path(&format!("{INPUTS}/away.png")),
        )
        .unwrap();
        assert_eq!(
            tv.run(),
            Ok(Icons::Changed {
                copied: 1,
                refused: 2
            })
        );
        assert_eq!(
            tv.copied(),
            [
                ".sum",
                "status",
                "usr/palm/plugins/inputapps/assets/small/HDMI_4.png"
            ]
        );
    }

    #[test]
    fn lg_homes_icon_is_copied_though_no_launch_point_names_it_and_only_once() {
        let tv = Tv::new();
        tv.png(LG_HOME_ICON, "LG Home");
        assert_eq!(
            tv.run(),
            Ok(Icons::Changed {
                copied: 1,
                refused: 0
            })
        );
        // A launch point naming it too gives the same plan: one copy.
        tv.points(&[LG_HOME_ICON]);
        assert_eq!(tv.run(), Ok(Icons::Unchanged));
        assert_eq!(
            tv.copied(),
            [
                ".sum",
                "status",
                "usr/palm/applications/com.webos.app.home/assets/icon.png"
            ]
        );
    }

    #[test]
    fn a_second_run_with_nothing_changed_leaves_the_copy_alone_and_a_change_redoes_it() {
        let tv = Tv::new();
        tv.png("/usr/palm/applications/a/i.png", "old");
        tv.points(&["/usr/palm/applications/a/i.png"]);
        assert!(matches!(tv.run(), Ok(Icons::Changed { copied: 1, .. })));
        assert_eq!(tv.run(), Ok(Icons::Unchanged));
        tv.png("/usr/palm/applications/a/i.png", "a new icon, longer");
        assert!(matches!(tv.run(), Ok(Icons::Changed { copied: 1, .. })));
        tv.points(&[]);
        assert!(matches!(tv.run(), Ok(Icons::Changed { copied: 0, .. })));
        assert_eq!(tv.copied(), [".sum", "status"]);
    }

    #[test]
    fn a_sum_that_isnt_roots_alone_is_not_trusted() {
        let tv = Tv::new();
        tv.png("/usr/palm/applications/a/i.png", "x");
        tv.points(&["/usr/palm/applications/a/i.png"]);
        tv.run().unwrap();
        std::fs::set_permissions(tv.out(), std::fs::Permissions::from_mode(0o777)).unwrap();
        assert!(matches!(tv.run(), Ok(Icons::Changed { .. })));
    }

    #[test]
    fn a_run_cut_off_mid_swap_is_put_right_and_never_loses_the_last_copy() {
        let tv = Tv::new();
        tv.png("/usr/palm/applications/a/i.png", "x");
        tv.points(&["/usr/palm/applications/a/i.png"]);
        tv.run().unwrap();
        std::fs::rename(tv.out(), tv.app().join(OLD)).unwrap();
        std::fs::create_dir_all(tv.app().join(NEW).join("half")).unwrap();
        assert_eq!(tv.run(), Ok(Icons::Unchanged));
        assert!(tv.out().join(".sum").exists());
        assert!(!tv.app().join(OLD).exists() && !tv.app().join(NEW).exists());
    }

    #[test]
    fn each_folder_the_ipk_sets_is_refused_if_others_can_write_it() {
        let own = format!("usr/palm/applications/{APP_ID}");
        for d in ["usr", "usr/palm", "usr/palm/applications", own.as_str()] {
            let tv = Tv::new();
            let p = tv.root.path(&format!("{DEV_APPS}/{d}"));
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o777)).unwrap();
            let err = tv.run().unwrap_err();
            assert_eq!(err.0, IconsFault::Folders, "{d}");
            assert!(err.1.contains("writable by others"), "{}", err.1);
        }
        // LG's own two 0777 developer folders are accepted.
        let tv = Tv::new();
        for d in ["/media/developer", DEV_APPS] {
            std::fs::set_permissions(tv.root.path(d), std::fs::Permissions::from_mode(0o777))
                .unwrap();
        }
        assert!(tv.run().is_ok());
    }

    #[test]
    fn a_link_on_the_way_to_lumens_folder_is_refused() {
        let tv = Tv::new();
        let apps = tv.root.path(&format!("{DEV_APPS}/usr/palm/applications"));
        std::fs::rename(apps.join(APP_ID), apps.join("elsewhere")).unwrap();
        std::os::unix::fs::symlink("elsewhere", apps.join(APP_ID)).unwrap();
        assert_eq!(tv.run().unwrap_err().0, IconsFault::Folders);
    }

    #[test]
    fn a_failed_or_refused_luna_call_keeps_the_copy_there_was() {
        let tv = Tv::new();
        tv.png("/usr/palm/applications/a/i.png", "x");
        tv.points(&["/usr/palm/applications/a/i.png"]);
        tv.run().unwrap();
        tv.bus.replies.lock().unwrap().clear();
        tv.bus
            .reply(bus::LAUNCH_POINTS, Ok(json!({"returnValue": false})));
        assert_eq!(tv.run().unwrap_err().0, IconsFault::Luna);
        tv.bus.replies.lock().unwrap().clear();
        tv.bus.reply(bus::LAUNCH_POINTS, Err("no reply".to_owned()));
        assert_eq!(tv.run().unwrap_err().0, IconsFault::Luna);
        assert!(tv.out().join("usr/palm/applications/a/i.png").exists());
    }

    #[test]
    #[allow(clippy::disallowed_methods)] // a child that outlives one run
    fn a_live_lock_makes_a_run_wait_its_turn_and_a_stale_one_is_taken_over() {
        let tv = Tv::new();
        // This test's own pid: its command line names neither program, so the
        // lock is stale.
        std::os::unix::fs::symlink(std::process::id().to_string(), tv.app().join(LOCK)).unwrap();
        assert!(tv.run().is_ok());
        assert!(
            std::fs::symlink_metadata(tv.app().join(LOCK)).is_err(),
            "released"
        );
        // A pid that is a run of the script: a child whose command line names it.
        let Ok(mut child) = std::process::Command::new("/bin/sh")
            .args(["-c", "sleep 5", "lumen-icons.sh"])
            .spawn()
        else {
            return;
        };
        std::os::unix::fs::symlink(child.id().to_string(), tv.app().join(LOCK)).unwrap();
        assert_eq!(tv.run(), Ok(Icons::Busy));
        let _ = child.kill();
        let _ = child.wait();
        assert!(
            std::fs::symlink_metadata(tv.app().join(LOCK)).is_ok(),
            "not ours to remove"
        );
    }

    #[test]
    fn at_most_128_app_icons_and_8_mb_in_all() {
        let tv = Tv::new();
        let mut paths = Vec::new();
        for n in 0..130 {
            let p = format!("/usr/palm/applications/app{n:03}/i.png");
            tv.png(&p, "x");
            paths.push(p);
        }
        let refs: Vec<&str> = paths.iter().map(String::as_str).collect();
        tv.points(&refs);
        assert_eq!(
            tv.run(),
            Ok(Icons::Changed {
                copied: 128,
                refused: 2
            })
        );
        let tv = Tv::new();
        let mut paths = Vec::new();
        for n in 0..9 {
            let p = format!("/usr/palm/applications/big{n}/i.png");
            tv.png(&p, &"x".repeat(MAX_FILE - 8));
            paths.push(p);
        }
        let refs: Vec<&str> = paths.iter().map(String::as_str).collect();
        tv.points(&refs);
        assert_eq!(
            tv.run(),
            Ok(Icons::Changed {
                copied: 8,
                refused: 1
            })
        );
    }

    #[test]
    fn the_undo_removes_every_lg_icons_entry() {
        let tv = Tv::new();
        tv.png("/usr/palm/applications/a/i.png", "x");
        tv.points(&["/usr/palm/applications/a/i.png"]);
        tv.run().unwrap();
        std::fs::create_dir_all(tv.app().join(OLD)).unwrap();
        let lines = remove(&tv.root).unwrap();
        assert_eq!(lines.len(), 2, "{lines:?}");
        assert_eq!(std::fs::read_dir(tv.app()).unwrap().count(), 0);
        assert_eq!(remove(&tv.root).unwrap(), ["icons: no copy to remove"]);
    }

    #[test]
    fn names_and_relative_paths() {
        assert!(name_ok("com.webos.app.home") && !name_ok("a b") && !name_ok(".."));
        assert!(rel_ok("assets/icon.png") && rel_ok("A.PNG"));
        for bad in [
            "",
            "/a.png",
            "a/",
            "a//b.png",
            "./a.png",
            "a/../b.png",
            "a.jpg",
            "a\\b.png",
        ] {
            assert!(!rel_ok(bad), "{bad:?}");
        }
    }
}
