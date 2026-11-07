//! The shell's link to lumend (decision 14): a Unix socket lumend makes from
//! the host side inside the shell's own jail, never in `/tmp`.
//!
//! On the B9 the shell runs chrooted in `/var/palm/jail/<id>`, which its
//! jail uid owns, and that jail's `var` is a folder of root's (b9.md,
//! "Platform"). Its `/var/run` and `/tmp` are the host's, shared with every
//! app, so they are out. lumend makes `var/lumen/lumend.sock` there: the
//! shell sees it as `/var/lumen/lumend.sock`.
//!
//! * The walk from `/var/palm/jail` is one name at a time with no link
//!   followed. `/var/palm/jail` must be root's alone (or another app could
//!   swap Lumen's jail), the jail's owner is the shell's uid, and `var` must
//!   be root's alone. The shell owns its jail's root, so it could rename
//!   `var`; the checks then refuse whatever stands in its place.
//! * `var/lumen` is root's, 0700 while the socket is made, then 0711: the
//!   shell may pass through but not list or change it.
//! * The socket is the shell's uid's, 0600. Each connection's peer uid is
//!   read from the kernel (`SO_PEERCRED`) and anything but the shell's uid
//!   is closed at once; the shell checks that lumend's end is root
//!   (`lumend_proto::unix::connect_checked`).
//! * One connection at a time: a new one from the shell (a restart)
//!   replaces the old. Lines are capped and checked by `lumend_proto`.
//!
//! Whether the shell can reach the socket there is fact 9 (depth.md), which
//! `lumend try link` answers without the shell: a child of lumend enters
//! the jail as the shell's uid and connects as the shell would.

use std::ffi::OsStr;
use std::io::{BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use lumend_proto::{Reply, Request, SOCKET_FOLDER, SOCKET_IN_JAIL, SOCKET_NAME, VERSION};

use crate::fsat::{Dir, check_private};
use crate::paths::{APP_ID, JAILS, Root};

/// How long a write to the shell may take before the link is dropped.
const WRITE_TIMEOUT: Duration = Duration::from_secs(2);
/// Connections accepted in any ten seconds before the accept loop slows.
const MAX_ACCEPTS: usize = 10;

/// What the accept and reader threads tell the daemon.
#[derive(Debug)]
pub enum LinkEvent {
    Up(u64),
    Request(u64, Request),
    Down(u64, String),
}

/// The socket, made.
pub struct JailSocket {
    pub listener: Arc<UnixListener>,
    pub shell_uid: u32,
    pub shell_gid: u32,
    /// The folder and the socket's file, to tell later whether they are
    /// still the ones made.
    ids: ((u64, u64), (u64, u64)),
}

/// Lumen's jail: its root, held open, and its owner.
fn jail(root: &Root, me: u32) -> Result<(Dir, u32, u32), String> {
    let jails = Dir::open_path(&root.path(JAILS)).map_err(|e| format!("{JAILS}: {e}"))?;
    check_private(&jails.meta().map_err(|e| e.to_string())?, me, JAILS)?;
    let jail = jails
        .open_dir(OsStr::new(APP_ID))
        .map_err(|e| format!("Lumen's jail: {e}"))?;
    let m = jail.meta().map_err(|e| e.to_string())?;
    if m.uid == 0 || m.uid == me {
        return Err(format!(
            "Lumen's jail is owned by uid {}, not a jail uid",
            m.uid
        ));
    }
    Ok((jail, m.uid, m.gid))
}

/// The jail's `var`, checked.
fn jail_var(jail: &Dir, me: u32) -> Result<Dir, String> {
    let var = jail
        .open_dir(OsStr::new(SOCKET_FOLDER[0]))
        .map_err(|e| format!("the jail's var: {e}"))?;
    check_private(
        &var.meta().map_err(|e| e.to_string())?,
        me,
        "the jail's var",
    )?;
    Ok(var)
}

/// Makes the socket in Lumen's jail, replacing one an earlier run left.
pub fn make(root: &Root) -> Result<JailSocket, String> {
    let me = crate::sys::os::euid();
    let (jail, shell_uid, shell_gid) = jail(root, me)?;
    let var = jail_var(&jail, me)?;
    let folder = OsStr::new(SOCKET_FOLDER[1]);
    match var.stat_opt(folder).map_err(|e| e.to_string())? {
        None => var
            .mkdir(folder, 0o700)
            .map_err(|e| format!("making the jail's var/lumen: {e}"))?,
        Some(m) if m.is_dir() && m.uid == me => {}
        Some(_) => return Err("the jail's var/lumen is not a folder of root's".to_owned()),
    }
    let dir = var
        .open_dir(folder)
        .map_err(|e| format!("the jail's var/lumen: {e}"))?;
    // Closed to everyone while the socket is made and set.
    dir.chmod(0o700).map_err(|e| e.to_string())?;
    let name = OsStr::new(SOCKET_NAME);
    match dir.stat_opt(name).map_err(|e| e.to_string())? {
        Some(m) if m.is_dir() => return Err("a folder where the socket goes".to_owned()),
        Some(_) => dir
            .unlink(name)
            .map_err(|e| format!("the old socket: {e}"))?,
        None => {}
    }
    let at = dir.at(name).map_err(|e| e.to_string())?;
    let listener = UnixListener::bind(&at).map_err(|e| format!("binding the socket: {e}"))?;
    dir.lchown(name, shell_uid, 0)
        .map_err(|e| format!("giving the socket to the shell: {e}"))?;
    std::fs::set_permissions(&at, std::fs::Permissions::from_mode(0o600))
        .map_err(|e| e.to_string())?;
    let socket = dir.stat(name).map_err(|e| e.to_string())?;
    if !socket.is_socket() {
        return Err("what was bound is not a socket".to_owned());
    }
    dir.chmod(0o711).map_err(|e| e.to_string())?;
    let folder_id = dir.meta().map_err(|e| e.to_string())?.id();
    Ok(JailSocket {
        listener: Arc::new(listener),
        shell_uid,
        shell_gid,
        ids: (folder_id, socket.id()),
    })
}

impl JailSocket {
    /// Whether the jail still shows this socket where the shell looks (a
    /// reinstall of Lumen may have made a new jail).
    pub fn still_there(&self, root: &Root) -> bool {
        let me = crate::sys::os::euid();
        let check = || -> Result<bool, String> {
            let (jail, uid, _) = jail(root, me)?;
            let dir = jail_var(&jail, me)?
                .open_dir(OsStr::new(SOCKET_FOLDER[1]))
                .map_err(|e| e.to_string())?;
            let socket = dir
                .stat(OsStr::new(SOCKET_NAME))
                .map_err(|e| e.to_string())?;
            Ok(uid == self.shell_uid
                && dir.meta().map_err(|e| e.to_string())?.id() == self.ids.0
                && socket.id() == self.ids.1)
        };
        check().unwrap_or(false)
    }
}

/// The undo: the socket and its folder out of the jail.
pub fn remove(root: &Root) -> Result<Vec<String>, String> {
    let me = crate::sys::os::euid();
    let (jail, _, _) = jail(root, me)?;
    let var = jail_var(&jail, me)?;
    let folder = OsStr::new(SOCKET_FOLDER[1]);
    if var.stat_opt(folder).map_err(|e| e.to_string())?.is_none() {
        return Ok(vec!["link: no socket in the jail".to_owned()]);
    }
    var.remove_tree(folder, &mut 16)
        .map_err(|e| format!("removing the jail's var/lumen: {e}"))?;
    Ok(vec![
        "link: removed the jail's var/lumen and its socket".to_owned(),
    ])
}

/// The connection lumend answers on, shared with the threads that read it.
#[derive(Default)]
pub struct Conn {
    slot: Mutex<Option<(u64, UnixStream)>>,
}

impl Conn {
    /// Sends one line to connection `id` if it is still the current one;
    /// a failed write drops it.
    pub fn send(&self, id: u64, reply: &Reply) {
        let mut slot = self
            .slot
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let failed = match slot.as_mut() {
            Some((current, stream)) if *current == id => {
                stream.write_all(reply.line().as_bytes()).is_err()
            }
            _ => false,
        };
        if failed && let Some((_, stream)) = slot.take() {
            let _ = stream.shutdown(std::net::Shutdown::Both);
        }
    }

    /// Sends to whichever connection is current, if any (events).
    pub fn send_current(&self, reply: &Reply) {
        let id = self.current();
        if let Some(id) = id {
            self.send(id, reply);
        }
    }

    pub fn current(&self) -> Option<u64> {
        self.slot
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
            .map(|(id, _)| *id)
    }

    /// Closes connection `id`.
    pub fn drop_conn(&self, id: u64) {
        let mut slot = self
            .slot
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if slot.as_ref().is_some_and(|(current, _)| *current == id)
            && let Some((_, stream)) = slot.take()
        {
            let _ = stream.shutdown(std::net::Shutdown::Both);
        }
    }

    fn replace(&self, id: u64, stream: UnixStream) {
        let mut slot = self
            .slot
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some((_, old)) = slot.replace((id, stream)) {
            let _ = old.shutdown(std::net::Shutdown::Both);
        }
    }
}

static NEXT_CONN: AtomicU64 = AtomicU64::new(1);

/// The accept loop, on a thread of its own until `stop` is set and the
/// listener shut down.
pub fn serve<E: From<LinkEvent> + Send + 'static>(
    listener: Arc<UnixListener>,
    shell_uid: u32,
    conn: Arc<Conn>,
    events: Sender<E>,
    stop: Arc<AtomicBool>,
) {
    let mut recent: Vec<Instant> = Vec::new();
    loop {
        let accepted = listener.accept();
        if stop.load(Ordering::SeqCst) {
            return;
        }
        let Ok((stream, _)) = accepted else {
            std::thread::sleep(Duration::from_millis(200));
            continue;
        };
        recent.retain(|t| t.elapsed() < Duration::from_secs(10));
        recent.push(Instant::now());
        if recent.len() > MAX_ACCEPTS {
            std::thread::sleep(Duration::from_secs(1));
        }
        match lumend_proto::unix::peer_uid(&stream) {
            Ok(uid) if uid == shell_uid => {}
            Ok(uid) => {
                crate::say!("link: closed a connection from uid {uid}, not the shell's");
                continue;
            }
            Err(e) => {
                crate::say!("link: closed a connection whose uid is unreadable: {e}");
                continue;
            }
        }
        let Ok(reader) = stream.try_clone() else {
            continue;
        };
        if stream.set_write_timeout(Some(WRITE_TIMEOUT)).is_err() {
            continue;
        }
        let id = NEXT_CONN.fetch_add(1, Ordering::SeqCst);
        conn.replace(id, stream);
        let events_for_reader = events.clone();
        let spawned = std::thread::Builder::new()
            .name("link-read".to_owned())
            .stack_size(64 << 10)
            .spawn(move || read_loop(id, reader, &events_for_reader));
        if spawned.is_err() {
            conn.drop_conn(id);
            continue;
        }
        if events.send(LinkEvent::Up(id).into()).is_err() {
            return;
        }
    }
}

fn read_loop<E: From<LinkEvent>>(id: u64, stream: UnixStream, events: &Sender<E>) {
    let mut reader = BufReader::new(stream);
    let why = loop {
        match lumend_proto::read_line(&mut reader) {
            Ok(Some(line)) => match Request::parse(&line) {
                Ok(req) => {
                    if events.send(LinkEvent::Request(id, req).into()).is_err() {
                        return;
                    }
                }
                Err(e) => break format!("a line that is no request ({e})"),
            },
            Ok(None) => break "the shell closed it".to_owned(),
            Err(e) if e.kind() == std::io::ErrorKind::InvalidData => {
                break format!("a line that is no request ({e})");
            }
            Err(e) => break format!("{e}"),
        }
    };
    let _ = events.send(LinkEvent::Down(id, why).into());
}

/// `lumend try link`: makes the socket, then has a child of lumend enter
/// Lumen's jail as the shell's uid and connect as the shell would; both ends
/// check the other's uid and exchange `hello`. The socket is left in place.
pub fn probe(root: &Root, exe: &Path) -> Vec<String> {
    let mut lines = Vec::new();
    let sock = match make(root) {
        Ok(sock) => sock,
        Err(e) => return vec![format!("link: FAILED to make the socket: {e}")],
    };
    lines.push(format!(
        "link: made the jail's var/lumen/lumend.sock for uid {}",
        sock.shell_uid
    ));
    let jail_root = root.path(&format!("{JAILS}/{APP_ID}"));
    let args = [
        "link-probe".to_owned(),
        jail_root.display().to_string(),
        sock.shell_uid.to_string(),
        sock.shell_gid.to_string(),
    ];
    let exe = exe.to_path_buf();
    let child = std::thread::spawn(move || {
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        crate::run::spawn(&exe, &args, Duration::from_secs(10))
    });
    if sock.listener.set_nonblocking(true).is_err() {
        lines.push("link: FAILED to wait for the child".to_owned());
    }
    let deadline = Instant::now() + Duration::from_secs(8);
    let accepted = loop {
        match sock.listener.accept() {
            Ok((stream, _)) => break Some(stream),
            Err(_) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            Err(_) => break None,
        }
    };
    match accepted {
        None => lines.push("link: no connection came in 8 s".to_owned()),
        Some(stream) => {
            let _ = stream.set_nonblocking(false);
            let _ = stream.set_read_timeout(Some(Duration::from_secs(3)));
            match lumend_proto::unix::peer_uid(&stream) {
                Ok(uid) if uid == sock.shell_uid => {
                    lines.push(format!("link: the connection's uid is {uid}, the shell's"));
                }
                Ok(uid) => lines.push(format!("link: REFUSED: the connection's uid is {uid}")),
                Err(e) => lines.push(format!("link: the connection's uid is unreadable: {e}")),
            }
            let mut reader = BufReader::new(&stream);
            match lumend_proto::read_line(&mut reader) {
                Ok(Some(line)) => {
                    lines.push(format!("link: it said: {line}"));
                    let _ =
                        (&stream).write_all(Reply::Hello { version: VERSION }.line().as_bytes());
                }
                other => lines.push(format!("link: it said nothing readable: {other:?}")),
            }
        }
    }
    match child.join() {
        Ok(Ok(out)) => {
            for line in out.stdout.lines().chain(out.stderr.lines()) {
                lines.push(format!("link: child: {}", crate::log::clean(line)));
            }
        }
        Ok(Err(e)) => lines.push(format!("link: the child failed: {e}")),
        Err(_) => lines.push("link: the child's thread failed".to_owned()),
    }
    lines
}

/// The child's side of `probe`, `lumend link-probe JAIL UID GID`: into the
/// jail, root given up for good, then the shell's own connect.
pub fn probe_child(jail_root: &Path, uid: u32, gid: u32) -> Result<String, String> {
    std::os::unix::fs::chroot(jail_root).map_err(|e| format!("chroot: {e}"))?;
    std::env::set_current_dir("/").map_err(|e| format!("chdir: {e}"))?;
    crate::sys::os::drop_to(uid, gid).map_err(|e| format!("giving up root: {e}"))?;
    let stream =
        lumend_proto::unix::connect_checked(Path::new(SOCKET_IN_JAIL), 0, Duration::from_secs(3))
            .map_err(|e| format!("connecting to {SOCKET_IN_JAIL} as uid {uid}: {e}"))?;
    (&stream)
        .write_all(Request::Hello { version: VERSION }.line().as_bytes())
        .map_err(|e| e.to_string())?;
    let mut reader = BufReader::new(&stream);
    let answer = lumend_proto::read_line(&mut reader)
        .map_err(|e| e.to_string())?
        .unwrap_or_default();
    Ok(format!(
        "connected to {SOCKET_IN_JAIL} as uid {uid} from inside the jail; lumend's end is root; it answered: {answer}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    /// A jail like the B9's, owned by `uid`.
    fn jail_tree(uid: u32) -> (std::path::PathBuf, Root) {
        let base = crate::fsat::scratch("link");
        let root = Root::at(&base);
        let jail = root.path(&format!("{JAILS}/{APP_ID}"));
        std::fs::create_dir_all(jail.join("var")).unwrap();
        std::fs::set_permissions(jail.join("var"), std::fs::Permissions::from_mode(0o555)).unwrap();
        std::fs::set_permissions(root.path(JAILS), std::fs::Permissions::from_mode(0o755)).unwrap();
        std::os::unix::fs::chown(&jail, Some(uid), Some(5000)).unwrap();
        (base, root)
    }

    fn as_root() -> bool {
        crate::sys::os::euid() == 0
    }

    #[test]
    fn makes_the_socket_for_the_jails_uid_only() {
        if !as_root() {
            return;
        }
        let (base, root) = jail_tree(5727);
        let sock = make(&root).unwrap();
        assert_eq!(sock.shell_uid, 5727);
        let dir = root.path(&format!("{JAILS}/{APP_ID}/var/lumen"));
        let m = std::fs::symlink_metadata(&dir).unwrap();
        assert_eq!(m.permissions().mode() & 0o7777, 0o711);
        let s = std::fs::symlink_metadata(dir.join(SOCKET_NAME)).unwrap();
        assert_eq!(s.permissions().mode() & 0o7777, 0o600);
        assert_eq!(std::os::unix::fs::MetadataExt::uid(&s), 5727);
        assert!(sock.still_there(&root));
        // Made again over the old one.
        let again = make(&root).unwrap();
        assert!(!sock.still_there(&root) || sock.ids.1 == again.ids.1);
        assert_eq!(remove(&root).unwrap().len(), 1);
        assert!(!dir.exists());
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn refuses_a_jail_others_could_swap_or_a_var_the_shell_made() {
        if !as_root() {
            return;
        }
        let (base, root) = jail_tree(5727);
        std::fs::set_permissions(root.path(JAILS), std::fs::Permissions::from_mode(0o777)).unwrap();
        assert!(make(&root).err().unwrap().contains("writable by others"));
        std::fs::set_permissions(root.path(JAILS), std::fs::Permissions::from_mode(0o755)).unwrap();
        // The shell renamed var and made its own.
        let jail = root.path(&format!("{JAILS}/{APP_ID}"));
        std::fs::rename(jail.join("var"), jail.join("var-was")).unwrap();
        std::fs::create_dir(jail.join("var")).unwrap();
        std::os::unix::fs::chown(jail.join("var"), Some(5727), Some(5000)).unwrap();
        assert!(make(&root).err().unwrap().contains("owned by uid 5727"));
        // Or a link to somewhere of root's.
        std::fs::remove_dir(jail.join("var")).unwrap();
        std::os::unix::fs::symlink("/etc", jail.join("var")).unwrap();
        assert!(make(&root).is_err());
        assert!(!Path::new("/etc/lumen").exists());
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn refuses_a_jail_owned_by_root() {
        if !as_root() {
            return;
        }
        let (base, root) = jail_tree(0);
        assert!(make(&root).err().unwrap().contains("not a jail uid"));
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn closes_connections_from_other_uids_and_reads_requests_from_the_shells() {
        let base = crate::fsat::scratch("link-serve");
        let path = base.join("s.sock");
        let listener = Arc::new(UnixListener::bind(&path).unwrap());
        let me = crate::sys::os::euid();
        let conn = Arc::new(Conn::default());
        let (tx, rx) = std::sync::mpsc::channel::<LinkEvent>();
        let stop = Arc::new(AtomicBool::new(false));
        {
            let (l, c, s) = (Arc::clone(&listener), Arc::clone(&conn), Arc::clone(&stop));
            std::thread::spawn(move || serve(l, me.wrapping_add(1), c, tx, s));
        }
        // Not the shell's uid: closed, and no event.
        let mut wrong = UnixStream::connect(&path).unwrap();
        let _ = wrong.write_all(b"hello 1\n");
        assert!(rx.recv_timeout(Duration::from_millis(300)).is_err());
        drop(wrong);
        stop.store(true, Ordering::SeqCst);
        crate::sys::os::shutdown_listener(&listener);
        std::fs::remove_dir_all(&base).unwrap();

        // The right uid: requests arrive, a bad line ends it.
        let base = crate::fsat::scratch("link-serve2");
        let path = base.join("s.sock");
        let listener = Arc::new(UnixListener::bind(&path).unwrap());
        let conn = Arc::new(Conn::default());
        let (tx, rx) = std::sync::mpsc::channel::<LinkEvent>();
        {
            let (l, c) = (Arc::clone(&listener), Arc::clone(&conn));
            std::thread::spawn(move || serve(l, me, c, tx, Arc::new(AtomicBool::new(false))));
        }
        let shell = lumend_proto::unix::connect_checked(&path, me, Duration::from_secs(1)).unwrap();
        (&shell)
            .write_all(b"hello 1\nstatus\nicons /etc\n")
            .unwrap();
        let id = match rx.recv_timeout(Duration::from_secs(2)).unwrap() {
            LinkEvent::Up(id) => id,
            other => panic!("{other:?}"),
        };
        let mut got = Vec::new();
        for _ in 0..3 {
            got.push(rx.recv_timeout(Duration::from_secs(2)).unwrap());
        }
        assert!(matches!(got[0], LinkEvent::Request(i, Request::Hello { version: 1 }) if i == id));
        assert!(matches!(got[1], LinkEvent::Request(_, Request::Status)));
        assert!(matches!(&got[2], LinkEvent::Down(_, why) if why.contains("no request")));
        conn.send(id, &Reply::Hello { version: 1 });
        std::fs::remove_dir_all(&base).unwrap();
    }
}
