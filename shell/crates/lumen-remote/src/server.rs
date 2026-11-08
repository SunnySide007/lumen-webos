//! The server: one thread in `poll`, over the listening socket, the link to
//! lumen-ctl and every connection, each a TLS session with a small state
//! machine: HTTP requests, waiting on lumen-ctl (a cookie to check, a code
//! to try), or a WebSocket.
//!
//! What each request gets:
//!
//! | | Unpaired | Paired |
//! |---|---|---|
//! | `GET /` | the pairing page | the front-end's `index.html` |
//! | `POST /pair` | tries the code; on the right one, the cookie | back to `/` |
//! | `GET /ctl` (WebSocket) | 401 | the protocol, through the hub |
//! | `GET /whoami` | 401 | the device's id, name and role |
//! | any other `GET` | back to `/` | the front-end's file, or 404 |
//!
//! Every request must name the TV as its Host (one of `--name`, with the
//! port), or it gets 421. The form's POST and the WebSocket must also come
//! from the TV's own origin, or they get 403. The cookie is
//! `__Host-lumen`, `Secure; HttpOnly; SameSite=Strict`, holding the token
//! lumen-ctl made; lumen-remote keeps none on disk.

use std::collections::{HashMap, VecDeque};
use std::io::{self, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::sync::Arc;
use std::time::{Duration, Instant};

use rustls::{ServerConfig, ServerConnection};

use crate::args::Net;
use crate::http::{self, Parse, Request, Response};
use crate::hub::{Action, ConnId, Device, Hub};
use crate::log::Log;
use crate::sys;
use crate::web::{self, Site};
use crate::ws;

pub const COOKIE: &str = "__Host-lumen";
/// 400 days, the longest a browser keeps a cookie.
const COOKIE_AGE: u64 = 400 * 86_400;
const MAX_CONNS: usize = 24;
const FIRST_REQUEST: Duration = Duration::from_secs(15);
const IDLE: Duration = Duration::from_secs(30);
const WAIT: Duration = Duration::from_secs(10);
/// Plain text a slow client may leave unsent before it is dropped.
const MAX_BACKLOG: usize = 2 << 20;
const MAX_INBUF: usize = http::MAX_HEAD + http::MAX_BODY + ws::MAX_MESSAGE + 4096;
const MAX_CTL_LINE: usize = 65_536 + 1024;
const MAX_CTL_OUT: usize = 4 << 20;

/// The front-end's policy, as a header too (its meta tag allows any
/// WebSocket; the header holds it to this origin).
const APP_CSP: &str = "default-src 'self'; connect-src 'self'; img-src 'self' data:; font-src 'self'; style-src 'self'; script-src 'self'; manifest-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'";

pub struct Config {
    pub names: Vec<String>,
    pub port: u16,
    pub allow: Vec<Net>,
    pub tls: Arc<ServerConfig>,
    pub site: Site,
    pub pair_csp: String,
}

impl Config {
    /// The Host the request names, if it is the TV's.
    fn host<'a>(&self, req: &'a Request) -> Option<&'a str> {
        let host = req.header("host")?;
        let lower = host.to_ascii_lowercase();
        let ok = self
            .names
            .iter()
            .any(|n| lower == format!("{n}:{}", self.port) || (self.port == 443 && lower == *n));
        ok.then_some(host)
    }

    fn same_origin(&self, req: &Request, host: &str) -> bool {
        req.header("origin")
            .is_some_and(|o| o.eq_ignore_ascii_case(&format!("https://{host}")))
    }

    fn allowed(&self, ip: Ipv4Addr) -> bool {
        self.allow.iter().any(|n| n.contains(ip))
    }
}

fn is_token(t: &str) -> bool {
    t.len() == 64 && t.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

enum Wait {
    Auth,
    Pair,
}

enum State {
    Http,
    Waiting(Request, Wait),
    Ws { reader: ws::Reader, device: Device },
}

struct Conn {
    id: ConnId,
    sock: TcpStream,
    tls: ServerConnection,
    peer: Ipv4Addr,
    inbuf: Vec<u8>,
    state: State,
    deadline: Option<Instant>,
    /// Close once everything queued is sent.
    closing: bool,
    /// Whether close_notify is queued.
    notified: bool,
    dead: bool,
    backlog: usize,
}

/// What one turn of a connection may need from the rest.
struct Ctx<'a> {
    cfg: &'a Config,
    hub: &'a mut Hub,
    actions: &'a mut VecDeque<Action>,
    log: &'a mut Log,
}

impl Conn {
    fn queue(&mut self, bytes: &[u8]) {
        if self.dead {
            return;
        }
        if self.tls.writer().write_all(bytes).is_err() {
            self.dead = true;
            return;
        }
        self.backlog += bytes.len();
        if self.backlog > MAX_BACKLOG {
            self.dead = true;
        }
    }

    fn respond(&mut self, r: &Response, req: &Request) {
        let keep = req.keep_alive() && !self.closing;
        self.queue(&r.bytes(req.method == "HEAD", keep));
        if keep {
            self.deadline = Some(Instant::now() + IDLE);
        } else {
            self.closing = true;
        }
    }

    /// A refusal that ends the connection.
    fn refuse(&mut self, status: u16, text: &str) {
        self.queue(&Response::text(status, text).bytes(false, false));
        self.closing = true;
    }

    fn send_ws(&mut self, line: &str) {
        if matches!(self.state, State::Ws { .. }) && !self.closing {
            self.queue(&ws::frame(ws::TEXT, line.as_bytes()));
        }
    }

    fn close_ws(&mut self, code: u16, reason: &str) {
        if matches!(self.state, State::Ws { .. }) && !self.closing {
            self.queue(&ws::close_frame(code, reason));
            self.closing = true;
        }
    }

    /// Reads what the socket has, into `inbuf`.
    fn read(&mut self, log: &mut Log) {
        let mut buf = [0u8; 16 << 10];
        loop {
            match self.tls.read_tls(&mut self.sock) {
                Ok(0) => {
                    self.dead = true;
                    return;
                }
                Ok(_) => {}
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => return,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(_) => {
                    self.dead = true;
                    return;
                }
            }
            if let Err(e) = self.tls.process_new_packets() {
                log.line(&format!("conn {} from {}: TLS: {e}", self.id, self.peer));
                let _ = self.tls.write_tls(&mut self.sock);
                self.dead = true;
                return;
            }
            loop {
                match self.tls.reader().read(&mut buf) {
                    Ok(0) => {
                        self.closing = true;
                        break;
                    }
                    Ok(n) => self.inbuf.extend_from_slice(&buf[..n]),
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                    Err(_) => {
                        self.dead = true;
                        return;
                    }
                }
            }
            if self.inbuf.len() > MAX_INBUF {
                self.dead = true;
                return;
            }
        }
    }

    fn flush(&mut self) {
        if self.closing && !self.notified {
            self.tls.send_close_notify();
            self.notified = true;
        }
        while !self.dead && self.tls.wants_write() {
            match self.tls.write_tls(&mut self.sock) {
                Ok(n) => self.backlog = self.backlog.saturating_sub(n),
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(_) => self.dead = true,
            }
        }
        if self.closing && !self.tls.wants_write() {
            self.dead = true;
        }
    }

    /// Handles whatever whole requests or messages `inbuf` holds.
    fn process(&mut self, cx: &mut Ctx) {
        while !self.dead && !self.closing {
            match &mut self.state {
                State::Waiting(..) => return,
                State::Http => match http::parse(&self.inbuf) {
                    Parse::More => return,
                    Parse::Bad(status, why) => {
                        self.refuse(status, why);
                        return;
                    }
                    Parse::Done(req, used) => {
                        self.inbuf.drain(..used);
                        self.request(req, cx);
                    }
                },
                State::Ws { reader, device } => {
                    let device_id = device.id.clone();
                    match reader.next(&self.inbuf) {
                        Ok(None) => return,
                        Ok(Some((msg, used))) => {
                            self.inbuf.drain(..used);
                            match msg {
                                ws::Message::Text(t) => {
                                    cx.actions.extend(cx.hub.phone_message(self.id, &t));
                                }
                                ws::Message::Ping(p) => self.queue(&ws::frame(ws::PONG, &p)),
                                ws::Message::Close(_) => {
                                    self.queue(&ws::close_frame(1000, ""));
                                    self.closing = true;
                                }
                                ws::Message::Nothing => {}
                            }
                        }
                        Err((code, why)) => {
                            cx.log
                                .line(&format!("device {device_id}: WebSocket refused: {why}"));
                            self.queue(&ws::close_frame(code, why));
                            self.closing = true;
                        }
                    }
                }
            }
        }
    }

    /// One HTTP request: the Host first, then the cookie, if any, is
    /// checked with lumen-ctl before the request is routed.
    fn request(&mut self, req: Request, cx: &mut Ctx) {
        if cx.cfg.host(&req).is_none() {
            self.refuse(421, "this server answers only to the TV's own name");
            return;
        }
        match req
            .cookie(COOKIE)
            .filter(|t| is_token(t))
            .map(str::to_owned)
        {
            Some(token) => {
                self.state = State::Waiting(req, Wait::Auth);
                self.deadline = Some(Instant::now() + WAIT);
                cx.actions.extend(cx.hub.auth(self.id, &token));
            }
            None => self.route(req, None, cx),
        }
    }

    fn pair_page(
        &self,
        msg: Option<(&str, bool)>,
        cfg: &Config,
        status: u16,
        clear: bool,
    ) -> Response {
        let mut r = Response::new(status, "text/html; charset=utf-8", web::pair_page(msg))
            .with("Content-Security-Policy", cfg.pair_csp.clone())
            .with("Cache-Control", "no-store");
        if clear {
            r = r.with(
                "Set-Cookie",
                format!("{COOKIE}=; Path=/; Secure; HttpOnly; SameSite=Strict; Max-Age=0"),
            );
        }
        r
    }

    fn route(&mut self, req: Request, device: Option<Device>, cx: &mut Ctx) {
        let Some(host) = cx.cfg.host(&req).map(str::to_owned) else {
            self.refuse(421, "this server answers only to the TV's own name");
            return;
        };
        let get = matches!(req.method.as_str(), "GET" | "HEAD");
        let stale = device.is_none() && req.cookie(COOKIE).is_some();
        let token = req.cookie(COOKIE).unwrap_or("").to_owned();
        match (device, req.path.as_str()) {
            (device, "/ctl") if req.method == "GET" && req.has_token("upgrade", "websocket") => {
                let Some(device) = device else {
                    self.refuse(401, "not paired");
                    return;
                };
                if !cx.cfg.same_origin(&req, &host) {
                    cx.log.line(&format!(
                        "device {}: WebSocket from another origin refused",
                        device.id
                    ));
                    self.refuse(403, "the WebSocket must come from the TV's own page");
                    return;
                }
                if req.header("sec-websocket-version") != Some("13") {
                    self.queue(
                        &Response::text(426, "WebSocket 13 only")
                            .with("Sec-WebSocket-Version", "13")
                            .bytes(false, false),
                    );
                    self.closing = true;
                    return;
                }
                let Some(key) = req.header("sec-websocket-key").filter(|k| ws::good_key(k)) else {
                    self.refuse(400, "a bad Sec-WebSocket-Key");
                    return;
                };
                let r = Response {
                    status: 101,
                    headers: vec![
                        ("Upgrade", "websocket".to_owned()),
                        ("Connection", "Upgrade".to_owned()),
                        ("Sec-WebSocket-Accept", ws::accept_key(key)),
                    ],
                    body: Vec::new(),
                };
                self.queue(&r.bytes(false, true));
                self.deadline = None;
                cx.log.line(&format!(
                    "device {} ({}): WebSocket open from {}",
                    device.id, device.role, self.peer
                ));
                cx.actions
                    .extend(cx.hub.ws_open(self.id, &token, device.clone()));
                self.state = State::Ws {
                    reader: ws::Reader::default(),
                    device,
                };
            }
            (None, "/pair") if req.method == "POST" => {
                if !cx.cfg.same_origin(&req, &host) {
                    self.refuse(403, "the form must come from the TV's own page");
                    return;
                }
                let code = http::form_value(&req.body, "code").unwrap_or_default();
                let code = code.trim().to_owned();
                if code.len() != 6 || !code.bytes().all(|b| b.is_ascii_digit()) {
                    let r = self.pair_page(
                        Some(("The code is six digits.", false)),
                        cx.cfg,
                        200,
                        false,
                    );
                    self.respond(&r, &req);
                    return;
                }
                let name = http::form_value(&req.body, "name").unwrap_or_default();
                let name: String = name
                    .trim()
                    .chars()
                    .filter(|c| !c.is_control())
                    .take(40)
                    .collect();
                let name = if name.is_empty() {
                    "Phone".to_owned()
                } else {
                    name
                };
                self.state = State::Waiting(req, Wait::Pair);
                self.deadline = Some(Instant::now() + WAIT);
                cx.actions.extend(cx.hub.pair(self.id, &code, &name));
            }
            (None, "/") if get => {
                let r = self.pair_page(None, cx.cfg, 200, stale);
                self.respond(&r, &req);
            }
            (None, "/whoami") => {
                let r = Response::new(401, "application/json", b"{\"paired\":false}".to_vec())
                    .with("Cache-Control", "no-store");
                self.respond(&r, &req);
            }
            (None, _) if get => self.respond(&Response::redirect("/"), &req),
            (Some(d), "/whoami") if get => {
                let r = Response::new(
                    200,
                    "application/json",
                    d.to_json().to_string().into_bytes(),
                )
                .with("Cache-Control", "no-store");
                self.respond(&r, &req);
            }
            (Some(_), "/pair") if req.method == "POST" => {
                self.respond(&Response::redirect("/"), &req)
            }
            (Some(_), path) if get => match cx.cfg.site.get(path) {
                Some(f) => {
                    let mut r = Response::new(200, f.content_type, f.body.clone()).with(
                        "Cache-Control",
                        if f.immutable {
                            "private, max-age=31536000, immutable"
                        } else {
                            "no-cache"
                        },
                    );
                    if f.content_type.starts_with("text/html") {
                        r = r.with("Content-Security-Policy", APP_CSP);
                    }
                    self.respond(&r, &req);
                }
                None => self.respond(&Response::text(404, "not found"), &req),
            },
            _ => self.respond(
                &Response::text(405, "not allowed").with("Allow", "GET, HEAD, POST"),
                &req,
            ),
        }
    }

    fn authed(&mut self, device: Option<Device>, cx: &mut Ctx) {
        let State::Waiting(_, Wait::Auth) = &self.state else {
            return;
        };
        let State::Waiting(req, _) = std::mem::replace(&mut self.state, State::Http) else {
            return;
        };
        self.route(req, device, cx);
        self.process(cx);
    }

    fn paired(&mut self, outcome: Result<(Device, String), String>, cx: &mut Ctx) {
        let State::Waiting(_, Wait::Pair) = &self.state else {
            return;
        };
        let State::Waiting(req, _) = std::mem::replace(&mut self.state, State::Http) else {
            return;
        };
        match outcome {
            Ok((device, token)) => {
                cx.log.line(&format!(
                    "device {} paired from {}, remote-only",
                    device.id, self.peer
                ));
                let r = Response::redirect("/").with(
                    "Set-Cookie",
                    format!("{COOKIE}={token}; Path=/; Secure; HttpOnly; SameSite=Strict; Max-Age={COOKIE_AGE}"),
                );
                self.respond(&r, &req);
            }
            Err(message) => {
                cx.log
                    .line(&format!("pairing from {}: {message}", self.peer));
                let r = self.pair_page(Some((&message, false)), cx.cfg, 200, false);
                self.respond(&r, &req);
            }
        }
        self.process(cx);
    }

    fn timed_out(&mut self) {
        match self.state {
            State::Waiting(..) => {
                self.state = State::Http;
                self.refuse(503, "the TV's lumen-ctl did not answer");
                self.deadline = None;
            }
            _ => self.dead = true,
        }
    }
}

/// The link to lumen-ctl: lines each way.
struct Ctl {
    sock: UnixStream,
    inbuf: Vec<u8>,
    out: Vec<u8>,
}

impl Ctl {
    /// Reads what is there; `Err` once lumen-ctl is gone.
    fn read(&mut self, lines: &mut Vec<String>) -> Result<(), String> {
        let mut buf = [0u8; 16 << 10];
        loop {
            match self.sock.read(&mut buf) {
                Ok(0) => return Err("lumen-ctl ended".to_owned()),
                Ok(n) => self.inbuf.extend_from_slice(&buf[..n]),
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => return Err(format!("lumen-ctl: {e}")),
            }
        }
        while let Some(at) = self.inbuf.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = self.inbuf.drain(..=at).collect();
            lines.push(String::from_utf8_lossy(&line[..at]).into_owned());
        }
        if self.inbuf.len() > MAX_CTL_LINE {
            return Err("lumen-ctl sent a line over 64 KiB".to_owned());
        }
        Ok(())
    }

    fn flush(&mut self) -> Result<(), String> {
        while !self.out.is_empty() {
            match self.sock.write(&self.out) {
                Ok(n) => {
                    self.out.drain(..n);
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => return Err(format!("lumen-ctl: {e}")),
            }
        }
        Ok(())
    }
}

/// Runs until a stop signal or until lumen-ctl ends; the exit code.
pub fn run(cfg: Config, listener: TcpListener, ctl: UnixStream, log: &mut Log) -> u8 {
    if let Err(e) = listener
        .set_nonblocking(true)
        .and_then(|()| ctl.set_nonblocking(true))
    {
        log.line(&format!("cannot go non-blocking: {e}"));
        return 1;
    }
    let mut ctl = Ctl {
        sock: ctl,
        inbuf: Vec::new(),
        out: Vec::new(),
    };
    let mut hub = Hub::new();
    let mut conns: HashMap<ConnId, Conn> = HashMap::new();
    let mut next_id: ConnId = 0;
    let mut actions: VecDeque<Action> = VecDeque::new();
    let mut refused = (0u64, None::<Instant>);

    loop {
        if let Some(signal) = sys::stop_signal() {
            log.line(&format!("stopped by {signal}"));
            return 0;
        }
        let order: Vec<ConnId> = conns.keys().copied().collect();
        let mut fds = vec![
            sys::pollfd(listener.as_raw_fd(), sys::POLLIN),
            sys::pollfd(
                ctl.sock.as_raw_fd(),
                sys::POLLIN | if ctl.out.is_empty() { 0 } else { sys::POLLOUT },
            ),
        ];
        for id in &order {
            let c = &conns[id];
            let out = if c.tls.wants_write() { sys::POLLOUT } else { 0 };
            fds.push(sys::pollfd(c.sock.as_raw_fd(), sys::POLLIN | out));
        }
        let now = Instant::now();
        let soonest = conns.values().filter_map(|c| c.deadline).min();
        let timeout = soonest.map_or(1000, |d| {
            d.saturating_duration_since(now).as_millis().min(1000) as i32
        });
        if let Err(e) = sys::poll(&mut fds, timeout) {
            log.line(&format!("poll: {e}"));
            return 1;
        }

        // lumen-ctl
        if fds[1].revents & (sys::POLLIN | sys::POLLERR_HUP) != 0 {
            let mut lines = Vec::new();
            let gone = ctl.read(&mut lines);
            for line in lines {
                actions.extend(hub.ctl_line(&line));
            }
            if let Err(e) = gone {
                log.line(&e);
                return 1;
            }
        }

        // New connections, from the home network only.
        if fds[0].revents & sys::POLLIN != 0 {
            loop {
                match listener.accept() {
                    Ok((sock, SocketAddr::V4(peer))) if cfg.allowed(*peer.ip()) => {
                        if conns.len() >= MAX_CONNS {
                            log.line(&format!(
                                "{} connections already: {} refused",
                                MAX_CONNS,
                                peer.ip()
                            ));
                            continue;
                        }
                        let tls = match ServerConnection::new(Arc::clone(&cfg.tls)) {
                            Ok(t) => t,
                            Err(e) => {
                                log.line(&format!("TLS: {e}"));
                                continue;
                            }
                        };
                        if sock.set_nonblocking(true).is_err() {
                            continue;
                        }
                        let _ = sock.set_nodelay(true);
                        sys::keepalive(sock.as_raw_fd());
                        next_id += 1;
                        conns.insert(
                            next_id,
                            Conn {
                                id: next_id,
                                sock,
                                tls,
                                peer: *peer.ip(),
                                inbuf: Vec::new(),
                                state: State::Http,
                                deadline: Some(Instant::now() + FIRST_REQUEST),
                                closing: false,
                                notified: false,
                                dead: false,
                                backlog: 0,
                            },
                        );
                    }
                    Ok((_, peer)) => {
                        // Outside --allow: closed unread, logged once a minute.
                        refused.0 += 1;
                        if refused
                            .1
                            .is_none_or(|t| t.elapsed() > Duration::from_secs(60))
                        {
                            log.line(&format!(
                                "refused {} connection(s) from outside --allow, last from {}",
                                refused.0,
                                peer.ip()
                            ));
                            refused = (0, Some(Instant::now()));
                        }
                    }
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                    Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                    Err(e) => {
                        log.line(&format!("accept: {e}"));
                        break;
                    }
                }
            }
        }

        // Connections with something to read.
        for (i, id) in order.iter().enumerate() {
            let rev = fds[2 + i].revents;
            if rev == 0 {
                continue;
            }
            let Some(c) = conns.get_mut(id) else { continue };
            if rev & (sys::POLLIN | sys::POLLERR_HUP) != 0 {
                c.read(log);
            }
            let mut cx = Ctx {
                cfg: &cfg,
                hub: &mut hub,
                actions: &mut actions,
                log,
            };
            c.process(&mut cx);
        }

        // What the hub asked for, which may ask for more.
        while let Some(action) = actions.pop_front() {
            match action {
                Action::Ctl(line) => {
                    if ctl.out.len() > MAX_CTL_OUT {
                        log.line("lumen-ctl is not reading: stopping");
                        return 1;
                    }
                    ctl.out.extend_from_slice(line.as_bytes());
                    ctl.out.push(b'\n');
                }
                Action::Send(id, line) => {
                    if let Some(c) = conns.get_mut(&id) {
                        c.send_ws(&line);
                    }
                }
                Action::Close(id, code, why) => {
                    if let Some(c) = conns.get_mut(&id) {
                        c.close_ws(code, why);
                    }
                }
                Action::Authed(id, device) => {
                    if let Some(c) = conns.get_mut(&id) {
                        let mut cx = Ctx {
                            cfg: &cfg,
                            hub: &mut hub,
                            actions: &mut actions,
                            log,
                        };
                        c.authed(device, &mut cx);
                    }
                }
                Action::Paired(id, outcome) => {
                    if let Some(c) = conns.get_mut(&id) {
                        let mut cx = Ctx {
                            cfg: &cfg,
                            hub: &mut hub,
                            actions: &mut actions,
                            log,
                        };
                        c.paired(outcome, &mut cx);
                    }
                }
                Action::Log(text) => log.line(&text),
            }
        }

        if let Err(e) = ctl.flush() {
            log.line(&e);
            return 1;
        }
        let now = Instant::now();
        for c in conns.values_mut() {
            if c.deadline.is_some_and(|d| d <= now) {
                c.timed_out();
            }
            c.flush();
        }
        conns.retain(|id, c| {
            if c.dead {
                if let State::Ws { device, .. } = &c.state {
                    log.line(&format!("device {}: WebSocket closed", device.id));
                }
                hub.closed(*id);
                false
            } else {
                true
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> Config {
        Config {
            names: vec!["192.168.86.20".into(), "lumen-tv.home.arpa".into()],
            port: 8443,
            allow: vec![Net::parse("192.168.86.0/24").unwrap()],
            tls: Arc::new(
                ServerConfig::builder_with_provider(Arc::new(
                    rustls::crypto::ring::default_provider(),
                ))
                .with_protocol_versions(&[&rustls::version::TLS13])
                .unwrap()
                .with_no_client_auth()
                .with_cert_resolver(Arc::new(rustls::server::ResolvesServerCertUsingSni::new())),
            ),
            site: Site::from(&[("/index.html", "x")]),
            pair_csp: String::new(),
        }
    }

    fn req(host: &str, origin: Option<&str>) -> Request {
        let mut headers = vec![("host".to_owned(), host.to_owned())];
        if let Some(o) = origin {
            headers.push(("origin".to_owned(), o.to_owned()));
        }
        Request {
            method: "POST".into(),
            path: "/pair".into(),
            http11: true,
            headers,
            body: Vec::new(),
        }
    }

    #[test]
    fn answers_only_to_the_tvs_names_and_origin() {
        let c = cfg();
        assert!(c.host(&req("192.168.86.20:8443", None)).is_some());
        assert!(c.host(&req("LUMEN-TV.home.arpa:8443", None)).is_some());
        for bad in [
            "192.168.86.20",
            "192.168.86.20:443",
            "evil.example:8443",
            "127.0.0.1:8443",
        ] {
            assert!(c.host(&req(bad, None)).is_none(), "{bad}");
        }
        let host = "192.168.86.20:8443";
        assert!(c.same_origin(&req(host, Some("https://192.168.86.20:8443")), host));
        for bad in [
            None,
            Some("http://192.168.86.20:8443"),
            Some("https://evil.example"),
            Some("null"),
        ] {
            assert!(!c.same_origin(&req(host, bad), host), "{bad:?}");
        }
        assert!(c.allowed("192.168.86.7".parse().unwrap()));
        assert!(!c.allowed("10.0.0.7".parse().unwrap()));
    }

    #[test]
    fn tokens_are_64_hex_digits() {
        assert!(is_token(&"a1".repeat(32)));
        assert!(!is_token(&"A1".repeat(32)) && !is_token("abc") && !is_token(&"g".repeat(64)));
    }
}
