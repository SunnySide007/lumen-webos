//! `lumend groups`: which of the TV's permission groups hold each method
//! lumend calls, read from the hub's API permission files. Read-only.
//!
//! lumend's client permissions (profiles/lumend/role-install.sh) name the
//! groups it is in. Homebrew Channel's service is in `all`; Lumen's app is
//! in `public` only (b9.md, "Platform"). The role asks for the groups this
//! command prints, no more: the owner picks them after reading its output,
//! since which groups a method is in is the TV's, not ours.
//!
//! An API permission file is one JSON object, group name to a list of
//! method patterns (`com.webos.applicationManager/*`); `*` matches any run
//! of characters.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;

use serde_json::Value;

use crate::bus::METHODS;
use crate::fsat::{Dir, read_capped};
use crate::paths::Root;

/// Where the hub's API permissions are: LG's, then the developer ones.
pub const API_DIRS: [&str; 2] = [
    "/usr/share/luna-service2/api-permissions.d",
    "/var/luna-service2-dev/api-permissions.d",
];
const MAX_FILES: usize = 1000;
const MAX_BYTES: usize = 1 << 20;
const MAX_NAMED: usize = 16;

/// `luna://service/category/method` as the permission files write it.
pub fn method_of(uri: &str) -> &str {
    uri.trim_start_matches("luna://")
}

/// Whether `pattern` (with `*` for any run of characters) matches `text`.
pub fn matches(pattern: &str, text: &str) -> bool {
    let parts: Vec<&str> = pattern.split('*').collect();
    let (first, rest) = parts.split_first().unwrap_or((&"", &[]));
    let Some(mut left) = text.strip_prefix(first) else {
        return false;
    };
    let Some((last, middle)) = rest.split_last() else {
        return left.is_empty();
    };
    for part in middle {
        match left.find(part) {
            Some(i) => left = &left[i + part.len()..],
            None => return false,
        }
    }
    left.len() >= last.len() && left.ends_with(last)
}

/// For each method, the groups whose patterns match it, from `files`'
/// contents (name, JSON).
pub fn holders(files: &[(String, Value)]) -> BTreeMap<&'static str, BTreeSet<String>> {
    let mut out: BTreeMap<&'static str, BTreeSet<String>> =
        METHODS.iter().map(|m| (*m, BTreeSet::new())).collect();
    for (_, json) in files {
        let Some(groups) = json.as_object() else {
            continue;
        };
        for (group, patterns) in groups {
            let Some(patterns) = patterns.as_array() else {
                continue;
            };
            for uri in METHODS {
                let method = method_of(uri);
                if patterns
                    .iter()
                    .filter_map(Value::as_str)
                    .any(|p| matches(p, method))
                    && let Some(set) = out.get_mut(uri)
                {
                    set.insert(crate::log::clean(group));
                }
            }
        }
    }
    out
}

/// The files of one folder, each read if it is a plain file under the cap.
fn read_dir(root: &Root, path: &str, lines: &mut Vec<String>) -> Vec<(String, Value)> {
    let dir = match Dir::open_path(&root.path(path)) {
        Ok(dir) => dir,
        Err(e) => {
            lines.push(format!("{path}: {e}"));
            return Vec::new();
        }
    };
    let names = match dir.entries(MAX_FILES) {
        Ok(names) => names,
        Err(e) => {
            lines.push(format!("{path}: {e}"));
            return Vec::new();
        }
    };
    let mut files = Vec::new();
    for name in names {
        if !name.to_string_lossy().ends_with(".json") {
            continue;
        }
        let read = dir
            .open_read(&name)
            .and_then(|mut f| read_capped(&mut f, MAX_BYTES));
        match read {
            Ok((bytes, false)) => match serde_json::from_slice(&bytes) {
                Ok(json) => files.push((name.to_string_lossy().into_owned(), json)),
                Err(_) => lines.push(format!("{path}/{}: not JSON", show(&name))),
            },
            Ok((_, true)) => lines.push(format!("{path}/{}: over {MAX_BYTES} bytes", show(&name))),
            Err(e) => lines.push(format!("{path}/{}: {e}", show(&name))),
        }
    }
    lines.push(format!("{path}: {} files read", files.len()));
    files
}

fn show(name: &OsStr) -> String {
    crate::log::clean(&name.to_string_lossy())
}

/// The report: each method and its groups, then the fewest groups that
/// hold them all, picked greedily.
pub fn report(root: &Root) -> Vec<String> {
    let mut lines = Vec::new();
    let mut files = Vec::new();
    for dir in API_DIRS {
        files.extend(read_dir(root, dir, &mut lines));
    }
    let held = holders(&files);
    for (uri, groups) in &held {
        let named: Vec<&str> = groups.iter().take(MAX_NAMED).map(String::as_str).collect();
        let more = groups.len().saturating_sub(MAX_NAMED);
        lines.push(format!(
            "{}: {}{}",
            method_of(uri),
            if named.is_empty() {
                "IN NO GROUP".to_owned()
            } else {
                named.join(" ")
            },
            if more > 0 {
                format!(" and {more} more")
            } else {
                String::new()
            }
        ));
    }
    let mut left: BTreeSet<&str> = held
        .iter()
        .filter(|(_, g)| !g.is_empty())
        .map(|(m, _)| *m)
        .collect();
    let mut pick = Vec::new();
    while !left.is_empty() {
        let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
        for m in &left {
            for g in &held[m] {
                *counts.entry(g.as_str()).or_default() += 1;
            }
        }
        let Some((best, _)) = counts
            .iter()
            .max_by_key(|(g, n)| (**n, std::cmp::Reverse(**g)))
        else {
            break;
        };
        let best = (*best).to_owned();
        left.retain(|m| !held[m].contains(&best));
        pick.push(best);
    }
    lines.push(format!("fewest groups for them all: {}", pick.join(" ")));
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn stars_match_runs_of_characters() {
        assert!(matches(
            "com.webos.applicationManager/*",
            "com.webos.applicationManager/running"
        ));
        assert!(matches("*", "x/y"));
        assert!(matches(
            "com.webos.*/launch",
            "com.webos.applicationManager/launch"
        ));
        assert!(!matches(
            "com.webos.applicationManager/run",
            "com.webos.applicationManager/running"
        ));
        assert!(!matches(
            "com.webos.service.tvpower/*",
            "com.webos.applicationManager/launch"
        ));
        assert!(matches("a*b*c", "abbc"));
        assert!(!matches("a*bc*c", "abc"));
        assert!(matches("exact", "exact"));
    }

    #[test]
    fn finds_the_groups_for_each_method_and_the_fewest() {
        let files = vec![(
            "x.json".to_owned(),
            json!({
                "applications": ["com.webos.applicationManager/*"],
                "settings.read": ["com.webos.settingsservice/getSystemSettings"],
                "tvpower": ["com.webos.service.tvpower/power/*"],
                "bus": ["com.palm.bus/signal/*"],
                "odd": "not a list"
            }),
        )];
        let held = holders(&files);
        assert!(held[crate::bus::LAUNCH].contains("applications"));
        assert!(held[crate::bus::SCREEN_OFF].contains("tvpower"));
        assert!(held[crate::bus::GET_SETTINGS].contains("settings.read"));
        assert!(held.values().all(|g| !g.is_empty()), "{held:?}");
    }

    #[test]
    fn reports_from_a_folder_and_skips_what_isnt_json() {
        let base = crate::fsat::scratch("groups");
        let root = Root::at(&base);
        let dir = root.path(API_DIRS[0]);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.json"), r#"{"all": ["*"]}"#).unwrap();
        std::fs::write(dir.join("b.json"), "not json").unwrap();
        std::fs::write(dir.join("c.txt"), "{}").unwrap();
        let lines = report(&root);
        assert!(
            lines.iter().any(|l| l.ends_with("b.json: not JSON")),
            "{lines:?}"
        );
        assert!(
            lines.iter().any(|l| l == "fewest groups for them all: all"),
            "{lines:?}"
        );
        std::fs::remove_dir_all(&base).unwrap();
    }
}
