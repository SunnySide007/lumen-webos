//! The file-mode watch: Lumen's folder made root's alone again after LG's
//! developer-mode job opens it to everyone.
//!
//! LG's `devmode` upstart job (started on `minimal-boot-done`) runs `chmod
//! -R 777` over `/media/developer` at every boot while developer mode is on
//! (b9.md, "Platform"). That leaves Lumen's binary, its appinfo and the icon
//! copy writable by every app, since every developer-mode jail mounts
//! `/media/developer` read-write; and it makes the icon copy refuse after
//! every boot. lumend puts back the modes the IPK installs
//! (`tools/ipk/fix_modes.py`): folders 0755, `lumen-shell` and
//! `lumen-icons.sh` 0755, every other file 0644. It acts after LG's job
//! because it acts on the kernel's word: inotify tells it when a mode
//! changes in a folder it watches, and it walks the folder again three
//! seconds after the last change.
//!
//! What it touches, and how:
//!
//! * The three shared folders the IPK sets (`/media/developer/apps/usr`,
//!   `usr/palm`, `usr/palm/applications`), then Lumen's folder and
//!   everything in it. `/media/developer` and `/media/developer/apps` are
//!   LG's and stay 0777.
//! * Only what root owns. An entry someone else owns is left as it is and
//!   named in the log: it is not Lumen's to fix, and it may be what an app
//!   put there while the folder was open.
//! * Never through a link: the walk holds each folder open and names one
//!   entry at a time; a file is changed through its own fd, opened without
//!   following a link or waiting on a pipe, once its device and inode match
//!   what the walk saw. A file with more than one link is left alone (a
//!   hard link made while the folder was open could point at a file
//!   elsewhere on the same partition).
//! * Links, pipes and sockets are counted, never followed or changed.
//!
//! The undo is nothing: LG's job opens the folders again at the next boot.

use std::ffi::OsStr;

use crate::fsat::{Dir, open_regular};
use crate::paths::{APP_ID, DEV_APPS, Root, SHARED};

/// The files the IPK installs executable, at the top of Lumen's folder.
pub const EXEC: [&str; 2] = ["lumen-shell", "lumen-icons.sh"];
/// The icon copy's lock: a link by design (icons.rs).
const LOCK: &str = "lg-icons.lock";
const MAX_ENTRIES: usize = 20_000;
const MAX_DEPTH: usize = 16;
/// The most names one report lists of each kind.
const MAX_NAMED: usize = 12;

/// What one walk found and did.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Report {
    /// Modes put back.
    pub fixed: u32,
    /// Entries not root's, by their path in Lumen's folder.
    pub not_root: Vec<String>,
    /// Files left alone for having more than one link.
    pub hard_linked: Vec<String>,
    /// Links, the icon lock aside.
    pub links: Vec<String>,
    /// Pipes, sockets and devices.
    pub other: u32,
    pub entries: usize,
}

impl Report {
    fn name(list: &mut Vec<String>, path: String) {
        if list.len() < MAX_NAMED {
            list.push(crate::log::clean(&path));
        }
    }

    /// One line for the log.
    pub fn describe(&self) -> String {
        let mut text = format!("modes: {} entries, {} put back", self.entries, self.fixed);
        for (what, list) in [
            ("NOT ROOT'S, left alone", &self.not_root),
            ("hard-linked, left alone", &self.hard_linked),
            ("links", &self.links),
        ] {
            if !list.is_empty() {
                text.push_str(&format!("; {what}: {}", list.join(" ")));
            }
        }
        if self.other > 0 {
            text.push_str(&format!("; {} pipes, sockets or devices", self.other));
        }
        text
    }

    /// Whether something there isn't as the IPK installs it and lumend
    /// can't put it right.
    pub fn suspicious(&self) -> bool {
        !self.not_root.is_empty() || !self.hard_linked.is_empty() || !self.links.is_empty()
    }
}

/// One walk. `watch` is called with each folder while it is held open, so
/// the daemon can add its inotify watch through the folder's fd.
pub fn fix(root: &Root, watch: &mut dyn FnMut(&Dir)) -> Result<Report, String> {
    let me = crate::sys::os::euid();
    let mut report = Report::default();
    let mut dir = Dir::open_path(&root.path(DEV_APPS)).map_err(|e| format!("{DEV_APPS}: {e}"))?;
    let mut shown = DEV_APPS.to_owned();
    for name in SHARED.iter().chain([&APP_ID]) {
        shown = format!("{shown}/{name}");
        let sub = dir
            .open_dir(OsStr::new(name))
            .map_err(|e| format!("{shown}: {e}"))?;
        let meta = sub.meta().map_err(|e| e.to_string())?;
        if meta.uid != me {
            return Err(format!(
                "{shown} is owned by uid {}, not root; left alone",
                meta.uid
            ));
        }
        if meta.perm() != 0o755 {
            sub.chmod(0o755).map_err(|e| format!("{shown}: {e}"))?;
            report.fixed += 1;
        }
        watch(&sub);
        dir = sub;
    }
    let mut budget = MAX_ENTRIES;
    walk(&dir, "", 0, me, &mut report, &mut budget, watch)?;
    Ok(report)
}

fn walk(
    dir: &Dir,
    at: &str,
    depth: usize,
    me: u32,
    report: &mut Report,
    budget: &mut usize,
    watch: &mut dyn FnMut(&Dir),
) -> Result<(), String> {
    let names = dir
        .entries(*budget)
        .map_err(|e| format!("Lumen's folder{at}: {e}"))?;
    *budget = budget.saturating_sub(names.len());
    for name in names {
        report.entries += 1;
        let path = format!("{at}/{}", name.to_string_lossy());
        let Ok(meta) = dir.stat(&name) else {
            continue;
        };
        if meta.is_link() {
            if !(depth == 0 && name == LOCK) {
                Report::name(&mut report.links, path);
            }
            continue;
        }
        if meta.uid != me {
            Report::name(&mut report.not_root, path);
            continue;
        }
        if meta.is_dir() {
            let Ok(sub) = dir.open_dir(&name) else {
                continue;
            };
            if sub.meta().map(|m| m.id()).ok() != Some(meta.id()) {
                continue;
            }
            if meta.perm() != 0o755 && sub.chmod(0o755).is_ok() {
                report.fixed += 1;
            }
            if depth + 1 >= MAX_DEPTH {
                return Err(format!(
                    "Lumen's folder{path} is deeper than {MAX_DEPTH} levels"
                ));
            }
            watch(&sub);
            walk(&sub, &path, depth + 1, me, report, budget, watch)?;
        } else if meta.is_file() {
            let exec = depth == 0 && EXEC.iter().any(|e| OsStr::new(e) == name);
            let want = if exec { 0o755 } else { 0o644 };
            if meta.perm() == want {
                continue;
            }
            if meta.nlink != 1 {
                Report::name(&mut report.hard_linked, path);
                continue;
            }
            let Ok(file) = dir.at(&name).and_then(|p| open_regular(&p)) else {
                continue;
            };
            let same = file.metadata().is_ok_and(|m| {
                use std::os::unix::fs::MetadataExt;
                (m.dev(), m.ino()) == meta.id() && m.uid() == me && m.nlink() == 1
            });
            if same {
                use std::os::unix::fs::PermissionsExt;
                if file
                    .set_permissions(std::fs::Permissions::from_mode(want))
                    .is_ok()
                {
                    report.fixed += 1;
                }
            }
        } else {
            report.other += 1;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;

    fn mode(p: &PathBuf) -> u32 {
        fs::symlink_metadata(p).unwrap().permissions().mode() & 0o7777
    }

    fn chmod(p: &PathBuf, m: u32) {
        fs::set_permissions(p, fs::Permissions::from_mode(m)).unwrap();
    }

    /// Lumen's folder as LG's developer-mode job leaves it: everything
    /// 0777.
    fn after_devmode() -> (PathBuf, Root, PathBuf) {
        let base = crate::fsat::scratch("modes");
        let root = Root::at(&base);
        let app = root.path(&format!("{DEV_APPS}/usr/palm/applications/{APP_ID}"));
        fs::create_dir_all(app.join("art")).unwrap();
        fs::create_dir_all(app.join("lg-icons/usr/palm/applications/x")).unwrap();
        for f in [
            "lumen-shell",
            "lumen-icons.sh",
            "appinfo.json",
            "art/hero.png",
            "lg-icons/usr/palm/applications/x/i.png",
        ] {
            fs::write(app.join(f), b"x").unwrap();
        }
        let mut stack = vec![root.path("/media/developer")];
        while let Some(p) = stack.pop() {
            chmod(&p, 0o777);
            if p.is_dir() {
                for e in fs::read_dir(&p).unwrap() {
                    stack.push(e.unwrap().path());
                }
            }
        }
        (base, root, app)
    }

    #[test]
    fn puts_back_the_ipks_modes_after_devmode() {
        let (base, root, app) = after_devmode();
        let mut watched = 0;
        let report = fix(&root, &mut |_| watched += 1).unwrap();
        assert_eq!(mode(&app.join("lumen-shell")), 0o755);
        assert_eq!(mode(&app.join("lumen-icons.sh")), 0o755);
        assert_eq!(mode(&app.join("appinfo.json")), 0o644);
        assert_eq!(mode(&app.join("art/hero.png")), 0o644);
        assert_eq!(
            mode(&app.join("lg-icons/usr/palm/applications/x/i.png")),
            0o644
        );
        assert_eq!(mode(&app.join("lg-icons/usr/palm/applications/x")), 0o755);
        assert_eq!(mode(&app), 0o755);
        assert_eq!(mode(&root.path(&format!("{DEV_APPS}/usr"))), 0o755);
        // LG's two stay as LG left them.
        assert_eq!(mode(&root.path(DEV_APPS)), 0o777);
        assert_eq!(mode(&root.path("/media/developer")), 0o777);
        assert!(!report.suspicious(), "{report:?}");
        assert!(watched >= 9, "{watched}");
        // A second walk finds nothing to do.
        assert_eq!(fix(&root, &mut |_| ()).unwrap().fixed, 0);
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn follows_no_link_and_leaves_hard_links_and_pipes() {
        let (base, root, app) = after_devmode();
        let outside = base.join("outside");
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("secret"), b"s").unwrap();
        chmod(&outside.join("secret"), 0o600);
        std::os::unix::fs::symlink(outside.join("secret"), app.join("evil.png")).unwrap();
        std::os::unix::fs::symlink(&outside, app.join("evil-dir")).unwrap();
        fs::hard_link(outside.join("secret"), app.join("hard.png")).unwrap();
        std::os::unix::fs::symlink("12345", app.join(LOCK)).unwrap();
        let report = fix(&root, &mut |_| ()).unwrap();
        assert_eq!(mode(&outside.join("secret")), 0o600, "never touched");
        assert_eq!(mode(&outside), mode(&outside));
        assert_eq!(report.links, ["/evil-dir", "/evil.png"]);
        assert_eq!(report.hard_linked, ["/hard.png"]);
        assert!(report.suspicious());
        assert!(
            report
                .describe()
                .contains("hard-linked, left alone: /hard.png")
        );
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn refuses_a_link_on_the_way_and_stops_at_a_folder_not_roots() {
        let (base, root, _app) = after_devmode();
        let apps = root.path(&format!("{DEV_APPS}/usr/palm"));
        fs::rename(apps.join("applications"), apps.join("moved")).unwrap();
        std::os::unix::fs::symlink("moved", apps.join("applications")).unwrap();
        assert!(fix(&root, &mut |_| ()).is_err());
        fs::remove_dir_all(&base).unwrap();
    }
}
