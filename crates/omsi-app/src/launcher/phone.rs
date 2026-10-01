//! The launcher made for a phone (or a tablet), not the desktop's pages squeezed onto it:
//! a tab bar at the foot (Play, Online, Mods, More), a Play screen with the bus large and
//! the duty as four big cards over one Start button, and every choice made on a sheet of
//! its own that fills the screen - a list of big rows, a search field, a way back.
//! The pages a phone needs less often (Settings, Controls, Profile, …) open from More,
//! full width, under a bar with a way back.

use super::theme::*;
use super::ui::{id_of, ButtonKind};
use super::{Launcher, Page};
use glam::Vec2;
use omsi_ui::paint::Align;
use omsi_ui::{Color, Rect, Weight};

/// Height of the tab bar.
const TAB_H: f32 = 58.0;
/// Height of a sheet's rows.
const ROW_H: f32 = 62.0;
/// Height of a sheet's bar.
const BAR_H: f32 = 54.0;

#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum Tab {
    #[default]
    Play,
    Online,
    Mods,
    More,
}

const TABS: [(Tab, &str, &str); 4] = [(Tab::Play, "Play", "directions_bus"), (Tab::Online, "Online", "groups"), (Tab::Mods, "Mods", "extension"), (Tab::More, "More", "menu")];

/// A choice made on a sheet of its own.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Sheet {
    Map,
    Bus,
    Livery,
    Duty,
    Tour,
    Time,
}

/// The pages More opens.
const MORE: [(Page, &str, &str, &str); 7] = [
    (Page::Profile, "Profile", "badge", "Your driver, level and records"),
    (Page::Settings, "Settings", "tune", "Graphics, sound, gameplay"),
    (Page::Controls, "Controls", "sports_esports", "Touch, wheels and gamepads"),
    (Page::Sessions, "Sessions", "terminal", "Games running and their logs"),
    (Page::Tutorials, "Tutorials", "help", "Learn to drive the buses"),
    (Page::Timetable, "Timetable", "schedule", "The map's lines and trips"),
    (Page::Setup, "Setup", "folder_open", "The OMSI 2 folder and content"),
];

#[derive(Default)]
pub struct PhoneView {
    pub tab: Tab,
    pub sheet: Option<Sheet>,
    /// A page of More that is open.
    pub page: Option<Page>,
    pub filter: String,
    pub code: String,
}

/// The whole launcher on a phone (instead of the rail and the desktop pages).
pub fn draw(l: &mut Launcher) {
    let size = l.ui.size;
    let full = Rect::new(0.0, 0.0, size.x, size.y);
    l.ui.p().rect(full, RAIL);
    if let Some(s) = l.phone.sheet {
        sheet(l, s, full);
        return;
    }
    let body = Rect::new(0.0, 0.0, size.x, size.y - TAB_H);
    match (l.phone.tab, l.phone.page) {
        (Tab::More, Some(p)) => embedded(l, p, body, true),
        (Tab::More, None) => more(l, body),
        (Tab::Mods, _) => embedded(l, Page::Mods, body, false),
        (Tab::Online, _) => online(l, body),
        (Tab::Play, _) => play(l, body),
    }
    tab_bar(l, Rect::new(0.0, size.y - TAB_H, size.x, TAB_H));
    toast(l, body);
}

fn tab_bar(l: &mut Launcher, r: Rect) {
    l.ui.solid(r);
    l.ui.p().rect(r, PANEL);
    l.ui.p().rect(Rect::new(r.x, r.y, r.w, 1.0), EDGE);
    let w = r.w / TABS.len() as f32;
    let running = l.state.instances.iter().filter(|i| i.running).count();
    for (k, (tab, name, icon)) in TABS.iter().enumerate() {
        let cell = Rect::new(r.x + w * k as f32, r.y, w, r.h);
        let (_, down, clicked) = l.ui.interact(id_of(&format!("ptab-{name}")), cell);
        if clicked {
            l.phone.tab = *tab;
            l.phone.page = None;
            l.page_scroll = 0.0;
            match tab {
                Tab::Mods => l.go(Page::Mods),
                Tab::Play => l.go(Page::Drive),
                Tab::Online => l.go(Page::Multiplayer),
                Tab::More => {}
            }
        }
        let on = l.phone.tab == *tab;
        if down {
            l.ui.p().rounded(cell.pad(10.0, 6.0), 12.0, HOVER);
        }
        let c = if on { ACCENT } else { TEXT_DIM };
        if on {
            l.ui.p().rounded(Rect::new(cell.center().x - 28.0, cell.y + 7.0, 56.0, 28.0), 14.0, ACCENT.alpha(0.16));
        }
        l.ui.icon(icon, Vec2::new(cell.center().x, cell.y + 21.0), 22.0, c);
        l.ui.text_in(name, Rect::new(cell.x, cell.y + 36.0, cell.w, 16.0), 11.5, if on { Weight::Bold } else { Weight::Medium }, c, Align::Center);
        if *tab == Tab::More && running > 0 {
            l.ui.p().circle(Vec2::new(cell.center().x + 14.0, cell.y + 12.0), 5.0, OK);
        }
    }
}

/// The status line as a toast over the page's foot.
fn toast(l: &mut Launcher, body: Rect) {
    let (text, err, at) = l.state.status.clone();
    if text.is_empty() {
        return;
    }
    let fade = if err { (1.0 - (at.elapsed().as_secs_f32() - 10.0) / 1.5).clamp(0.0, 1.0) } else { (1.0 - (at.elapsed().as_secs_f32() - 4.0) / 1.0).clamp(0.0, 1.0) };
    if fade <= 0.0 {
        return;
    }
    let first = text.lines().next().unwrap_or("").to_string();
    let w = (l.ui.width(&first, 13.0, Weight::Medium) + 44.0).min(body.w - 32.0);
    let r = Rect::new(body.center().x - w * 0.5, body.bottom() - 46.0, w, 36.0);
    l.ui.p().rounded(r, 18.0, Color::rgba(40, 40, 40, 0.96 * fade));
    l.ui.icon(if err { "error" } else { "info" }, Vec2::new(r.x + 18.0, r.center().y), 16.0, (if err { DANGER } else { TEXT_DIM }).alpha(fade));
    l.ui.text_in(&first, Rect::new(r.x + 32.0, r.y, r.w - 42.0, r.h), 13.0, Weight::Medium, TEXT.alpha(fade), Align::Left);
}

/// A card of the Play screen: what is chosen, and the way to its sheet.
fn card(l: &mut Launcher, name: &str, r: Rect, icon: &str, label: &str, value: &str, warn: bool) -> bool {
    let (h, down, clicked) = l.ui.interact(id_of(name), r);
    l.ui.p().rounded(r, 14.0, if down { SELECTED } else if h { HOVER } else { FIELD });
    let ic = Vec2::new(r.x + 26.0, r.center().y);
    l.ui.p().circle(ic, 17.0, Color::rgba(255, 255, 255, 0.05));
    l.ui.icon(icon, ic, 19.0, if warn { WARN } else { ACCENT });
    l.ui.text_in(label, Rect::new(r.x + 54.0, r.y + 8.0, r.w - 84.0, 15.0), 11.0, Weight::Medium, TEXT_DIM, Align::Left);
    l.ui.text_in(value, Rect::new(r.x + 54.0, r.y + 24.0, r.w - 84.0, r.h - 30.0), 14.5, Weight::Bold, TEXT, Align::Left);
    l.ui.icon("chevron_right", Vec2::new(r.right() - 18.0, r.center().y), 20.0, TEXT_FAINT);
    clicked
}

fn duty_text(l: &Launcher) -> String {
    match (&l.state.choice.line, &l.state.choice.tour, l.state.choice.free) {
        (_, _, true) | (None, _, _) => "Free drive".into(),
        (Some(line), Some(t), _) => format!("Line {line} · tour {t}"),
        (Some(line), None, _) => format!("Line {line} · choose a tour"),
    }
}

fn time_text(l: &Launcher) -> String {
    let (y, m, d) = super::ui::parse_date(&l.state.choice.date);
    let weather = match l.state.choice.weather.strip_prefix("metar:") {
        Some(c) => format!("live {c}"),
        None if l.state.choice.weather == "cycle" => "weather cycle".into(),
        None => l.state.weathers.iter().find(|w| w.file == l.state.choice.weather).map(|w| w.name.clone()).unwrap_or_else(|| "map weather".into()),
    };
    format!("{:02}:{:02} · {d} {} {y} · {weather}", l.state.choice.time / 60, l.state.choice.time % 60, &super::ui::MONTHS[(m as usize).clamp(1, 12) - 1][..3])
}

fn play(l: &mut Launcher, body: Rect) {
    let pad = 12.0;
    let inner = body.pad(pad, pad);
    let wide = inner.w > 1.35 * inner.h;
    // the bus: the left of a phone held sideways, the top of one held upright
    let (pr, col) = if wide {
        let pw = (inner.w * 0.52).round();
        (Rect::new(inner.x, inner.y, pw, inner.h), Rect::new(inner.x + pw + pad, inner.y, inner.w - pw - pad, inner.h))
    } else {
        let ph = (inner.h * 0.4).round();
        (Rect::new(inner.x, inner.y, inner.w, ph), Rect::new(inner.x, inner.y + ph + pad, inner.w, inner.h - ph - pad))
    };
    l.preview(pr);
    // the bus's name over the foot of its picture, the livery as a chip beside it
    let shade = Rect::new(pr.x, pr.bottom() - 58.0, pr.w, 58.0);
    l.ui.p().rounded(shade, RADIUS, Color::rgba(0, 0, 0, 0.55));
    let (name, maker) = l.state.bus().map(|b| (b.name.clone(), b.manufacturer.clone())).unwrap_or_else(|| ("Choose a bus".into(), String::new()));
    l.ui.text_in(&maker, Rect::new(shade.x + 14.0, shade.y + 7.0, shade.w - 150.0, 15.0), 11.0, Weight::Medium, TEXT_DIM, Align::Left);
    l.ui.text_in(&name, Rect::new(shade.x + 14.0, shade.y + 22.0, shade.w - 150.0, 26.0), 17.0, Weight::Bold, TEXT, Align::Left);
    let paints = l.state.bus().map(|b| b.paints.len()).unwrap_or(0);
    if paints > 0 {
        let label = if l.state.choice.paint.is_empty() { l.state.bus().map(super::drive::default_livery_label).unwrap_or("Default paint").to_string() } else { l.state.choice.paint.clone() };
        let cw = (l.ui.width(&label, 12.0, Weight::Medium) + 40.0).min(170.0);
        let chip = Rect::new(shade.right() - cw - 10.0, shade.y + 13.0, cw, 32.0);
        let (_, down, clicked) = l.ui.interact(id_of("p-livery"), chip);
        l.ui.p().rounded(chip, 16.0, if down { SELECTED } else { Color::rgba(255, 255, 255, 0.1) });
        l.ui.icon("palette", Vec2::new(chip.x + 16.0, chip.center().y), 15.0, ACCENT);
        l.ui.text_in(&label, Rect::new(chip.x + 28.0, chip.y, chip.w - 34.0, chip.h), 12.0, Weight::Medium, TEXT, Align::Left);
        if clicked {
            open(l, Sheet::Livery);
        }
    }
    // on a server: which (and the way back to driving alone)
    if let Some(sn) = l.state.joined_server.clone() {
        let title = l.state.server_info.get(&sn).and_then(|x| x.1.as_ref().ok()).map(|i| i.name.clone()).unwrap_or(sn);
        let b = Rect::new(pr.x + 10.0, pr.y + 10.0, (l.ui.width(&title, 12.5, Weight::Bold) + 70.0).min(pr.w - 20.0), 34.0);
        l.ui.p().rounded(b, 17.0, Color::rgba(0, 0, 0, 0.6));
        l.ui.icon("dns", Vec2::new(b.x + 18.0, b.center().y), 15.0, OK);
        l.ui.text_in(&title, Rect::new(b.x + 32.0, b.y, b.w - 64.0, b.h), 12.5, Weight::Bold, OK, Align::Left);
        let x = Rect::new(b.right() - 32.0, b.y, 32.0, b.h);
        let (_, _, leave) = l.ui.interact(id_of("p-leave"), x);
        l.ui.icon("close", x.center(), 16.0, TEXT);
        if leave {
            l.state.leave_server();
        }
    }
    // the duty as four cards, the start under them
    let start_h = 54.0;
    let n = 4.0;
    let gap = 8.0;
    let card_h = ((col.h - start_h - gap * n) / n).clamp(44.0, 64.0);
    let map = l.state.map().map(|m| if m.friendly.is_empty() { m.name.clone() } else { m.friendly.clone() }).unwrap_or_else(|| "Choose a map".into());
    let incomplete = l.state.bus().is_some_and(|b| !b.missing_packs.is_empty());
    let bus = l.state.bus().map(|b| b.name.clone()).unwrap_or_else(|| "Choose a bus".into());
    let (duty, time) = (duty_text(l), time_text(l));
    let mut y = col.y;
    let cards = [("p-map", "map", "Map", map, false, Sheet::Map), ("p-bus", "directions_bus", "Bus", bus, incomplete, Sheet::Bus), ("p-duty", "route", "Duty", duty, false, Sheet::Duty), ("p-time", "partly_cloudy_day", "Time & weather", time, false, Sheet::Time)];
    for (id, icon, label, value, warn, s) in cards {
        if card(l, id, Rect::new(col.x, y, col.w, card_h), icon, label, &value, warn) {
            if s == Sheet::Time && l.state.joined_server.is_some() {
                l.state.set_status("On a server its clock and weather are the server's", false);
            } else {
                open(l, s);
            }
        }
        y += card_h + gap;
    }
    // Start, and "continue where you left off" beside it when there is a game to continue
    let btn = Rect::new(col.x, col.bottom() - start_h, col.w, start_h);
    let cont = l.state.joined_server.is_none() && l.state.has_last_situation();
    let (start, rest) = if cont { (Rect::new(btn.x + btn.w * 0.34 + 8.0, btn.y, btn.w * 0.66 - 8.0, btn.h), Some(Rect::new(btn.x, btn.y, btn.w * 0.34, btn.h))) } else { (btn, None) };
    if let Some(c) = rest {
        if l.ui.button("p-continue", c, "Continue", Some("history"), ButtonKind::Normal) {
            l.state.launch_last_situation();
        }
    }
    let free = l.state.choice.free || l.state.choice.line.is_none();
    let label = if l.state.joined_server.is_some() { "Join and drive" } else if free { "Drive" } else { "Start the duty" };
    if l.ui.button("p-start", start, label, Some("play_arrow"), ButtonKind::Primary) {
        super::drive::start_from_phone(l);
    }
}

fn open(l: &mut Launcher, s: Sheet) {
    l.phone.sheet = Some(s);
    l.phone.filter.clear();
}

/// A sheet's bar: the way back, the title, and a search field when `search`.
fn bar(l: &mut Launcher, r: Rect, title: &str, search: bool) -> bool {
    l.ui.solid(r);
    l.ui.p().rect(r, PANEL);
    l.ui.p().rect(Rect::new(r.x, r.bottom() - 1.0, r.w, 1.0), EDGE);
    let back = Rect::new(r.x + 6.0, r.y + 5.0, 44.0, 44.0);
    let (_, down, clicked) = l.ui.interact(id_of("sheet-back"), back);
    if down {
        l.ui.p().circle(back.center(), 20.0, HOVER);
    }
    l.ui.icon("arrow_back", back.center(), 22.0, TEXT);
    let tw = if search { (r.w * 0.34).min(260.0) } else { r.w - 70.0 };
    l.ui.text_in(title, Rect::new(r.x + 56.0, r.y, tw, r.h), 18.0, Weight::Bold, TEXT, Align::Left);
    if search {
        let f = Rect::new(r.x + 60.0 + tw, r.y + 8.0, r.w - tw - 72.0, r.h - 16.0);
        l.ui.text_input("sheet-search", f, &mut l.phone.filter, "Search…", Some("search"));
    }
    clicked
}

/// A big row of a sheet: its title, a line under it, and whether it is the one chosen.
fn big_row(ui: &mut super::ui::Ui, id: &str, r: Rect, title: &str, sub: &str, chosen: bool, badge: Option<(&str, Color)>) -> bool {
    let clicked = ui.row(id, r, chosen);
    let tx = r.x + 16.0;
    let mut tw = r.w - 60.0;
    if let Some((b, c)) = badge {
        // (left of the check mark of the chosen row)
        let bw = ui.width(b, 10.0, Weight::Bold) + 10.0;
        ui.badge(Vec2::new(r.right() - 48.0 - bw, r.y + 23.0), b, c);
        tw -= bw + 12.0;
    }
    ui.text_in(title, Rect::new(tx, r.y + 9.0, tw, 22.0), 15.5, Weight::Bold, TEXT, Align::Left);
    if !sub.is_empty() {
        ui.text_in(sub, Rect::new(tx, r.y + 32.0, r.w - 60.0, 18.0), 12.0, Weight::Regular, TEXT_DIM, Align::Left);
    }
    if chosen {
        ui.icon("check_circle", Vec2::new(r.right() - 24.0, r.center().y), 20.0, ACCENT);
    }
    clicked
}

fn sheet(l: &mut Launcher, s: Sheet, full: Rect) {
    let title = match s {
        Sheet::Map => "Choose the map",
        Sheet::Bus => "Choose the bus",
        Sheet::Livery => "Choose the livery",
        Sheet::Duty => "Line or free drive",
        Sheet::Tour => "Choose the tour",
        Sheet::Time => "Time and weather",
    };
    let search = matches!(s, Sheet::Map | Sheet::Bus | Sheet::Duty);
    let barr = Rect::new(full.x, full.y, full.w, BAR_H);
    let list = Rect::new(full.x + 8.0, barr.bottom() + 8.0, full.w - 16.0, full.h - BAR_H - 16.0);
    let back = match s {
        Sheet::Map => map_sheet(l, list),
        Sheet::Bus => bus_sheet(l, list),
        Sheet::Livery => livery_sheet(l, list),
        Sheet::Duty => duty_sheet(l, list),
        Sheet::Tour => tour_sheet(l, list),
        Sheet::Time => time_sheet(l, list),
    };
    // (the bar drawn last: the list scrolls under it)
    if bar(l, barr, title, search) || back {
        l.phone.sheet = match (s, back) {
            // a line chosen: its tours next
            (Sheet::Duty, true) if l.state.choice.line.is_some() && !l.state.choice.free => Some(Sheet::Tour),
            // a bus chosen that has liveries: the livery next
            (Sheet::Bus, true) if l.state.bus().is_some_and(|b| !b.paints.is_empty()) => Some(Sheet::Livery),
            _ => None,
        };
        l.phone.filter.clear();
    }
}

fn matches(q: &str, text: &str) -> bool {
    q.is_empty() || text.to_lowercase().contains(q)
}

fn map_sheet(l: &mut Launcher, r: Rect) -> bool {
    let q = l.phone.filter.to_lowercase();
    let items: Vec<(String, String, String, bool)> = l.state.maps.iter().filter(|m| matches(&q, &format!("{} {} {}", m.friendly, m.name, m.file))).map(|m| (m.file.clone(), if m.friendly.is_empty() { m.name.clone() } else { m.friendly.clone() }, m.description.lines().next().unwrap_or("").to_string(), m.installed)).collect();
    let chosen = l.state.choice.map.clone();
    let mut pick = None;
    l.ui.scroll_area("ps-maps", r, &mut |ui, v| {
        for (k, (file, name, desc, mod_)) in items.iter().enumerate() {
            let rr = Rect::new(v.x, v.y + k as f32 * (ROW_H + 6.0), v.w - 8.0, ROW_H);
            if big_row(ui, &format!("pm-{file}"), rr, name, desc, *file == chosen, mod_.then_some(("MOD", ACCENT_2))) {
                pick = Some(file.clone());
            }
        }
        items.len() as f32 * (ROW_H + 6.0)
    });
    if let Some(f) = pick {
        l.state.select_map(&f);
        return true;
    }
    false
}

fn bus_sheet(l: &mut Launcher, r: Rect) -> bool {
    let q = l.phone.filter.to_lowercase();
    let norm = |f: &str| f.replace('\\', "/").to_ascii_lowercase();
    let allowed: Option<std::collections::HashSet<String>> = l.state.host_vehicles().map(|v| v.iter().map(|f| norm(f)).collect());
    let items: Vec<(String, String, String, bool, bool)> = l
        .state
        .vehicles
        .iter()
        .filter(|v| matches(&q, &format!("{} {} {}", v.name, v.manufacturer, v.file)))
        .filter(|v| allowed.as_ref().map(|a| a.contains(&norm(&v.file))).unwrap_or(true))
        .map(|v| {
            let mut sub = v.manufacturer.clone();
            if !v.paints.is_empty() {
                sub = format!("{sub}{}{} liveries", if sub.is_empty() { "" } else { " · " }, v.paints.len());
            }
            if !v.missing_packs.is_empty() {
                sub = format!("{sub} · parts missing");
            }
            (v.file.clone(), v.name.clone(), sub, v.installed, !v.missing_packs.is_empty())
        })
        .collect();
    let chosen = l.state.choice.bus.clone();
    let loading = l.state.loading_content;
    let mut pick = None;
    l.ui.scroll_area("ps-buses", r, &mut |ui, v| {
        if items.is_empty() {
            ui.text_in(if loading { "Reading the buses…" } else { "No bus matches." }, Rect::new(v.x + 16.0, v.y, v.w, 40.0), 14.0, Weight::Regular, TEXT_DIM, Align::Left);
        }
        for (k, (file, name, sub, mod_, incomplete)) in items.iter().enumerate() {
            let rr = Rect::new(v.x, v.y + k as f32 * (ROW_H + 6.0), v.w - 8.0, ROW_H);
            if rr.bottom() < r.y - ROW_H * 2.0 || rr.y > r.bottom() + ROW_H * 2.0 {
                continue;
            }
            let badge = if *incomplete { Some(("PARTS", WARN)) } else if *mod_ { Some(("MOD", ACCENT_2)) } else { None };
            if big_row(ui, &format!("pb-{file}"), rr, name, sub, *file == chosen, badge) {
                pick = Some(file.clone());
            }
        }
        items.len() as f32 * (ROW_H + 6.0)
    });
    if let Some(f) = pick {
        l.state.select_bus(&f);
        return true;
    }
    false
}

fn livery_sheet(l: &mut Launcher, r: Rect) -> bool {
    let paints: Vec<String> = l.state.bus().map(|b| b.paints.clone()).unwrap_or_default();
    let default_paint = l.state.bus().map(super::drive::default_livery_label).unwrap_or("Default paint").to_string();
    let chosen = l.state.choice.paint.clone();
    let mut pick: Option<String> = None;
    l.ui.scroll_area("ps-paints", r, &mut |ui, v| {
        let all: Vec<String> = std::iter::once(String::new()).chain(paints.iter().cloned()).collect();
        for (k, p) in all.iter().enumerate() {
            let rr = Rect::new(v.x, v.y + k as f32 * (ROW_H + 6.0), v.w - 8.0, ROW_H);
            let name = if p.is_empty() { default_paint.as_str() } else { p.as_str() };
            if big_row(ui, &format!("pp-{k}"), rr, name, "", *p == chosen, None) {
                pick = Some(p.clone());
            }
        }
        all.len() as f32 * (ROW_H + 6.0)
    });
    if let Some(p) = pick {
        l.state.choice.paint = p;
        l.state.touched();
        return true;
    }
    false
}

fn duty_sheet(l: &mut Launcher, r: Rect) -> bool {
    let q = l.phone.filter.to_lowercase();
    let lines: Vec<(String, String, usize)> = l.state.lines.iter().filter(|x| x.user_allowed).filter(|x| matches(&q, &format!("{} {}", x.name, x.termini.join(" ")))).map(|x| (x.name.clone(), x.termini.join(" – "), x.tours.len())).collect();
    let free = l.state.choice.free || l.state.choice.line.is_none();
    let chosen = l.state.choice.line.clone();
    let loading = l.state.loading_lines;
    let mut pick: Option<Option<String>> = None;
    l.ui.scroll_area("ps-lines", r, &mut |ui, v| {
        let free_r = Rect::new(v.x, v.y, v.w - 8.0, ROW_H);
        if big_row(ui, "pl-free", free_r, "Free drive", "No timetable: drive where you like, the buses and traffic about you", free, None) {
            pick = Some(None);
        }
        let mut y = v.y + ROW_H + 14.0;
        ui.text_in(if loading { "Reading the timetable…" } else if lines.is_empty() { "The map has no lines to drive." } else { "Lines of the timetable" }, Rect::new(v.x + 8.0, y, v.w, 18.0), 12.0, Weight::Medium, TEXT_DIM, Align::Left);
        y += 24.0;
        for (name, termini, tours) in &lines {
            let rr = Rect::new(v.x, y, v.w - 8.0, ROW_H);
            if big_row(ui, &format!("pl-{name}"), rr, &format!("Line {name}"), &format!("{termini} · {tours} tours"), !free && chosen.as_deref() == Some(name.as_str()), None) {
                pick = Some(Some(name.clone()));
            }
            y += ROW_H + 6.0;
        }
        y - v.y
    });
    match pick {
        Some(None) => {
            l.state.choice.free = true;
            l.state.touched();
            true
        }
        Some(Some(n)) => {
            l.state.choice.free = false;
            if l.state.choice.line.as_deref() != Some(n.as_str()) {
                l.state.choice.line = Some(n);
                l.state.choice.tour = None;
            }
            l.state.touched();
            true
        }
        None => false,
    }
}

fn tour_sheet(l: &mut Launcher, r: Rect) -> bool {
    let Some(line) = l.state.line().cloned() else {
        l.ui.text_in("Choose a line first.", Rect::new(r.x + 16.0, r.y, r.w, 40.0), 14.0, Weight::Regular, TEXT_DIM, Align::Left);
        return false;
    };
    let mut tours: Vec<&omsi_launcher_lib::TourInfo> = line.tours.iter().collect();
    tours.sort_by(|a, b| b.runs.cmp(&a.runs).then_with(|| super::drive::natural(&a.number).cmp(&super::drive::natural(&b.number))));
    let items: Vec<(String, String, bool, Option<String>)> = tours
        .iter()
        .map(|t| {
            let when = format!("{} – {}", super::state::hhmm(t.first), super::state::hhmm(t.last));
            let sub = if t.runs { format!("{when} · {} trips · {}", t.trips.len(), t.days) } else { format!("{when} · {} · runs {}", t.days, t.next_run.clone().unwrap_or_else(|| "never".into())) };
            (t.number.clone(), sub, t.runs, t.next_run.clone())
        })
        .collect();
    let chosen = l.state.choice.tour.clone();
    let mut pick = None;
    l.ui.scroll_area("ps-tours", r, &mut |ui, v| {
        for (k, (num, sub, runs, next)) in items.iter().enumerate() {
            let rr = Rect::new(v.x, v.y + k as f32 * (ROW_H + 6.0), v.w - 8.0, ROW_H);
            if big_row(ui, &format!("pt-{num}"), rr, &format!("Tour {num}"), sub, chosen.as_deref() == Some(num.as_str()), (!runs).then_some(("OTHER DAY", TEXT_FAINT))) {
                pick = Some((num.clone(), *runs, next.clone()));
            }
        }
        items.len() as f32 * (ROW_H + 6.0)
    });
    if let Some((num, runs, next)) = pick {
        // a tour of another day moves the date to the next day it runs, as the desktop does
        if !runs {
            if let Some(n) = next {
                l.state.choice.date = n;
                l.state.load_lines();
            }
        }
        l.state.choice.tour = Some(num);
        l.state.touched();
        return true;
    }
    false
}

fn time_sheet(l: &mut Launcher, r: Rect) -> bool {
    let left = Rect::new(r.x + 8.0, r.y + 4.0, (r.w * 0.42).max(260.0).min(r.w - 16.0), r.h - 8.0);
    l.ui.text_in("Start", Rect::new(left.x, left.y, left.w, 18.0), 12.0, Weight::Medium, TEXT_DIM, Align::Left);
    let mut t = l.state.choice.time;
    if l.ui.time_field("p-time", Rect::new(left.x, left.y + 22.0, left.w, 48.0), &mut t) {
        l.state.choice.time = t;
        l.state.touched();
    }
    l.ui.text_in("Date", Rect::new(left.x, left.y + 84.0, left.w, 18.0), 12.0, Weight::Medium, TEXT_DIM, Align::Left);
    let mut d = l.state.choice.date.clone();
    if l.ui.date_field("p-date", Rect::new(left.x, left.y + 106.0, left.w, 48.0), &mut d) {
        l.state.choice.date = d;
        l.state.load_lines();
        l.state.touched();
    }
    let done = l.ui.button("p-time-done", Rect::new(left.x, left.bottom() - 52.0, left.w, 52.0), "Done", Some("check"), ButtonKind::Primary);
    // the weather: the map's, or one of the weather files
    let right = Rect::new(left.right() + 16.0, r.y, r.right() - left.right() - 16.0, r.h);
    let items: Vec<(String, String, String)> = [(String::new(), "The map's weather".to_string(), "As the map sets it".to_string()), ("cycle".to_string(), "Weather cycle".to_string(), "Changes every 25-60 minutes, as the month allows".to_string())].into_iter().chain(l.state.weathers.iter().map(|w| (w.file.clone(), w.name.clone(), format!("{:.0} °C · {} · {}", w.temp, if w.clouds.is_empty() { "clear" } else { w.clouds.as_str() }, if w.precip.is_empty() { "dry" } else { w.precip.as_str() })))).collect();
    let chosen = l.state.choice.weather.clone();
    let mut pick = None;
    if right.w > 120.0 {
        l.ui.scroll_area("ps-weather", right, &mut |ui, v| {
            for (k, (file, name, sub)) in items.iter().enumerate() {
                let rr = Rect::new(v.x, v.y + k as f32 * (ROW_H + 6.0), v.w - 8.0, ROW_H);
                if big_row(ui, &format!("pw-{k}"), rr, name, sub, *file == chosen, None) {
                    pick = Some(file.clone());
                }
            }
            items.len() as f32 * (ROW_H + 6.0)
        });
    }
    if let Some(f) = pick {
        l.state.choice.weather = f;
        l.state.touched();
    }
    done
}

fn online(l: &mut Launcher, body: Rect) {
    let inner = body.pad(12.0, 12.0);
    let official = omsi_net::official::ALIAS;
    l.state.ask_server(official, 15.0);
    // the official server, first and large
    let card = Rect::new(inner.x, inner.y, inner.w, 104.0);
    l.ui.p().rounded(card, 16.0, FIELD);
    l.ui.p().rounded(Rect::new(card.x, card.y, 5.0, card.h), 2.5, ACCENT);
    let ir = Rect::new(card.x + 18.0, card.y + 18.0, 68.0, 68.0);
    match l.icons.get(official) {
        Some(tex) => l.ui.image(ir, *tex, 12.0),
        None => {
            l.ui.p().rounded(ir, 12.0, SELECTED);
            l.ui.icon("public", ir.center(), 32.0, ACCENT);
        }
    }
    let info = l.state.server_info.get(official).map(|x| x.1.clone());
    if let Some(Ok(i)) = info.as_ref() {
        if !i.icon.is_empty() && !l.icons.contains_key(official) && !l.icons_pending.iter().any(|p| p.0 == official) {
            if let Ok(img) = image::load_from_memory(&i.icon) {
                l.icons_pending.push((official.to_string(), img.to_rgba8()));
            }
        }
    }
    let tx = ir.right() + 16.0;
    let tw = card.right() - tx - 170.0;
    l.ui.text_in(omsi_net::official::NAME, Rect::new(tx, card.y + 14.0, tw, 24.0), 17.0, Weight::Bold, TEXT, Align::Left);
    let (line1, line2, c) = match info.as_ref() {
        Some(Ok(i)) => {
            let map = std::path::Path::new(&i.map.replace('\\', "/")).parent().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            (format!("{} / {} players online · {map}", i.players, i.max_players), i.motd.clone(), OK)
        }
        Some(Err(e)) => (e.clone(), String::new(), DANGER),
        None => ("Looking for the server…".into(), String::new(), TEXT_DIM),
    };
    l.ui.text_in(&line1, Rect::new(tx, card.y + 42.0, tw, 18.0), 13.0, Weight::Medium, c, Align::Left);
    l.ui.text_in(&line2, Rect::new(tx, card.y + 62.0, tw, 30.0), 12.0, Weight::Regular, TEXT_DIM, Align::Left);
    let joined = l.state.joined_server.as_deref() == Some(official);
    let jb = Rect::new(card.right() - 156.0, card.y + 26.0, 140.0, 52.0);
    if joined {
        if l.ui.button("po-leave", jb, "Leave", Some("logout"), ButtonKind::Danger) {
            l.state.leave_server();
        }
    } else if l.ui.button("po-join", jb, "Join", Some("play_arrow"), ButtonKind::Primary) {
        join(l, official);
    }
    // a friend's game by its code, and hosting one's own
    let row = Rect::new(inner.x, card.bottom() + 14.0, inner.w, 52.0);
    let code_w = row.w - 150.0 - 12.0;
    l.ui.text_input("po-code", Rect::new(row.x, row.y, code_w, row.h), &mut l.phone.code, "A friend's code or a server address", Some("key"));
    if l.ui.button("po-code-join", Rect::new(row.right() - 150.0, row.y, 150.0, row.h), "Join", Some("login"), ButtonKind::Normal) {
        let c = l.phone.code.trim().to_string();
        if c.is_empty() {
            l.state.set_status("Type the code your friend's game shows (or a server's address)", true);
        } else if omsi_net::ws::ws_url(&c).is_some() || omsi_net::official::is_alias(&c) || l.state.server_info.get(&c).is_some_and(|i| i.1.is_ok()) {
            join(l, &c);
        } else {
            // a session code or a friend's address (a server's too: asked meanwhile, and
            // joined through its web gateway once it has answered): joined when the game starts
            if !omsi_net::looks_like_code(&c) {
                l.state.ask_server(&c, 5.0);
            }
            l.state.choice.lan_mode = "join".into();
            l.state.choice.lan_addr = c;
            l.state.joined_server = None;
            l.state.touched();
            l.phone.tab = Tab::Play;
            l.state.set_status("Choose your bus and press Start to join your friend", false);
        }
    }
    let mut host = l.state.choice.lan_mode == "host";
    let hr = Rect::new(inner.x, row.bottom() + 12.0, inner.w, 44.0);
    l.ui.p().rounded(hr, 12.0, FIELD);
    l.ui.text_in("Host my next game", Rect::new(hr.x + 16.0, hr.y, hr.w - 120.0, hr.h), 14.0, Weight::Medium, TEXT, Align::Left);
    if l.ui.toggle("po-host", Rect::new(hr.right() - 70.0, hr.y + 8.0, 56.0, 28.0), &mut host, "") {
        l.state.choice.lan_mode = if host { "host".into() } else { "off".into() };
        l.state.joined_server = None;
        l.state.touched();
        if host {
            l.state.set_status("Your next game is hosted: its code shows in the game for your friends", false);
        }
    }
    // the other servers of the list
    let others: Vec<super::state::ServerEntry> = l.state.servers.iter().filter(|s| !omsi_net::official::is_alias(&s.address)).cloned().collect();
    let list = Rect::new(inner.x, hr.bottom() + 12.0, inner.w, inner.bottom() - hr.bottom() - 12.0);
    for e in &others {
        l.state.ask_server(&e.address, 15.0);
    }
    let mut pick = None;
    let rows: Vec<(String, String, String)> = others
        .iter()
        .map(|e| {
            let info = l.state.server_info.get(&e.address).map(|x| x.1.clone());
            let name = if !e.name.is_empty() { e.name.clone() } else { info.as_ref().and_then(|i| i.as_ref().ok()).map(|i| i.name.clone()).unwrap_or_else(|| e.address.clone()) };
            let sub = match info {
                Some(Ok(i)) => format!("{} / {} players · {}", i.players, i.max_players, i.motd),
                Some(Err(err)) => format!("Can't reach it: {err}"),
                None => "Asking…".into(),
            };
            (e.address.clone(), name, sub)
        })
        .collect();
    if list.h > 40.0 {
        l.ui.scroll_area("po-servers", list, &mut |ui, v| {
            if rows.is_empty() {
                ui.text_in("More servers: add them by their address in the desktop launcher, or type one above.", Rect::new(v.x + 8.0, v.y, v.w - 16.0, 30.0), 12.0, Weight::Regular, TEXT_FAINT, Align::Left);
            }
            for (k, (addr, name, sub)) in rows.iter().enumerate() {
                let rr = Rect::new(v.x, v.y + k as f32 * (ROW_H + 6.0), v.w - 8.0, ROW_H);
                if big_row(ui, &format!("psv-{k}"), rr, name, sub, false, None) {
                    pick = Some(addr.clone());
                }
            }
            rows.len().max(1) as f32 * (ROW_H + 6.0)
        });
    }
    if let Some(a) = pick {
        join(l, &a);
    }
}

fn join(l: &mut Launcher, address: &str) {
    l.state.ask_server(address, 5.0);
    l.state.join_server(address);
    if l.state.joined_server.as_deref() == Some(address) {
        l.phone.tab = Tab::Play;
        l.go(Page::Drive);
    }
}

fn more(l: &mut Launcher, body: Rect) {
    let inner = body.pad(12.0, 12.0);
    let cols = if inner.w > 700.0 { 4 } else { 2 };
    let rows = MORE.len().div_ceil(cols);
    let gap = 10.0;
    let tw = (inner.w - gap * (cols as f32 - 1.0)) / cols as f32;
    let th = ((inner.h - gap * (rows as f32 - 1.0)) / rows as f32).clamp(64.0, 120.0);
    for (k, (page, name, icon, sub)) in MORE.iter().enumerate() {
        let (c, rw) = (k % cols, k / cols);
        let r = Rect::new(inner.x + (tw + gap) * c as f32, inner.y + (th + gap) * rw as f32, tw, th);
        let (h, down, clicked) = l.ui.interact(id_of(&format!("pmore-{name}")), r);
        l.ui.p().rounded(r, 14.0, if down { SELECTED } else if h { HOVER } else { FIELD });
        l.ui.icon(icon, Vec2::new(r.x + 28.0, r.y + 28.0), 24.0, ACCENT);
        l.ui.text_in(name, Rect::new(r.x + 16.0, r.bottom() - 46.0, r.w - 24.0, 20.0), 15.0, Weight::Bold, TEXT, Align::Left);
        l.ui.text_in(sub, Rect::new(r.x + 16.0, r.bottom() - 26.0, r.w - 24.0, 18.0), 11.5, Weight::Regular, TEXT_DIM, Align::Left);
        if clicked {
            l.phone.page = Some(*page);
            l.go(*page);
        }
    }
}

/// One of the desktop's pages, full width, scrolled as a whole (with a bar and a way back
/// when `back`).
fn embedded(l: &mut Launcher, page: Page, body: Rect, back: bool) {
    let top = if back { BAR_H } else { 0.0 };
    let seen = body.h - top - 12.0;
    let h = seen.max(super::mobile::PAGE_H);
    l.page_max = (h - seen).max(0.0);
    l.page_scroll = l.page_scroll.clamp(0.0, l.page_max);
    let content = Rect::new(body.x + 16.0, body.y + top + 10.0 - l.page_scroll, body.w - 32.0, h);
    l.ui.push_clip(Rect::new(body.x, body.y + top, body.w, body.h - top), 0.0);
    match page {
        Page::Drive => super::drive::draw(l, content),
        Page::Multiplayer => super::multiplayer::draw(l, content),
        Page::Profile => super::pages::profile(l, content),
        Page::Settings => super::pages::settings(l, content),
        Page::Controls => super::pages::controls(l, content),
        Page::Sessions => super::pages::sessions(l, content),
        Page::Mods => super::pages::mods(l, content),
        Page::Tutorials => super::pages::tutorials(l, content),
        Page::Timetable => super::timetable::draw(l, content),
        Page::Setup => super::pages::setup(l, content),
    }
    l.ui.pop_clip();
    if back {
        let name = MORE.iter().find(|m| m.0 == page).map(|m| m.1).unwrap_or("");
        if bar(l, Rect::new(body.x, body.y, body.w, BAR_H), name, false) {
            l.phone.page = None;
            l.page_scroll = 0.0;
        }
    }
}
