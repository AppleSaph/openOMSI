//! Positional voice in multiplayer through GreenTeaSpeak, the way SaltyChat does it for FiveM.
//!
//! GreenTeaSpeak 2 (a TeamSpeak-style voice client, <https://greenteaspeak.de>) runs plugins
//! with a 3D voice API (`ctx.voice.setListenerPose` / `setClientPose`). openOMSI's plugin
//! (`tools/greenteaspeak-plugin`) listens on `127.0.0.1:38088`; the game connects to it and
//! sends one JSON object a line:
//!
//! - `initiate`: the voice server the session uses (its unique id, the in-game channel and its
//!   password, the voice range) and the nickname to take there. The plugin moves the user
//!   into that channel, renames them and switches 3D voice on - only on the server whose
//!   unique id the session names, never on another one the user happens to be on.
//! - `self`: where the listener (the camera) is and which way it looks, ten times a second.
//! - `players`: every other player who can be heard: nickname, where their head is, their
//!   range, and a volume when the sound is muffled (one of the two sits in a bus).
//! - `reset`: the session is over: 3D voice off.
//!
//! The plugin answers with `state` (connected to the voice server, in the channel), `talk`
//! (who speaks now: drawn by their name tags) and `mute`.
//!
//! The voice server is the host's to name: `voice_server_uid`, `voice_channel` and the rest in
//! a dedicated server's `server.cfg`, or `~/.openomsi/voice.cfg` for a game that hosts. A
//! joining game asks for it with the command `voice?` and the host answers `voice …`.
//!
//! Players are told apart in the voice channel by their nicknames: `<name> #<id>` with the
//! session's player id, which every game of the session derives the same way.

use glam::DVec3;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::time::{Duration, Instant};

/// The port of the plugin (SaltyChat's WebSocket port, for those who know it).
pub(crate) const DEFAULT_PORT: u16 = 38088;
/// How far a player is heard (m) when the host does not say.
pub(crate) const DEFAULT_RANGE: f32 = 20.0;
/// TeamSpeak's limit on a nickname's length (characters).
const MAX_NICK: usize = 30;
/// Inside a bus with the other one outside it (or in another bus): heard through the
/// bodywork at this share of the open-air loudness.
const MUFFLED: f32 = 0.35;
const SEND_EVERY: f32 = 0.1;
const ASK_EVERY: f32 = 4.0;
const ASK_TRIES: u32 = 8;
#[cfg(not(test))]
const RECONNECT: Duration = Duration::from_secs(3);
#[cfg(test)]
const RECONNECT: Duration = Duration::from_millis(20);

/// The voice server a session uses, as its host names it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct VoiceServer {
    /// The voice server's unique id (the one in its info panel; empty: whichever server
    /// the player is on).
    pub server_uid: String,
    /// The in-game channel: its id or its name.
    pub channel: String,
    pub password: String,
    /// How far a player is heard (m).
    pub range: f32,
}

impl VoiceServer {
    /// From `key = value` pairs (keys lower case): `voice_server_uid`, `voice_channel`,
    /// `voice_channel_password`, `voice_range`. None without a channel.
    pub(crate) fn from_kv(get: impl Fn(&str) -> Option<String>) -> Option<VoiceServer> {
        let channel = get("voice_channel").map(|c| c.trim().to_string()).filter(|c| !c.is_empty())?;
        Some(VoiceServer {
            server_uid: get("voice_server_uid").map(|v| v.trim().to_string()).unwrap_or_default(),
            channel,
            password: get("voice_channel_password").map(|v| v.trim().to_string()).unwrap_or_default(),
            range: get("voice_range").and_then(|v| v.trim().parse::<f32>().ok()).filter(|r| r.is_finite()).map(|r| r.clamp(2.0, 200.0)).unwrap_or(DEFAULT_RANGE),
        })
    }

    /// `~/.openomsi/voice.cfg` of a game that hosts (the same keys as `server.cfg`).
    pub(crate) fn host_file() -> Option<VoiceServer> {
        let text = std::fs::read_to_string(crate::lan::data_dir()?.join("voice.cfg")).ok()?;
        let kv = parse_kv(&text);
        VoiceServer::from_kv(|k| kv.get(k).cloned())
    }

    /// The host's answer to `voice?`.
    pub(crate) fn command(server: Option<&VoiceServer>) -> String {
        match server {
            Some(s) => format!("voice {} {} {} {}", s.range, enc(&s.server_uid), enc(&s.channel), enc(&s.password)),
            None => "voice -".into(),
        }
    }

    /// The host's answer as a joining game reads it: Some(None) for a session without voice.
    pub(crate) fn parse_command(text: &str) -> Option<Option<VoiceServer>> {
        let rest = text.strip_prefix("voice ")?.trim();
        if rest == "-" {
            return Some(None);
        }
        let mut f = rest.split(' ');
        let range = f.next()?.parse::<f32>().ok().filter(|r| r.is_finite())?.clamp(2.0, 200.0);
        let (uid, channel, password) = (dec(f.next()?), dec(f.next()?), dec(f.next().unwrap_or("")));
        if channel.is_empty() {
            return Some(None);
        }
        Some(Some(VoiceServer { server_uid: uid, channel, password, range }))
    }
}

/// The voice server this game names when it hosts: a dedicated server's `server.cfg`, else
/// `~/.openomsi/voice.cfg`.
pub(crate) fn hosted() -> Option<VoiceServer> {
    match crate::server::SERVER_VOICE.get() {
        Some(v) => v.clone(),
        None => VoiceServer::host_file(),
    }
}

fn parse_kv(text: &str) -> std::collections::HashMap<String, String> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| l.split_once('='))
        .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string()))
        .collect()
}

/// A command's field: no spaces, no '|' (the LAN messages' separator), '-' for empty.
fn enc(s: &str) -> String {
    if s.is_empty() {
        return "-".into();
    }
    let mut out = String::new();
    for c in s.chars() {
        match c {
            '%' | ' ' | '|' | '-' => out.push_str(&format!("%{:02X}", c as u32)),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out
}

fn dec(s: &str) -> String {
    if s == "-" {
        return String::new();
    }
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Some(v) = std::str::from_utf8(&b[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// A player's nickname in the voice channel: their name (letters, digits and a few signs)
/// and their id in the session, within TeamSpeak's 30 characters.
pub(crate) fn nickname(name: &str, id: u32) -> String {
    let tail = format!(" #{id}");
    let clean: String = name
        .chars()
        .map(|c| if c.is_alphanumeric() || matches!(c, '-' | '_' | '.' | '\'') { c } else { ' ' })
        .collect();
    let clean = clean.split_whitespace().collect::<Vec<_>>().join(" ");
    let clean = if clean.is_empty() { "Driver".to_string() } else { clean };
    let room = MAX_NICK.saturating_sub(tail.chars().count());
    let short: String = clean.chars().take(room).collect();
    format!("{}{tail}", short.trim_end())
}

/// What the plugin said.
#[derive(Debug, Clone, PartialEq)]
enum Event {
    /// The link to the plugin was made (again): it needs the session told afresh.
    Linked,
    Unlinked,
    Line(Value),
}

/// The link to the plugin: a thread keeps a TCP connection to it, made again whenever it
/// breaks (the plugin starts after the game, GreenTeaSpeak is restarted).
struct Link {
    tx: Sender<String>,
    rx: Receiver<Event>,
}

impl Link {
    fn start(addr: SocketAddr) -> Option<Link> {
        let (tx, out_rx) = mpsc::channel::<String>();
        let (ev_tx, rx) = mpsc::channel::<Event>();
        std::thread::Builder::new()
            .name("voice".into())
            .spawn(move || link_thread(addr, out_rx, ev_tx))
            .ok()?;
        Some(Link { tx, rx })
    }
}

fn link_thread(addr: SocketAddr, out: Receiver<String>, events: Sender<Event>) {
    let mut told_missing = false;
    loop {
        let stream = match TcpStream::connect_timeout(&addr, Duration::from_millis(500)) {
            Ok(s) => s,
            Err(e) => {
                if !told_missing {
                    log::info!("voice: no GreenTeaSpeak plugin at {addr} ({e}); trying again every few seconds");
                    told_missing = true;
                }
                // (lines for a plugin that is not there are dropped; the game stops the
                // thread by dropping its sender)
                let until = Instant::now() + RECONNECT;
                while Instant::now() < until {
                    match out.try_recv() {
                        Ok(_) => {}
                        Err(TryRecvError::Disconnected) => return,
                        Err(TryRecvError::Empty) => std::thread::sleep(Duration::from_millis(20)),
                    }
                }
                continue;
            }
        };
        told_missing = false;
        log::info!("voice: linked to the GreenTeaSpeak plugin at {addr}");
        let _ = stream.set_nodelay(true);
        let _ = stream.set_read_timeout(Some(Duration::from_millis(20)));
        if events.send(Event::Linked).is_err() {
            return;
        }
        let mut writer = match stream.try_clone() {
            Ok(w) => w,
            Err(_) => continue,
        };
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        let alive = 'link: loop {
            // what the game sends
            loop {
                match out.try_recv() {
                    Ok(mut l) => {
                        l.push('\n');
                        if writer.write_all(l.as_bytes()).is_err() {
                            break 'link true;
                        }
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => break 'link false,
                }
            }
            // what the plugin says
            match reader.read_line(&mut line) {
                Ok(0) => break 'link true,
                Ok(_) => {
                    if let Ok(v) = serde_json::from_str::<Value>(line.trim()) {
                        if events.send(Event::Line(v)).is_err() {
                            break 'link false;
                        }
                    }
                    line.clear();
                }
                Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
                Err(_) => break 'link true,
            }
        };
        let _ = events.send(Event::Unlinked);
        if !alive {
            return;
        }
        log::info!("voice: the GreenTeaSpeak plugin went away");
        std::thread::sleep(RECONNECT);
    }
}

/// What the game knows of the voice chat.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Status {
    /// The plugin is there.
    pub linked: bool,
    /// The plugin is on the session's voice server, in its channel.
    pub in_channel: bool,
    /// What went wrong, as the plugin says (wrong server, no such channel, ...).
    pub problem: Option<String>,
    pub mic_muted: bool,
}

/// A player as the voice chat places them, each frame.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Speaker {
    pub id: u32,
    pub name: String,
    /// Where their head is.
    pub at: DVec3,
    /// Which bus they are in (its player's id), None out in the open.
    pub inside: Option<u32>,
}

/// Where we hear from: the camera, and which bus it is in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Listener {
    pub at: DVec3,
    /// Degrees, 0 = north, clockwise (the camera's yaw).
    pub yaw: f32,
    pub inside: Option<u32>,
}

/// The game's side of the voice chat during a session.
pub(crate) struct Voice {
    link: Option<Link>,
    addr: SocketAddr,
    /// The host's voice server: None until it answered, Some(None) for none.
    server: Option<Option<VoiceServer>>,
    asked: u32,
    ask_t: f32,
    send_t: f32,
    /// What `initiate` last told the plugin (sent again when it changes or the link is new).
    initiated: Option<(VoiceServer, String)>,
    /// The positions go to the plugin relative to this point: a map's coordinates run into
    /// the millions of metres, more than a 32-bit float of an audio engine keeps to the
    /// centimetre.
    origin: Option<DVec3>,
    /// Who is speaking now, by nickname.
    pub talking: hashbrown::HashSet<String>,
    pub status: Status,
}

impl Voice {
    /// The voice chat of a session (`port` 0: the default).
    pub(crate) fn new(port: u16) -> Voice {
        let port = if port == 0 { DEFAULT_PORT } else { port };
        Voice {
            link: None,
            addr: SocketAddr::from(([127, 0, 0, 1], port)),
            server: None,
            asked: 0,
            ask_t: 0.0,
            send_t: 0.0,
            initiated: None,
            origin: None,
            talking: Default::default(),
            status: Status::default(),
        }
    }

    /// The host's voice server is known (we host, or its answer came).
    pub(crate) fn set_server(&mut self, server: Option<VoiceServer>) {
        if self.server.as_ref() != Some(&server) {
            match &server {
                Some(s) => log::info!("voice: the session talks on voice server '{}' channel '{}' (range {} m)", s.server_uid, s.channel, s.range),
                None => log::info!("voice: the session names no voice server"),
            }
        }
        self.server = Some(server);
    }

    /// The voice server, once known.
    pub(crate) fn server(&self) -> Option<&VoiceServer> {
        self.server.as_ref().and_then(|s| s.as_ref())
    }

    /// Ask the host for its voice server now and then until it answers (an older host
    /// never does: given up after a while). True when it is time to ask.
    pub(crate) fn should_ask(&mut self, dt: f32) -> bool {
        if self.server.is_some() || self.asked >= ASK_TRIES {
            return false;
        }
        self.ask_t -= dt;
        if self.ask_t > 0.0 {
            return false;
        }
        self.ask_t = ASK_EVERY;
        self.asked += 1;
        true
    }

    fn send(&self, v: Value) {
        if let Some(l) = &self.link {
            let _ = l.tx.send(v.to_string());
        }
    }

    /// Once a frame: take in what the plugin said, tell it the session, where we are and
    /// where the others are.
    pub(crate) fn tick(&mut self, dt: f32, me: (&str, u32), listener: Option<Listener>, others: &[Speaker]) {
        let Some(server) = self.server().cloned() else { return };
        if self.link.is_none() {
            self.link = Link::start(self.addr);
        }
        let mut relink = false;
        let events: Vec<Event> = self.link.as_ref().map(|l| l.rx.try_iter().collect()).unwrap_or_default();
        for e in events {
            match e {
                Event::Linked => relink = true,
                Event::Unlinked => {
                    relink = false;
                    self.status = Status::default();
                    self.talking.clear();
                }
                Event::Line(v) => self.on_line(&v),
            }
        }
        if relink {
            self.status.linked = true;
            self.initiated = None;
        }
        if !self.status.linked {
            return;
        }
        let nick = nickname(me.0, me.1);
        if self.initiated.as_ref() != Some(&(server.clone(), nick.clone())) {
            self.send(json!({
                "type": "initiate",
                "game": "openOMSI",
                "protocol": 1,
                "serverUid": server.server_uid,
                "channel": server.channel,
                "password": server.password,
                "nickname": nick,
                "range": server.range,
            }));
            self.initiated = Some((server.clone(), nick));
        }
        self.send_t -= dt;
        if self.send_t > 0.0 {
            return;
        }
        self.send_t = SEND_EVERY;
        let Some(me) = listener else { return };
        let origin = *self.origin.get_or_insert_with(|| (me.at / 1000.0).round() * 1000.0);
        let rel = |p: DVec3| {
            let r = p - origin;
            // (to the centimetre: the messages stay short)
            [(r.x * 100.0).round() / 100.0, (r.y * 100.0).round() / 100.0, (r.z * 100.0).round() / 100.0]
        };
        let [x, y, z] = rel(me.at);
        self.send(json!({ "type": "self", "x": x, "y": y, "z": z, "yaw": saltychat_yaw(me.yaw) }));
        let players: Vec<Value> = others
            .iter()
            .map(|o| {
                let [x, y, z] = rel(o.at);
                let volume = heard_volume(&me, o, server.range);
                json!({ "nickname": nickname(&o.name, o.id), "x": x, "y": y, "z": z, "range": server.range, "volume": volume })
            })
            .collect();
        self.send(json!({ "type": "players", "players": players }));
    }

    fn on_line(&mut self, v: &Value) {
        let s = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
        let b = |k: &str| v.get(k).and_then(|x| x.as_bool()).unwrap_or(false);
        match v.get("type").and_then(|t| t.as_str()).unwrap_or("") {
            "state" => {
                let was = self.status.clone();
                self.status.in_channel = b("inChannel");
                self.status.problem = Some(s("error")).filter(|e| !e.is_empty());
                if was.in_channel != self.status.in_channel || was.problem != self.status.problem {
                    match &self.status.problem {
                        Some(p) => log::warn!("voice: {p}"),
                        None if self.status.in_channel => log::info!("voice: in the session's voice channel"),
                        None => log::info!("voice: not in the session's voice channel"),
                    }
                }
            }
            "talk" => {
                let n = s("nickname");
                if b("talking") {
                    self.talking.insert(n);
                } else {
                    self.talking.remove(&n);
                }
            }
            "mute" => self.status.mic_muted = b("microphoneMuted"),
            _ => {}
        }
    }

    /// Is this player speaking now?
    pub(crate) fn speaks(&self, name: &str, id: u32) -> bool {
        !self.talking.is_empty() && self.talking.contains(&nickname(name, id))
    }

    /// The HUD's line, when there is something to say.
    pub(crate) fn hud_line(&self) -> Option<String> {
        self.server()?;
        if !self.status.linked {
            return Some("Voice: start GreenTeaSpeak with the openOMSI plugin to talk".into());
        }
        if let Some(p) = &self.status.problem {
            return Some(format!("Voice: {p}"));
        }
        if !self.status.in_channel {
            return Some("Voice: joining the voice channel ...".into());
        }
        self.status.mic_muted.then(|| "Voice: your microphone is muted".into())
    }
}

impl Drop for Voice {
    fn drop(&mut self) {
        // (the plugin switches 3D voice off; the thread ends when the sender goes)
        self.send(json!({ "type": "reset" }));
    }
}

/// The camera's yaw as SaltyChat's `Rotation` (and GreenTeaSpeak's `yawDeg`) has it: GTA's
/// heading, 0 = north and counter-clockwise, -180..180.
fn saltychat_yaw(yaw: f32) -> f32 {
    let r = (-yaw + 180.0).rem_euclid(360.0) - 180.0;
    (r * 10.0).round() / 10.0
}

/// A player heard through a bus's bodywork: the volume (0..1) the plugin is to use instead
/// of its own fall-off with distance; None in the open (or both in the same bus).
fn heard_volume(me: &Listener, o: &Speaker, range: f32) -> Option<f32> {
    if me.inside == o.inside {
        return None;
    }
    let d = (o.at - me.at).length() as f32;
    let open = (1.0 - d / range.max(1.0)).clamp(0.0, 1.0);
    Some((open * MUFFLED * 100.0).round() / 100.0)
}

/// The other players as the voice chat places them: their head on foot, the driver's
/// place in their bus, the bus they ride in.
pub(crate) fn speakers(lan: &omsi_net::LanSession, game: &crate::lan::LanGame, my_bus: Option<DVec3>) -> Vec<Speaker> {
    let bus_of = |id: u32| -> Option<DVec3> {
        if id == lan.my_id {
            return my_bus;
        }
        game.remotes.get(&id).map(|r| r.vehicle().position).or_else(|| lan.peers().find(|p| p.pose.id == id).map(|p| DVec3::new(p.pose.x, p.pose.y, p.pose.z)))
    };
    let mut out = Vec::new();
    for p in lan.peers().filter(|p| p.has_pose && p.pose.id != lan.my_id) {
        let pose = &p.pose;
        let (at, inside) = match pose.walker {
            Some(w) => match w.aboard {
                Some(a) => (bus_of(a.owner).map(|b| b + DVec3::new(0.0, 0.0, 1.9)).unwrap_or(DVec3::new(w.x, w.y, w.z + 1.6)), Some(a.owner)),
                None => (DVec3::new(w.x, w.y, w.z + 1.6), None),
            },
            None if pose.has_vehicle() => (bus_of(pose.id).unwrap_or(DVec3::new(pose.x, pose.y, pose.z)) + DVec3::new(0.0, 0.0, 1.9), Some(pose.id)),
            // (a dedicated server's own place in the session: nobody there)
            None => continue,
        };
        out.push(Speaker { id: pose.id, name: pose.name.clone(), at, inside });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::net::TcpListener;

    #[test]
    fn the_hosts_answer_round_trips() {
        let s = VoiceServer { server_uid: "abc+/=Def".into(), channel: "OMSI - In game".into(), password: "p|w 100%".into(), range: 25.0 };
        let c = VoiceServer::command(Some(&s));
        assert!(!c.contains('|') && c.len() < omsi_net::MAX_CHAT);
        assert_eq!(VoiceServer::parse_command(&c), Some(Some(s)));
        let open = VoiceServer { server_uid: String::new(), channel: "12".into(), password: String::new(), range: 20.0 };
        assert_eq!(VoiceServer::parse_command(&VoiceServer::command(Some(&open))), Some(Some(open)));
        assert_eq!(VoiceServer::parse_command(&VoiceServer::command(None)), Some(None));
        assert_eq!(VoiceServer::parse_command("trigger x"), None);
    }

    #[test]
    fn server_cfg_keys() {
        let kv = parse_kv("# x\nvoice_server_uid = AbC=\nvoice_channel = 5\nvoice_range = 1000\n");
        let s = VoiceServer::from_kv(|k| kv.get(k).cloned()).unwrap();
        assert_eq!((s.server_uid.as_str(), s.channel.as_str(), s.range), ("AbC=", "5", 200.0));
        assert!(VoiceServer::from_kv(|_| None).is_none());
    }

    #[test]
    fn nicknames_fit_and_tell_players_apart() {
        assert_eq!(nickname("Max Mustermann", 3), "Max Mustermann #3");
        assert_eq!(nickname("  a|b\tc ", 12), "a b c #12");
        assert_eq!(nickname("", 2), "Driver #2");
        let long = nickname("Maximilian Alexander von Mustermannshausen", 1234);
        assert!(long.chars().count() <= MAX_NICK && long.ends_with(" #1234"), "{long}");
    }

    #[test]
    fn yaw_is_gtas_heading() {
        assert_eq!(saltychat_yaw(0.0), 0.0);
        assert_eq!(saltychat_yaw(90.0), -90.0); // east: GTA -90
        assert_eq!(saltychat_yaw(270.0), 90.0);
        assert_eq!(saltychat_yaw(-90.0), 90.0);
    }

    #[test]
    fn a_bus_muffles_the_voice() {
        let me = Listener { at: DVec3::ZERO, yaw: 0.0, inside: Some(1) };
        let near = Speaker { id: 2, name: "b".into(), at: DVec3::new(10.0, 0.0, 0.0), inside: None };
        assert_eq!(heard_volume(&me, &near, 20.0), Some(0.18));
        let aboard = Speaker { inside: Some(1), ..near.clone() };
        assert_eq!(heard_volume(&me, &aboard, 20.0), None);
    }

    #[test]
    fn the_plugin_is_told_the_session_and_the_positions() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let mut v = Voice::new(port);
        v.set_server(Some(VoiceServer { server_uid: "UID".into(), channel: "7".into(), password: String::new(), range: 20.0 }));
        let me = Listener { at: DVec3::new(1_234_567.0, -2_000_000.0, 50.0), yaw: 90.0, inside: None };
        let others = [Speaker { id: 2, name: "Anna".into(), at: DVec3::new(1_234_570.0, -2_000_000.0, 51.6), inside: None }];
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut conn = None;
        listener.set_nonblocking(true).unwrap();
        while conn.is_none() && Instant::now() < deadline {
            v.tick(0.2, ("Max", 1), Some(me), &others);
            if let Ok((c, _)) = listener.accept() {
                conn = Some(c);
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let mut conn = conn.expect("the game links to the plugin");
        conn.write_all(b"{\"type\":\"state\",\"inChannel\":true}\n{\"type\":\"talk\",\"nickname\":\"Anna #2\",\"talking\":true}\n").unwrap();
        conn.set_read_timeout(Some(Duration::from_millis(50))).unwrap();
        let mut got = String::new();
        while Instant::now() < deadline && !got.contains("\"players\"") {
            v.tick(0.2, ("Max", 1), Some(me), &others);
            let mut buf = [0u8; 4096];
            if let Ok(n) = conn.read(&mut buf) {
                got.push_str(&String::from_utf8_lossy(&buf[..n]));
            }
        }
        let lines: Vec<Value> = got.lines().filter_map(|l| serde_json::from_str(l).ok()).collect();
        let init = lines.iter().find(|l| l["type"] == "initiate").expect("initiate");
        assert_eq!((init["serverUid"].as_str(), init["channel"].as_str(), init["nickname"].as_str()), (Some("UID"), Some("7"), Some("Max #1")));
        let me_line = lines.iter().find(|l| l["type"] == "self").expect("self");
        assert_eq!((me_line["x"].as_f64(), me_line["yaw"].as_f64()), (Some(-433.0), Some(-90.0)));
        let players = lines.iter().find(|l| l["type"] == "players").expect("players");
        assert_eq!(players["players"][0]["nickname"], "Anna #2");
        assert_eq!(players["players"][0]["x"].as_f64(), Some(-430.0));
        assert!(players["players"][0]["volume"].is_null());
        // what the plugin said came in
        let until = Instant::now() + Duration::from_secs(2);
        while Instant::now() < until && !(v.status.in_channel && v.speaks("Anna", 2)) {
            v.tick(0.01, ("Max", 1), Some(me), &others);
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(v.status.in_channel);
        assert!(v.speaks("Anna", 2));
        assert_eq!(v.hud_line(), None);
    }
}
