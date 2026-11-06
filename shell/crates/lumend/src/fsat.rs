//! Files relative to a folder `lumend` holds open, never by a path that
//! someone else could change under it.
//!
//! LG's developer mode makes `/media/developer` 0777 at every boot, the
//! shell owns its jail's root, and root follows links like anyone else. So
//! every folder `lumend` works in is opened one name at a time from `/`,
//! with `O_NOFOLLOW` at each step, and kept open as a [`Dir`]. Everything
//! after that names one entry of that open folder, through
//! `/proc/self/fd/<fd>/<name>`: the kernel resolves the fd to the folder it
//! holds, whatever was renamed since, and the entry's own name is looked up
//! without following a link (`O_NOFOLLOW`, `lstat`, `unlink`, `rename`).
//! It is plain `std`, so no `unsafe`.

use std::ffi::{OsStr, OsString};
use std::fs::{self, File, OpenOptions, Permissions};
use std::io::{self, Read};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Component, Path, PathBuf};

/// What `lumend` keeps of an entry's `lstat`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Meta {
    /// The whole `st_mode`: type and permission bits.
    pub mode: u32,
    pub uid: u32,
    pub gid: u32,
    pub nlink: u64,
    pub dev: u64,
    pub ino: u64,
    pub size: u64,
    pub mtime: i64,
    pub mtime_nsec: i64,
}

const S_IFMT: u32 = 0o170_000;
const S_IFDIR: u32 = 0o040_000;
const S_IFREG: u32 = 0o100_000;
const S_IFLNK: u32 = 0o120_000;
const S_IFSOCK: u32 = 0o140_000;

impl Meta {
    fn of(m: &fs::Metadata) -> Self {
        Self {
            mode: m.mode(),
            uid: m.uid(),
            gid: m.gid(),
            nlink: m.nlink(),
            dev: m.dev(),
            ino: m.ino(),
            size: m.size(),
            mtime: m.mtime(),
            mtime_nsec: m.mtime_nsec(),
        }
    }

    pub fn is_dir(&self) -> bool {
        self.mode & S_IFMT == S_IFDIR
    }

    pub fn is_file(&self) -> bool {
        self.mode & S_IFMT == S_IFREG
    }

    pub fn is_link(&self) -> bool {
        self.mode & S_IFMT == S_IFLNK
    }

    pub fn is_socket(&self) -> bool {
        self.mode & S_IFMT == S_IFSOCK
    }

    /// The permission bits, set-id and sticky included.
    pub fn perm(&self) -> u32 {
        self.mode & 0o7777
    }

    /// The same file: device and inode.
    pub fn id(&self) -> (u64, u64) {
        (self.dev, self.ino)
    }

    /// Writable by its group or by others.
    pub fn shared_write(&self) -> bool {
        self.mode & 0o022 != 0
    }
}

/// One name inside a folder: no `/`, no NUL, not `.` or `..`, not empty.
pub fn name_ok(name: &OsStr) -> bool {
    let b = name.as_encoded_bytes();
    !b.is_empty() && b != b"." && b != b".." && !b.contains(&b'/') && !b.contains(&0)
}

fn check_name(name: &OsStr) -> io::Result<()> {
    if name_ok(name) {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("not a single name: {}", name.to_string_lossy()),
        ))
    }
}

/// A folder held open.
#[derive(Debug)]
pub struct Dir {
    file: File,
}

fn open_dir_at(path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
}

impl Dir {
    /// Opens the folder at the absolute `path`, one name at a time from
    /// `/`, refusing a link anywhere on the way.
    pub fn open_path(path: &Path) -> io::Result<Self> {
        if !path.is_absolute() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "not an absolute path",
            ));
        }
        let mut dir = Self {
            file: open_dir_at(Path::new("/"))?,
        };
        for part in path.components() {
            match part {
                Component::RootDir => {}
                Component::Normal(name) => dir = dir.open_dir(name)?,
                _ => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "a path with . or ..",
                    ));
                }
            }
        }
        Ok(dir)
    }

    /// `/proc/self/fd/<fd>/<name>`: `name` in this very folder.
    pub fn at(&self, name: &OsStr) -> io::Result<PathBuf> {
        check_name(name)?;
        Ok(self.proc_path().join(name))
    }

    /// `/proc/self/fd/<fd>`: this very folder.
    pub fn proc_path(&self) -> PathBuf {
        PathBuf::from(format!("/proc/self/fd/{}", self.file.as_raw_fd()))
    }

    /// The folder's own `fstat`.
    pub fn meta(&self) -> io::Result<Meta> {
        Ok(Meta::of(&self.file.metadata()?))
    }

    /// An entry's `lstat`.
    pub fn stat(&self, name: &OsStr) -> io::Result<Meta> {
        Ok(Meta::of(&fs::symlink_metadata(self.at(name)?)?))
    }

    /// An entry's `lstat`, `None` when there is none.
    pub fn stat_opt(&self, name: &OsStr) -> io::Result<Option<Meta>> {
        match self.stat(name) {
            Ok(m) => Ok(Some(m)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// The sub-folder `name`, refused if it is a link or not a folder.
    pub fn open_dir(&self, name: &OsStr) -> io::Result<Self> {
        Ok(Self {
            file: open_dir_at(&self.at(name)?)?,
        })
    }

    /// Makes the sub-folder `name` with `mode` (the umask may take bits
    /// away; [`Dir::chmod`] on the opened folder sets them exactly).
    pub fn mkdir(&self, name: &OsStr, mode: u32) -> io::Result<()> {
        fs::DirBuilder::new().mode(mode).create(self.at(name)?)
    }

    /// Opens the regular file `name` to read: never through a link, never
    /// waiting on a pipe, and refused unless it is a regular file.
    pub fn open_read(&self, name: &OsStr) -> io::Result<File> {
        open_regular(&self.at(name)?)
    }

    /// Opens the regular file `name` to read and write, as `open_read`.
    pub fn open_rw(&self, name: &OsStr) -> io::Result<File> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)
            .open(self.at(name)?)?;
        if !file.metadata()?.is_file() {
            return Err(io::Error::other("not a regular file"));
        }
        Ok(file)
    }

    /// Makes the file `name`, which must not exist, to write, with `mode`
    /// set exactly.
    pub fn create(&self, name: &OsStr, mode: u32) -> io::Result<File> {
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .mode(mode)
            .open(self.at(name)?)?;
        file.set_permissions(Permissions::from_mode(mode))?;
        Ok(file)
    }

    pub fn unlink(&self, name: &OsStr) -> io::Result<()> {
        fs::remove_file(self.at(name)?)
    }

    pub fn rmdir(&self, name: &OsStr) -> io::Result<()> {
        fs::remove_dir(self.at(name)?)
    }

    /// Renames `from` to `to`, both in this folder; `to` is replaced if it
    /// is a file.
    pub fn rename(&self, from: &OsStr, to: &OsStr) -> io::Result<()> {
        fs::rename(self.at(from)?, self.at(to)?)
    }

    pub fn readlink(&self, name: &OsStr) -> io::Result<PathBuf> {
        fs::read_link(self.at(name)?)
    }

    /// A link `name` pointing at `target`; fails if `name` exists.
    pub fn symlink(&self, target: &Path, name: &OsStr) -> io::Result<()> {
        std::os::unix::fs::symlink(target, self.at(name)?)
    }

    /// The folder's own mode, through its fd.
    pub fn chmod(&self, mode: u32) -> io::Result<()> {
        self.file.set_permissions(Permissions::from_mode(mode))
    }

    /// An entry's owner, without following a link.
    pub fn lchown(&self, name: &OsStr, uid: u32, gid: u32) -> io::Result<()> {
        std::os::unix::fs::lchown(self.at(name)?, Some(uid), Some(gid))
    }

    /// The names in this folder, `.` and `..` left out, at most `max`.
    pub fn entries(&self, max: usize) -> io::Result<Vec<OsString>> {
        let mut names = Vec::new();
        for entry in fs::read_dir(self.proc_path())? {
            if names.len() >= max {
                return Err(io::Error::other(format!("more than {max} entries")));
            }
            names.push(entry?.file_name());
        }
        names.sort();
        Ok(names)
    }

    /// Flushes the folder's entries to disk.
    pub fn sync(&self) -> io::Result<()> {
        self.file.sync_all()
    }

    /// Removes `name` and, if it is a folder, everything in it, never
    /// following a link: a link is removed, not what it points at. At most
    /// `budget` entries in all, 32 levels down.
    pub fn remove_tree(&self, name: &OsStr, budget: &mut usize) -> io::Result<()> {
        self.remove_tree_at(name, budget, 0)
    }

    fn remove_tree_at(&self, name: &OsStr, budget: &mut usize, depth: usize) -> io::Result<()> {
        let Some(meta) = self.stat_opt(name)? else {
            return Ok(());
        };
        if *budget == 0 || depth > 32 {
            return Err(io::Error::other("too much to remove"));
        }
        *budget -= 1;
        if !meta.is_dir() {
            return self.unlink(name);
        }
        let sub = self.open_dir(name)?;
        if sub.meta()?.id() != meta.id() {
            return Err(io::Error::other("a folder changed while it was removed"));
        }
        for entry in sub.entries(*budget + 1)? {
            sub.remove_tree_at(&entry, budget, depth + 1)?;
        }
        self.rmdir(name)
    }
}

/// Opens `path` to read if its last part is a regular file: not a link, and
/// never blocking on a pipe or a device.
pub fn open_regular(path: &Path) -> io::Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)
        .open(path)?;
    if !file.metadata()?.is_file() {
        return Err(io::Error::other("not a regular file"));
    }
    Ok(file)
}

/// Reads at most `max` bytes of `file`, and says whether there was more.
pub fn read_capped(file: &mut File, max: usize) -> io::Result<(Vec<u8>, bool)> {
    let mut buf = Vec::new();
    file.by_ref().take(max as u64 + 1).read_to_end(&mut buf)?;
    let over = buf.len() > max;
    buf.truncate(max);
    Ok((buf, over))
}

/// A folder that only `owner` may change: owned by it, and writable by
/// neither its group nor others. `what` names it in the refusal.
pub fn check_private(meta: &Meta, owner: u32, what: &str) -> Result<(), String> {
    if !meta.is_dir() {
        return Err(format!("{what} is not a folder"));
    }
    if meta.uid != owner {
        return Err(format!("{what} is owned by uid {}, not {owner}", meta.uid));
    }
    if meta.shared_write() {
        return Err(format!(
            "{what} is writable by others ({:04o})",
            meta.perm()
        ));
    }
    Ok(())
}

/// A test folder of its own under the system's temporary folder.
#[cfg(test)]
pub fn scratch(tag: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU32, Ordering};
    static N: AtomicU32 = AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!(
        "lumend-{tag}-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[cfg(test)]
mod tests {
    use super::*;

    fn os(s: &str) -> &OsStr {
        OsStr::new(s)
    }

    #[test]
    fn refuses_names_that_are_not_one_name() {
        for bad in ["", ".", "..", "a/b", "/a", "a\0b"] {
            assert!(!name_ok(os(bad)), "{bad:?}");
        }
        assert!(name_ok(os("lg-icons.new")));
    }

    #[test]
    fn open_path_refuses_a_link_on_the_way() {
        let root = scratch("fsat-link");
        fs::create_dir_all(root.join("real/inner")).unwrap();
        std::os::unix::fs::symlink(root.join("real"), root.join("link")).unwrap();
        assert!(Dir::open_path(&root.join("real/inner")).is_ok());
        assert!(Dir::open_path(&root.join("link/inner")).is_err());
        assert!(Dir::open_path(&root.join("link")).is_err());
        assert!(Dir::open_path(Path::new("relative")).is_err());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn works_in_the_folder_it_holds_after_a_rename() {
        let root = scratch("fsat-rename");
        fs::create_dir_all(root.join("a")).unwrap();
        let dir = Dir::open_path(&root.join("a")).unwrap();
        fs::rename(root.join("a"), root.join("b")).unwrap();
        // A link where the folder was: the held folder is used, not it.
        std::os::unix::fs::symlink("/", root.join("a")).unwrap();
        let mut f = dir.create(os("x"), 0o644).unwrap();
        std::io::Write::write_all(&mut f, b"hi").unwrap();
        assert!(root.join("b/x").exists());
        assert_eq!(dir.stat(os("x")).unwrap().perm(), 0o644);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn never_reads_through_a_link_or_a_pipe() {
        let root = scratch("fsat-read");
        fs::write(root.join("f"), b"data").unwrap();
        std::os::unix::fs::symlink(root.join("f"), root.join("l")).unwrap();
        let dir = Dir::open_path(&root).unwrap();
        assert!(dir.open_read(os("f")).is_ok());
        assert!(dir.open_read(os("l")).is_err());
        assert!(dir.open_dir(os("l")).is_err());
        assert!(dir.open_read(os("..")).is_err());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn remove_tree_removes_links_not_their_targets() {
        let root = scratch("fsat-tree");
        fs::create_dir_all(root.join("keep")).unwrap();
        fs::write(root.join("keep/precious"), b"x").unwrap();
        fs::create_dir_all(root.join("gone/deep/er")).unwrap();
        fs::write(root.join("gone/deep/er/f"), b"x").unwrap();
        std::os::unix::fs::symlink(root.join("keep"), root.join("gone/deep/link")).unwrap();
        let dir = Dir::open_path(&root).unwrap();
        let mut budget = 100;
        dir.remove_tree(os("gone"), &mut budget).unwrap();
        assert!(!root.join("gone").exists());
        assert!(root.join("keep/precious").exists());
        // Nothing there is fine.
        dir.remove_tree(os("gone"), &mut budget).unwrap();
        let mut tiny = 1;
        fs::create_dir_all(root.join("big/a")).unwrap();
        assert!(dir.remove_tree(os("big"), &mut tiny).is_err());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn reads_at_most_the_cap() {
        let root = scratch("fsat-cap");
        fs::write(root.join("f"), b"abcdef").unwrap();
        let mut f = File::open(root.join("f")).unwrap();
        assert_eq!(read_capped(&mut f, 4).unwrap(), (b"abcd".to_vec(), true));
        let mut f = File::open(root.join("f")).unwrap();
        assert_eq!(read_capped(&mut f, 6).unwrap(), (b"abcdef".to_vec(), false));
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn private_folders_are_the_owners_alone() {
        let root = scratch("fsat-private");
        let dir = Dir::open_path(&root).unwrap();
        dir.chmod(0o755).unwrap();
        let me = crate::sys::os::euid();
        let meta = dir.meta().unwrap();
        assert!(check_private(&meta, me, "x").is_ok());
        assert!(check_private(&meta, me + 1, "x").is_err());
        dir.chmod(0o777).unwrap();
        assert!(check_private(&dir.meta().unwrap(), me, "x").is_err());
        fs::remove_dir_all(&root).unwrap();
    }
}
