//! What lumen-remote serves: the built front-end (`remote/ui/dist`), read
//! into memory at start, to paired devices; and to everyone else, one page
//! of its own with the pairing form. That page has no script, loads
//! nothing, and posts the code to `/pair`; its style is inline, allowed by
//! its hash in the page's content security policy.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use ring::digest;

use crate::ws::base64;

/// One file of the front-end.
pub struct File {
    pub body: Vec<u8>,
    pub content_type: &'static str,
    /// Vite's hashed files never change under their name.
    pub immutable: bool,
}

pub struct Site {
    files: HashMap<String, File>,
}

const MAX_FILE: u64 = 4 << 20;
const MAX_TOTAL: u64 = 16 << 20;
const MAX_FILES: usize = 256;

fn content_type(name: &str) -> Option<&'static str> {
    let ext = name.rsplit_once('.')?.1;
    Some(match ext {
        "html" => "text/html; charset=utf-8",
        "js" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "woff2" => "font/woff2",
        "txt" => "text/plain; charset=utf-8",
        "json" => "application/json",
        "webmanifest" => "application/manifest+json",
        _ => return None,
    })
}

/// A path segment the site takes: no dots first, nothing but plain
/// characters.
fn plain(seg: &str) -> bool {
    !seg.is_empty()
        && !seg.starts_with('.')
        && seg
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
}

impl Site {
    /// Reads every file under `dir`, at most four folders deep. Links,
    /// dot-files and unknown types are skipped; `index.html` is needed.
    pub fn load(dir: &Path) -> Result<Self, String> {
        let mut files = HashMap::new();
        let mut total = 0u64;
        walk(dir, "", 0, &mut files, &mut total)?;
        if !files.contains_key("/index.html") {
            return Err(format!("no index.html in {}", dir.display()));
        }
        Ok(Self { files })
    }

    #[cfg(test)]
    pub fn from(list: &[(&str, &str)]) -> Self {
        let files = list
            .iter()
            .map(|(p, body)| {
                (
                    (*p).to_owned(),
                    File {
                        body: body.as_bytes().to_vec(),
                        content_type: content_type(p).unwrap_or("text/plain"),
                        immutable: p.starts_with("/assets/"),
                    },
                )
            })
            .collect();
        Self { files }
    }

    /// The file at a request's path; `/` is `index.html`.
    pub fn get(&self, path: &str) -> Option<&File> {
        let path = if path == "/" { "/index.html" } else { path };
        if !path[1..].split('/').all(plain) {
            return None;
        }
        self.files.get(path)
    }

    pub fn len(&self) -> usize {
        self.files.len()
    }
}

fn walk(
    dir: &Path,
    prefix: &str,
    depth: usize,
    files: &mut HashMap<String, File>,
    total: &mut u64,
) -> Result<(), String> {
    let entries = fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("{}: {e}", dir.display()))?;
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if !plain(&name) {
            continue;
        }
        let meta = entry.file_type().map_err(|e| format!("{name}: {e}"))?;
        let path = format!("{prefix}/{name}");
        if meta.is_dir() && depth < 4 {
            walk(&entry.path(), &path, depth + 1, files, total)?;
        } else if meta.is_file()
            && let Some(content_type) = content_type(&name)
        {
            let len = entry.metadata().map(|m| m.len()).unwrap_or(u64::MAX);
            *total += len;
            if len > MAX_FILE || *total > MAX_TOTAL || files.len() >= MAX_FILES {
                return Err(format!("the front-end is too big at {path}"));
            }
            let body = fs::read(entry.path()).map_err(|e| format!("{path}: {e}"))?;
            let immutable = path.starts_with("/assets/");
            files.insert(
                path,
                File {
                    body,
                    content_type,
                    immutable,
                },
            );
        }
    }
    Ok(())
}

const STYLE: &str = "\
:root{color-scheme:dark}\
*{box-sizing:border-box}\
body{margin:0;min-height:100vh;display:grid;place-items:center;padding:24px;\
background:radial-gradient(120% 90% at 50% 0%,#1b2340 0%,#0e1222 55%,#000 100%);\
color:#ebeef5;font:16px/1.45 system-ui,-apple-system,'Segoe UI',Roboto,sans-serif}\
main{width:min(420px,100%);padding:28px;border-radius:28px;\
background:rgba(255,255,255,.06);border:1px solid rgba(255,255,255,.12);\
box-shadow:0 30px 80px rgba(0,0,0,.45)}\
h1{margin:0 0 6px;font-size:24px;font-weight:600}\
p{margin:0 0 18px;color:#8c94a5}\
p.msg{color:#ebeef5;background:rgba(255,120,120,.14);padding:10px 14px;border-radius:14px}\
p.msg.ok{background:rgba(120,200,255,.14)}\
label{display:block;margin:14px 0 6px;color:#8c94a5;font-size:14px}\
input{width:100%;padding:14px 16px;border-radius:16px;border:1px solid rgba(255,255,255,.18);\
background:rgba(0,0,0,.35);color:#ebeef5;font:inherit}\
input.code{font-size:32px;letter-spacing:.35em;text-align:center;font-variant-numeric:tabular-nums}\
button{margin-top:20px;width:100%;padding:14px;border:0;border-radius:999px;\
background:#ebeef5;color:#0e1222;font:600 17px system-ui,sans-serif}\
ol{margin:0 0 6px;padding-left:20px;color:#8c94a5}";

/// The pairing page's content security policy: its own style, its own
/// form, nothing else.
pub fn pair_csp() -> String {
    let hash = digest::digest(&digest::SHA256, STYLE.as_bytes());
    format!(
        "default-src 'none'; style-src 'sha256-{}'; form-action 'self'; base-uri 'none'; frame-ancestors 'none'",
        base64(hash.as_ref())
    )
}

fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

/// The page an unpaired browser gets, with a message when the last try
/// said something (`ok` colours it as news rather than a problem).
pub fn pair_page(message: Option<(&str, bool)>) -> Vec<u8> {
    let msg = message.map_or_else(String::new, |(text, ok)| {
        format!(
            "<p class=\"msg{}\" role=\"status\">{}</p>",
            if ok { " ok" } else { "" },
            escape(text)
        )
    });
    format!(
        "<!doctype html>\n<html lang=\"en\"><head><meta charset=\"utf-8\">\
<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
<meta name=\"color-scheme\" content=\"dark\"><meta name=\"referrer\" content=\"same-origin\">\
<title>Pair with Lumen Remote</title><style>{STYLE}</style></head><body><main>\
<h1>Pair this phone</h1>\
<ol><li>On the PC, open Lumen Remote and choose Pair a phone.</li>\
<li>The TV and the PC show a six-digit code for two minutes.</li>\
<li>Type it here. Five wrong codes close pairing.</li></ol>\
{msg}\
<form method=\"post\" action=\"/pair\" autocomplete=\"off\">\
<label for=\"code\">The code on the TV</label>\
<input class=\"code\" id=\"code\" name=\"code\" inputmode=\"numeric\" autocomplete=\"one-time-code\" \
pattern=\"[0-9]{{6}}\" minlength=\"6\" maxlength=\"6\" required autofocus>\
<label for=\"name\">This phone's name, as the PC will list it</label>\
<input id=\"name\" name=\"name\" maxlength=\"40\" value=\"Phone\" required>\
<button type=\"submit\">Pair</button></form></main></body></html>\n"
    )
    .into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serves_only_plain_paths() {
        let site = Site::from(&[
            ("/index.html", "<!doctype html>"),
            ("/assets/app-1a2b.js", "x"),
        ]);
        assert!(site.get("/").is_some());
        assert!(site.get("/assets/app-1a2b.js").unwrap().immutable);
        assert!(!site.get("/index.html").unwrap().immutable);
        for bad in [
            "/../index.html",
            "/assets/../index.html",
            "/.env",
            "//index.html",
            "/assets/",
        ] {
            assert!(site.get(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn the_pairing_page_escapes_and_its_policy_matches_its_style() {
        let page = String::from_utf8(pair_page(Some(("<b>wrong</b>", false)))).unwrap();
        assert!(page.contains("&lt;b&gt;wrong&lt;/b&gt;"));
        assert!(!page.contains("<script"));
        assert!(page.contains("action=\"/pair\""));
        let csp = pair_csp();
        assert!(csp.starts_with("default-src 'none'; style-src 'sha256-"));
        // The hash is of exactly what sits between the style tags.
        let style = page
            .split("<style>")
            .nth(1)
            .unwrap()
            .split("</style>")
            .next()
            .unwrap();
        let hash = base64(digest::digest(&digest::SHA256, style.as_bytes()).as_ref());
        assert!(csp.contains(&hash));
    }

    #[test]
    fn loads_a_built_front_end() {
        let dir = std::env::temp_dir().join(format!("lumen-remote-web-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("assets")).unwrap();
        fs::write(dir.join("index.html"), "<!doctype html>").unwrap();
        fs::write(dir.join("assets/a.js"), "x").unwrap();
        fs::write(dir.join(".secret"), "no").unwrap();
        fs::write(dir.join("notes.md"), "no").unwrap();
        std::os::unix::fs::symlink("/etc/passwd", dir.join("passwd.txt")).unwrap();
        let site = Site::load(&dir).unwrap();
        assert_eq!(site.len(), 2);
        assert!(site.get("/passwd.txt").is_none());
        fs::remove_file(dir.join("index.html")).unwrap();
        assert!(Site::load(&dir).is_err());
        let _ = fs::remove_dir_all(&dir);
    }
}
