//! The Drive page: the duty put together in four steps - the bus, the route (map, line,
//! tour), the time and weather, the roadbook - in a panel on the left, the bus itself in
//! the showroom on the right, and the button that starts the game.

use super::state::{fmt_bytes, hhmm, trip_index_at};
use super::theme::*;
use super::ui::{id_of, ButtonKind};
use super::Launcher;
use glam::Vec2;
use omsi_ui::paint::Align;
use omsi_ui::{Color, Rect, Weight};

/// A line of the bus list: index, name, file, new, installed, liveries, parts missing.
type BusItem = (usize, String, String, bool, bool, usize, bool);

#[derive(Default)]
pub struct DriveView {
    pub step: usize,
    pub bus_filter: String,
    /// The bus list as last built, and what it was built for (the filter, how many buses
    /// were known, the host's list): rebuilt only when one of those changes - it was built
    /// afresh every frame, every name and path copied, and scrolling stuttered on a phone.
    bus_items: std::sync::Arc<Vec<BusItem>>,
    bus_items_key: (String, usize, usize, usize),
    pub line_filter: String,
    /// The list was scrolled to the chosen bus (once, when the lists came).
    pub scrolled_to_bus: bool,
}

const STEPS: [(&str, &str); 4] = [("Bus", "directions_bus"), ("Route", "route"), ("Time & weather", "partly_cloudy_day"), ("Roadbook", "receipt_long")];

pub fn draw(l: &mut Launcher, area: Rect) {
    let form_w = (area.w * 0.5).clamp(440.0, 580.0);
    let panel = Rect::new(area.x, area.y, form_w, area.h);
    l.ui.panel(panel);
    // the steps: plain tabs with a line under the chosen one
    let tabs = Rect::new(panel.x + 16.0, panel.y + 8.0, panel.w - 32.0, 42.0);
    let w = tabs.w / 4.0;
    let sel = l.ui.anim(id_of("drive-step"), l.drive.step as f32, 0.08);
    l.ui.p().rect(Rect::new(tabs.x, tabs.bottom() - 1.0, tabs.w, 1.0), EDGE);
    l.ui.p().rect(Rect::new(tabs.x + w * sel, tabs.bottom() - 2.0, w, 2.0), ACCENT);
    for (k, (name, _)) in STEPS.iter().enumerate() {
        let cell = Rect::new(tabs.x + w * k as f32, tabs.y, w, tabs.h - 2.0);
        let id = id_of(&format!("step-{k}"));
        let (h, _, clicked) = l.ui.interact(id, cell);
        if clicked {
            l.drive.step = k;
            if k == 3 {
                l.state.load_ibis();
            }
        }
        let on = l.drive.step == k;
        let c = if on { TEXT } else if h { TEXT_SOFT } else { TEXT_DIM };
        l.ui.text_in(name, cell, 13.0, if on { Weight::Medium } else { Weight::Regular }, c, Align::Center);
    }
    let body = Rect::new(panel.x + 18.0, tabs.bottom() + 16.0, panel.w - 36.0, panel.bottom() - tabs.bottom() - 32.0);
    match l.drive.step {
        0 => step_bus(l, body),
        1 => step_route(l, body),
        2 => step_time(l, body),
        _ => step_roadbook(l, body),
    }
    // the right side: the bus, what the duty is, and the start
    let side = Rect::new(panel.right() + 24.0, area.y, area.right() - panel.right() - 24.0, area.h);
    summary(l, side);
}

fn step_bus(l: &mut Launcher, r: Rect) {
    let search = Rect::new(r.x, r.y, r.w, ROW);
    l.ui.text_input("bus-filter", search, &mut l.drive.bus_filter, "Search buses…", Some("search"));
    let q = l.drive.bus_filter.to_lowercase();
    // joining a host or a server: only the buses it has (another bus would be drawn there as
    // a stand-in of its own)
    let norm = |f: &str| f.replace('\\', "/").to_ascii_lowercase();
    let allowed: Option<std::collections::HashSet<String>> = l.state.host_vehicles().map(|v| v.iter().map(|f| norm(f)).collect());
    if let Some(a) = allowed.as_ref() {
        if !a.contains(&norm(&l.state.choice.bus)) {
            if let Some(first) = l.state.vehicles.iter().find(|v| a.contains(&norm(&v.file))).map(|v| v.file.clone()) {
                l.state.select_bus(&first);
            }
        }
    }
    let list_h = (r.h - ROW - 12.0 - 190.0).max(160.0);
    let list = Rect::new(r.x - 4.0, search.bottom() + 10.0, r.w + 8.0, list_h);
    let key = (q.clone(), l.state.vehicles.len(), allowed.as_ref().map(|a| a.len()).unwrap_or(usize::MAX), l.state.fresh.len());
    if key != l.drive.bus_items_key || (l.drive.bus_items.is_empty() && !l.state.vehicles.is_empty()) {
        let built: Vec<BusItem> = l
            .state
            .vehicles
            .iter()
            .enumerate()
            .filter(|(_, v)| q.is_empty() || format!("{} {}", v.name, v.file).to_lowercase().contains(&q))
            .filter(|(_, v)| allowed.as_ref().map(|a| a.contains(&norm(&v.file))).unwrap_or(true))
            .map(|(i, v)| (i, v.name.clone(), v.file.clone(), l.state.fresh.contains_key(&v.file), v.installed, v.paints.len(), !v.missing_packs.is_empty()))
            .collect();
        l.drive.bus_items = std::sync::Arc::new(built);
        l.drive.bus_items_key = key;
    }
    let items = l.drive.bus_items.clone();
    let chosen = l.state.choice.bus.clone();
    if !l.drive.scrolled_to_bus && !items.is_empty() {
        if let Some(k) = items.iter().position(|i| i.2 == chosen) {
            l.ui.scroll_to("bus-list", k as f32 * 52.0, 52.0 * 3.0, list.h);
        }
        l.drive.scrolled_to_bus = true;
    }
    let mut pick: Option<String> = None;
    let loading = l.state.loading_content;
    l.ui.scroll_area("bus-list", list, &mut |ui, v| {
        let row_h = 52.0;
        if items.is_empty() {
            ui.text_in(if loading { "Reading the buses…" } else { "No bus matches." }, Rect::new(v.x + 12.0, v.y, v.w, 40.0), 13.0, Weight::Regular, TEXT_DIM, Align::Left);
        }
        for (k, (_, name, file, fresh, installed, paints, incomplete)) in items.iter().enumerate() {
            let rr = Rect::new(v.x + 4.0, v.y + k as f32 * row_h, v.w - 14.0, row_h - 4.0);
            if rr.bottom() < list.y - row_h || rr.y > list.bottom() + row_h {
                continue;
            }
            if ui.row(&format!("bus-{file}"), rr, *file == chosen) {
                pick = Some(file.clone());
            }
            ui.icon("directions_bus", Vec2::new(rr.x + 22.0, rr.center().y), 20.0, if *file == chosen { TEXT } else { TEXT_FAINT });
            let mut x = rr.x + 44.0;
            let tw = ui.text_in(name, Rect::new(x, rr.y + 6.0, rr.w - 150.0, 20.0), 13.5, Weight::Medium, TEXT, Align::Left);
            x += tw + 8.0;
            if *fresh {
                x += ui.badge(Vec2::new(x, rr.y + 8.0), "NEW", OK) + 4.0;
            }
            if *installed {
                x += ui.badge(Vec2::new(x, rr.y + 8.0), "MOD", ACCENT_2) + 4.0;
            }
            if *incomplete {
                ui.badge(Vec2::new(x, rr.y + 8.0), "PARTS MISSING", WARN);
            }
            ui.text_in(file, Rect::new(rr.x + 44.0, rr.y + 26.0, rr.w - 150.0, 16.0), 11.5, Weight::Regular, TEXT_FAINT, Align::Left);
            if *paints > 0 {
                ui.text_in(&format!("{paints} {}", omsi_ui::tr(if *paints == 1 { "livery" } else { "liveries" })), Rect::new(rr.right() - 100.0, rr.y, 90.0, rr.h), 11.5, Weight::Medium, TEXT_DIM, Align::Right);
            }
        }
        items.len() as f32 * row_h + 4.0
    });
    if let Some(f) = pick {
        l.state.select_bus(&f);
    }
    // the livery
    let mut y = list.bottom() + 14.0;
    if let Some(v) = l.state.bus().cloned() {
        l.ui.label(Rect::new(r.x, y, 130.0, ROW), "Livery");
        let mut opts = vec!["Default paint".to_string()];
        opts.extend(v.paints.iter().cloned());
        let mut sel = v.paints.iter().position(|p| *p == l.state.choice.paint).map(|i| i + 1).unwrap_or(0);
        if l.ui.select("paint", Rect::new(r.x + 130.0, y, r.w - 130.0, ROW), &mut sel, &opts) {
            l.state.choice.paint = if sel == 0 { String::new() } else { v.paints[sel - 1].clone() };
            l.state.touched();
        }
        // the depot file (.hof): the map's for the date by default (see
        // `State::default_hof`), or one of the bus's own chosen by hand - a bus often brings
        // several for the same map
        if v.hofs.len() > 1 || l.state.choice.hof_manual {
            y += ROW + 8.0;
            l.ui.label(Rect::new(r.x, y, 130.0, ROW), "Depot file");
            let auto = l.state.default_hof();
            let mut opts = vec![format!("Automatic ({auto})")];
            opts.extend(v.hofs.iter().cloned());
            let mut sel = if l.state.choice.hof_manual { v.hofs.iter().position(|h| h.eq_ignore_ascii_case(&l.state.choice.hof)).map(|i| i + 1).unwrap_or(0) } else { 0 };
            if l.ui.select("hof", Rect::new(r.x + 130.0, y, r.w - 130.0, ROW), &mut sel, &opts) {
                if sel == 0 {
                    l.state.choice.hof_manual = false;
                    l.state.choice.hof = auto;
                } else {
                    l.state.choice.hof_manual = true;
                    l.state.choice.hof = v.hofs[sel - 1].clone();
                }
                l.state.touched();
            }
        }
        // the fleet number from the bus's `[number]` list (Omsi.exe's number combo,
        // 0x67e150): it gives the `number` variable and the plate the list pairs with it
        if !v.numbers.is_empty() {
            y += ROW + 8.0;
            l.ui.label(Rect::new(r.x, y, 130.0, ROW), "Fleet number");
            let opts: Vec<String> = v.numbers.iter().map(|(n, p)| if p.trim().is_empty() { n.clone() } else { format!("{n}  ({})", p.trim()) }).collect();
            let mut sel = v.numbers.iter().position(|(n, _)| *n == l.state.choice.number).unwrap_or(0);
            if l.ui.select("number", Rect::new(r.x + 130.0, y, r.w - 130.0, ROW), &mut sel, &opts) {
                l.state.choice.number = v.numbers[sel].0.clone();
                l.state.touched();
            }
        }
        // the number plate (registration) by hand: empty leaves it to the bus's `[number]`
        // list and the map's `registrations.txt`, as before
        y += ROW + 8.0;
        l.ui.label(Rect::new(r.x, y, 130.0, ROW), "Number plate");
        if l.ui.text_input("plate", Rect::new(r.x + 130.0, y, r.w - 130.0, ROW), &mut l.state.choice.plate, "Automatic", Some("badge")) {
            l.state.touched();
        }
        y += ROW + 8.0;
        if !v.missing_packs.is_empty() {
            let text = format!(
                "This bus takes its dashboard, steering wheel or ticket machine from {} - not installed. It will drive with those parts missing, as in OMSI 2; install {} (Mods page) to complete it.",
                v.missing_packs.join(", "),
                if v.missing_packs.len() == 1 { "that pack" } else { "those packs" }
            );
            let h = l.ui.paragraph(&text, Vec2::new(r.x, y), r.w, 12.5, Weight::Regular, WARN);
            y += h + 10.0;
        }
        y += ROW + 10.0;
        // (lines as the file writes them, without their tabs and indents: a tab drew the
        // first letters outside the panel)
        let desc = v.description.replace('\t', " ").lines().map(str::trim).collect::<Vec<_>>().join("\n").trim().to_string();
        if !desc.is_empty() {
            let dr = Rect::new(r.x, y, r.w, r.bottom() - y);
            l.ui.push_clip(dr, 0.0);
            l.ui.paragraph(&desc, Vec2::new(dr.x, dr.y), dr.w, 12.5, Weight::Regular, TEXT_DIM);
            l.ui.pop_clip();
        }
    }
}

fn step_route(l: &mut Launcher, r: Rect) {
    let mut y = r.y;
    // the map (on a server: the server's, not to be changed)
    if let Some(name) = joined_server_name(l) {
        let m = l.state.map().map(|m| if m.friendly.is_empty() { m.name.clone() } else { m.friendly.clone() }).unwrap_or_else(|| l.state.choice.map.clone());
        l.ui.label(Rect::new(r.x, y, 110.0, ROW), "Map");
        let fr = Rect::new(r.x + 110.0, y, r.w - 110.0, ROW);
        l.ui.solid(fr);
        l.ui.p().rounded(fr, RADIUS, FIELD);
        l.ui.icon("lock", Vec2::new(fr.x + 16.0, fr.center().y), 15.0, TEXT_DIM);
        l.ui.text_in(&format!("{m}  ·  set by {name}"), Rect::new(fr.x + 32.0, fr.y, fr.w - 40.0, fr.h), 13.0, Weight::Regular, TEXT_SOFT, Align::Left);
        y += ROW + 8.0;
        return step_route_rest(l, r, y);
    }
    let maps: Vec<(String, String)> = l.state.maps.iter().map(|m| (m.file.clone(), format!("{}{}{}", if l.state.fresh.contains_key(&m.file) { "★ NEW · " } else { "" }, if m.friendly.is_empty() { &m.name } else { &m.friendly }, if m.installed { "  (mod)" } else { "" }))).collect();
    let mut sel = maps.iter().position(|m| m.0 == l.state.choice.map).unwrap_or(0);
    l.ui.label(Rect::new(r.x, y, 110.0, ROW), "Map");
    if l.ui.select("map", Rect::new(r.x + 110.0, y, r.w - 110.0, ROW), &mut sel, &maps.iter().map(|m| m.1.clone()).collect::<Vec<_>>()) {
        if let Some((f, _)) = maps.get(sel) {
            let f = f.clone();
            l.state.select_map(&f);
        }
    }
    y += ROW + 8.0;
    step_route_rest(l, r, y);
}

fn step_route_rest(l: &mut Launcher, r: Rect, mut y: f32) {
    let mut free = l.state.choice.free;
    if l.ui.toggle("free", Rect::new(r.x, y, r.w, ROW), &mut free, "Free drive (no timetable duty)") {
        l.state.choice.free = free;
        l.state.touched();
    }
    y += ROW + 10.0;
    // where to start: one of the map's entry points, as in OMSI 2; Automatic takes the one
    // nearest to the duty's first stop by road (a free drive: the map's first)
    if let Some(m) = l.state.map().cloned() {
        let mut labels = vec![if free { "Automatic (the map's first)".to_string() } else { "Automatic (nearest to the first stop)".to_string() }];
        labels.extend(m.entry_points.iter().map(|e| if e.name.is_empty() { format!("entry {}", e.index + 1) } else { e.name.clone() }));
        // (the choice is the entry's place in the list; 0 = automatic here)
        let mut es = if l.state.choice.entry < 0 { 0 } else { (l.state.choice.entry as usize + 1).min(labels.len() - 1) };
        l.ui.label(Rect::new(r.x, y, 110.0, ROW), "Start at");
        if l.ui.select("entry", Rect::new(r.x + 110.0, y, r.w - 110.0, ROW), &mut es, &labels) {
            l.state.choice.entry = es as i32 - 1;
            l.state.touched();
        }
        y += ROW + 10.0;
    }
    if free {
        l.ui.paragraph("Free driving: the bus starts where you chose, with the traffic and the timetable's buses around it, but no line of your own.", Vec2::new(r.x, y), r.w, 13.0, Weight::Regular, TEXT_DIM);
        return;
    }
    // lines and tours side by side
    let half = (r.w - 12.0) * 0.5;
    let left = Rect::new(r.x, y, half, r.bottom() - y);
    let right = Rect::new(r.x + half + 12.0, y, half, r.bottom() - y);
    l.ui.heading(Rect::new(left.x, left.y, left.w, 28.0), "Line", None);
    l.ui.text_input("line-filter", Rect::new(left.x, left.y + 30.0, left.w, 34.0), &mut l.drive.line_filter, "Filter…", Some("search"));
    let q = l.drive.line_filter.to_lowercase();
    let lines: Vec<(String, String, usize)> = l.state.lines.iter()
        .filter(|x| x.user_allowed)
        .filter(|x| q.is_empty() || x.name.to_lowercase().contains(&q))
        .map(|x| (x.name.clone(), x.termini.join(" · "), x.tours.len()))
        .collect();
    let chosen = l.state.choice.line.clone();
    let mut pick_line = None;
    let loading = l.state.loading_lines;
    l.ui.scroll_area("line-list", Rect::new(left.x - 4.0, left.y + 72.0, left.w + 8.0, left.h - 72.0), &mut |ui, v| {
        let rh = 48.0;
        if lines.is_empty() {
            ui.text_in(if loading { "Reading the timetable…" } else { "No lines on this date." }, Rect::new(v.x + 10.0, v.y, v.w, 36.0), 12.5, Weight::Regular, TEXT_DIM, Align::Left);
        }
        for (k, (name, termini, tours)) in lines.iter().enumerate() {
            let rr = Rect::new(v.x + 4.0, v.y + k as f32 * rh, v.w - 12.0, rh - 4.0);
            let on = chosen.as_deref() == Some(name.as_str());
            if ui.row(&format!("line-{name}"), rr, on) {
                pick_line = Some(name.clone());
            }
            // (a long line name, as Ahlheim's "Eichenhoehe TA11 Mo-Do Schule", is cut)
            let count = format!("{tours}");
            let cw = ui.width(&count, 11.5, Weight::Bold) + 8.0;
            let bw = (ui.width(name, 13.0, Weight::Black) + 14.0).clamp(34.0, (rr.w - cw - 24.0).max(34.0));
            let badge = Rect::new(rr.x + 8.0, rr.y + 8.0, bw, 22.0);
            ui.p().rounded(badge, 4.0, Color::rgba(52, 52, 52, 1.0));
            ui.text_in(name, badge.pad(6.0, 0.0), 12.5, Weight::Bold, TEXT, Align::Center);
            ui.icon("event", Vec2::new(rr.right() - cw - 8.0, rr.y + 19.0), 13.0, TEXT_FAINT);
            ui.text_in(&count, Rect::new(rr.right() - cw, rr.y + 8.0, cw - 4.0, 22.0), 11.5, Weight::Bold, TEXT_DIM, Align::Right);
            ui.text_in(termini, Rect::new(rr.x + 8.0, rr.y + 28.0, rr.w - 16.0, 16.0), 11.0, Weight::Regular, TEXT_FAINT, Align::Left);
        }
        lines.len() as f32 * rh + 4.0
    });
    if let Some(n) = pick_line {
        if l.state.choice.line.as_deref() != Some(n.as_str()) {
            l.state.choice.line = Some(n);
            l.state.choice.tour = None;
            l.state.touched();
        }
    }
    l.ui.heading(Rect::new(right.x, right.y, right.w, 28.0), "Tour", None);
    let Some(line) = l.state.line().cloned() else {
        l.ui.paragraph("Pick a line first. As in OMSI, the start time and date then say where in the tour the bus is: the trip under way, or the next to leave.", Vec2::new(right.x, right.y + 34.0), right.w, 12.5, Weight::Regular, TEXT_DIM);
        return;
    };
    let mut tours: Vec<&omsi_launcher_lib::TourInfo> = line.tours.iter().collect();
    tours.sort_by(|a, b| b.runs.cmp(&a.runs).then_with(|| natural(&a.number).cmp(&natural(&b.number))));
    let now = l.state.choice.time as f64 * 60.0;
    let tours: Vec<(String, usize, String, bool, Option<String>, Option<(f64, f64)>, String, String)> = tours.iter().map(|t| {
        let trip = trip_index_at(t, now).and_then(|i| t.trips.get(i));
        let from = t.trips.first().map(|x| x.from.clone()).unwrap_or_default();
        let terminus = t.trips.last().map(|x| x.terminus.clone()).unwrap_or_default();
        (
            t.number.clone(),
            t.trips.len(),
            t.days.clone(),
            t.runs,
            t.next_run.clone(),
            trip.map(|x| (x.departure, x.arrival)),
            from,
            terminus,
        )
    }).collect();
    let chosen_t = l.state.choice.tour.clone();
    let mut pick = None;
    l.ui.scroll_area("tour-list", Rect::new(right.x - 4.0, right.y + 30.0, right.w + 8.0, right.h - 30.0), &mut |ui, v| {
        let rh = 84.0;
        for (k, (num, trips, days, runs, next, trip_time, from, terminus)) in tours.iter().enumerate() {
            let rr = Rect::new(v.x + 4.0, v.y + k as f32 * rh, v.w - 12.0, rh - 4.0);
            let on = chosen_t.as_deref() == Some(num.as_str());
            if ui.row(&format!("tour-{num}"), rr, on) {
                pick = Some((num.clone(), *runs, next.clone()));
            }
            let c = if *runs { TEXT } else { TEXT_FAINT };
            // (the tour's name as the map writes it and OMSI lists it: "1", "Mo-Fr 1")
            let name = num.clone();
            ui.text_in(&name, Rect::new(rr.x + 10.0, rr.y + 5.0, rr.w - 110.0, 18.0), 13.5, Weight::Bold, c, Align::Left);
            if let Some((departure, arrival)) = trip_time {
                ui.text_in(&format!("{} - {}", hhmm(*departure), hhmm(*arrival)), Rect::new(rr.right() - 110.0, rr.y + 6.0, 100.0, 18.0), 12.0, Weight::Medium, if *runs { ACCENT } else { TEXT_FAINT }, Align::Right);
                ui.text_in(&format!("{}: {}", omsi_ui::tr("Trip duration"), trip_duration(*departure, *arrival)), Rect::new(rr.x + 10.0, rr.y + 43.0, rr.w - 20.0, 16.0), 12.0, Weight::Medium, c, Align::Left);
            }
            if !from.is_empty() || !terminus.is_empty() {
                ui.text_in(&format!("{from} → {terminus}"), Rect::new(rr.x + 10.0, rr.y + 25.0, rr.w - 20.0, 16.0), 11.5, Weight::Medium, c, Align::Left);
            }
            let sub = if *runs {
                format!("{trips} trips · {days}")
            } else {
                match next {
                    Some(n) => format!("{trips} trips · {days} · runs {n}"),
                    None => format!("{trips} trips · never within a year"),
                }
            };
            ui.text_in(&sub, Rect::new(rr.x + 10.0, rr.y + 61.0, rr.w - 20.0, 16.0), 11.0, Weight::Regular, TEXT_DIM, Align::Left);
        }
        tours.len() as f32 * rh + 4.0
    });
    if let Some((num, runs, next)) = pick {
        // a tour of another day moves the date to the next day it runs (OMSI lists only
        // the day's tours)
        if !runs {
            if let Some(n) = next {
                l.state.choice.date = n;
                l.state.load_lines();
            }
        }
        l.state.choice.tour = Some(num);
        l.state.touched();
    }
}

pub(super) fn natural(s: &str) -> (u64, String) {
    let digits: String = s.chars().take_while(|c| c.is_ascii_digit()).collect();
    (digits.parse().unwrap_or(u64::MAX), s.to_string())
}

/// Time from the departure to the arrival of one trip.
fn trip_duration(first: f64, last: f64) -> String {
    let minutes = ((last - first).max(0.0) / 60.0).round() as i64;
    let hours = minutes / 60;
    let remaining = minutes % 60;
    let count = |n: i64, one: &str, many: &str| format!("{n} {}", omsi_ui::tr(if n == 1 { one } else { many }));
    match (hours, remaining) {
        (0, m) => count(m, "minute", "minutes"),
        (h, 0) => count(h, "hour", "hours"),
        (h, m) => format!("{} {}", count(h, "hour", "hours"), count(m, "minute", "minutes")),
    }
}

/// The name of the server the Drive page is joined to (see the Multiplayer page).
fn joined_server_name(l: &Launcher) -> Option<String> {
    let a = l.state.joined_server.as_ref()?;
    let entry = l.state.servers.iter().find(|s| &s.address == a);
    let info = l.state.server_info.get(a).and_then(|x| x.1.as_ref().ok());
    Some(entry.map(|e| e.name.clone()).filter(|n| !n.is_empty()).or_else(|| info.map(|i| i.name.clone())).unwrap_or_else(|| a.clone()))
}

fn step_time(l: &mut Launcher, r: Rect) {
    let mut y = r.y;
    if let Some(name) = joined_server_name(l) {
        // the server's world: nothing to choose here
        let info = l.state.joined_server.as_ref().and_then(|a| l.state.server_info.get(a)).and_then(|x| x.1.as_ref().ok()).cloned();
        l.ui.heading(Rect::new(r.x, y, r.w, 28.0), &format!("Set by {name}"), Some("lock"));
        y += 36.0;
        let rows = [("Time", info.as_ref().map(|i| i.time.clone()).unwrap_or_default()), ("Weather", info.as_ref().map(|i| if i.weather.is_empty() { "the map's".to_string() } else { i.weather.clone() }).unwrap_or_default())];
        for (k, v) in rows {
            l.ui.text_in(k, Rect::new(r.x, y, 110.0, 22.0), 13.0, Weight::Regular, TEXT_DIM, Align::Left);
            l.ui.text_in(&v, Rect::new(r.x + 110.0, y, r.w - 110.0, 22.0), 13.0, Weight::Regular, TEXT, Align::Left);
            y += 26.0;
        }
        l.ui.paragraph("On a server the map, the time, the date and the weather are the same for everybody: the server keeps the world's clock. You choose your bus and your duty.", Vec2::new(r.x, y + 8.0), r.w, 12.5, Weight::Regular, TEXT_DIM);
        return;
    }
    let col = (r.w - 12.0) * 0.5;
    l.ui.label(Rect::new(r.x, y, col, 20.0), "Time");
    l.ui.label(Rect::new(r.x + col + 12.0, y, col, 20.0), "Date");
    y += 22.0;
    let mut t = l.state.choice.time;
    if l.ui.time_field("time", Rect::new(r.x, y, col, 44.0), &mut t) {
        l.state.choice.time = t;
        l.state.touched();
    }
    let mut d = l.state.choice.date.clone();
    if l.ui.date_field("date", Rect::new(r.x + col + 12.0, y, col, 44.0), &mut d) {
        l.state.choice.date = d;
        l.state.choice.season = "auto".into();
        l.state.load_lines();
        l.state.touched();
    }
    y += 54.0;
    let seasons = ["auto", "spring", "summer", "autumn", "winter"];
    let mut s = seasons.iter().position(|x| *x == l.state.choice.season).unwrap_or(0);
    if l.ui.segmented("season", Rect::new(r.x, y, r.w, 34.0), &mut s, &["By date", "Spring", "Summer", "Autumn", "Winter"]) {
        l.state.choice.season = seasons[s].to_string();
        if s > 0 {
            let month = ["", "04", "07", "10", "01"][s];
            let date = l.state.choice.date.clone();
            let (yy, dd) = (date.get(0..4).unwrap_or("1989").to_string(), date.get(8..10).unwrap_or("15").to_string());
            l.state.choice.date = format!("{yy}-{month}-{dd}");
            l.state.load_lines();
        }
        let w = l.state.choice.weather.clone();
        if let Some(wi) = l.state.weathers.iter().find(|x| x.file == w).cloned() {
            if !l.state.weather_fits(&wi) {
                l.state.choice.weather.clear();
            }
        }
        l.state.touched();
    }
    y += 46.0;
    let mut traffic = l.state.choice.traffic;
    if l.ui.slider("traffic", Rect::new(r.x, y, r.w, 34.0), &mut traffic, 0.0, 120.0, 1.0, "Cars around", &|v| format!("{v:.0}")) {
        l.state.choice.traffic = traffic;
        l.state.touched();
    }
    y += 40.0;
    let mut v = l.state.choice.passengers;
    if l.ui.toggle("pax", Rect::new(r.x, y, col, 32.0), &mut v, "Passengers") {
        l.state.choice.passengers = v;
        l.state.touched();
    }
    let mut v = l.state.choice.schedule;
    if l.ui.toggle("sched", Rect::new(r.x + col + 12.0, y, col, 32.0), &mut v, "Timetable buses") {
        l.state.choice.schedule = v;
        l.state.touched();
    }
    y += 36.0;
    let mut v = l.state.choice.autostart;
    if l.ui.toggle("autostart", Rect::new(r.x, y, r.w, 32.0), &mut v, "Put the bus into service on start (Shift+U)") {
        l.state.choice.autostart = v;
        l.state.touched();
    }
    y += 36.0;
    let mut v = l.state.choice.on_foot;
    if l.ui.toggle("onfoot", Rect::new(r.x, y, r.w, 32.0), &mut v, "Start on foot (place a bus from the game menu)") {
        l.state.choice.on_foot = v;
        l.state.touched();
    }
    y += 44.0;
    // weather cards
    l.ui.heading(Rect::new(r.x, y, r.w, 28.0), "Weather", Some("partly_cloudy_day"));
    y += 30.0;
    let mut items: Vec<(String, String, String, String, bool)> = vec![(String::new(), "Map default".into(), "Whatever the map starts with".into(), "wb_sunny".into(), false)];
    // OMSI 2's current weather: an airport's METAR report, fetched when the game starts
    let metar = l.state.choice.weather.strip_prefix("metar:").map(str::to_string);
    // (the airport nearest the map, not Berlin's for every map: Novi Sad got Berlin's rain)
    let home = nearest_airport(&l.state.config.root, &l.state.choice.map);
    items.push((format!("metar:{}", metar.clone().unwrap_or_else(|| home.clone())), "Current weather".into(), format!("METAR of {} (fetched at the start)", metar.clone().unwrap_or_else(|| home.clone())), "public".into(), false));
    // the weather going on from one to another through the day
    items.push(("cycle".into(), "Weather cycle".into(), "Changes every 25-60 minutes, as the month allows".into(), "autorenew".into(), false));
    for w in l.state.weathers.clone() {
        if !l.state.weather_fits(&w) {
            continue;
        }
        let vis = if w.fog_m >= 20000.0 { "clear air".to_string() } else { format!("{:.0} m", w.fog_m) };
        let icon = if w.snow || w.precip.starts_with("snow") {
            "weather_snowy"
        } else if w.precip.starts_with("rain") {
            "rainy"
        } else if w.fog_m < 1500.0 {
            "foggy"
        } else if w.clouds.to_lowercase().contains("overcast") {
            "cloud"
        } else if w.clouds.to_lowercase().contains("cumulus") {
            "partly_cloudy_day"
        } else {
            "wb_sunny"
        };
        items.push((w.file.clone(), w.name.clone(), format!("{:.0} °C · {} · {vis}", w.temp, w.precip), icon.into(), l.state.fresh.contains_key(&w.file)));
    }
    if let Some(code) = metar.as_ref() {
        static AIRPORTS: std::sync::OnceLock<Vec<(String, String)>> = std::sync::OnceLock::new();
        let root = std::path::PathBuf::from(&l.state.config.root);
        let list = AIRPORTS.get_or_init(|| {
            let text = std::fs::read(root.join("Weather").join("ICAO.txt"))
                .map(|b| omsi_cfg::codepage::decode(&b))
                .unwrap_or_default();
            let mut v: Vec<(String, String)> = text
                .lines()
                .filter_map(|l| {
                    l.split_once(" - ")
                        .map(|(c, n)| (c.trim().to_string(), format!("{} - {}", c.trim(), n.trim())))
                })
                .collect();
            if !v.iter().any(|a| a.0 == "EDDB") {
                v.insert(0, ("EDDB".into(), "EDDB - Berlin Brandenburg".into()));
            }

            v
        });

        let mut airport = code.to_uppercase().chars().take(4).collect::<String>();

        l.ui.label(Rect::new(r.x, y, 110.0, ROW), "Airport");

        let button_w = 150.0;
        let gap = 8.0;
        let input_w = r.w - 110.0 - button_w - gap;

        if l.ui.text_input(
            "metar-airport",
            Rect::new(r.x + 110.0, y, input_w, ROW),
            &mut airport,
            "ICAO",
            None,
        ) {
            airport = airport
                .chars()
                .filter(|c| c.is_ascii_alphabetic())
                .take(4)
                .collect::<String>()
                .to_uppercase();

            l.state.choice.weather = if airport.is_empty() {
                "metar:".into()
            } else {
                format!("metar:{airport}")
            };
            l.state.touched();
        }
        let labels: Vec<String> = list.iter().map(|a| a.1.clone()).collect();
        let mut sel = list
            .iter()
            .position(|a| a.0.eq_ignore_ascii_case(&airport))
            .unwrap_or(0);

        if l.ui.select(
            "metar-airport-list",
            Rect::new(r.x + 110.0 + input_w + gap, y, button_w, ROW),
            &mut sel,
            &labels,
        ) {
            if let Some(a) = list.get(sel) {
                l.state.choice.weather = format!("metar:{}", a.0);
                l.state.touched();
            }
        }

        y += ROW + 8.0;
    }
    let chosen = l.state.choice.weather.clone();
    let mut pick = None;
    let area = Rect::new(r.x - 4.0, y, r.w + 8.0, r.bottom() - y);
    l.ui.scroll_area("weather", area, &mut |ui, v| {
        let cw = (v.w - 20.0) / 2.0;
        let ch = 62.0;
        for (k, (file, name, meta, icon, fresh)) in items.iter().enumerate() {
            let (cx, cy) = ((k % 2) as f32, (k / 2) as f32);
            let rr = Rect::new(v.x + 4.0 + cx * (cw + 8.0), v.y + cy * (ch + 8.0), cw, ch);
            let on = *file == chosen;
            let id = id_of(&format!("w-{file}"));
            let (h, _, clicked) = ui.interact(id, rr);
            if clicked {
                pick = Some(file.clone());
            }
            let t = ui.anim(id, if h { 1.0 } else { 0.0 }, 0.08);
            ui.p().rounded(rr, 6.0, if on { SELECTED } else { FIELD.mix(HOVER, t) });
            ui.p().rounded_border(rr, 6.0, 1.0, if on { ACCENT.alpha(0.6) } else { EDGE });
            ui.icon(icon, Vec2::new(rr.x + 24.0, rr.center().y), 22.0, if on { TEXT } else { TEXT_DIM });
            let tw = ui.text_in(name, Rect::new(rr.x + 46.0, rr.y + 10.0, rr.w - 56.0, 20.0), 13.0, Weight::Bold, TEXT, Align::Left);
            if *fresh {
                ui.badge(Vec2::new(rr.x + 50.0 + tw, rr.y + 12.0), "NEW", OK);
            }
            ui.text_in(meta, Rect::new(rr.x + 46.0, rr.y + 33.0, rr.w - 56.0, 18.0), 11.0, Weight::Regular, TEXT_DIM, Align::Left);
        }
        ((items.len() + 1) / 2) as f32 * (ch + 8.0) + 4.0
    });
    if let Some(f) = pick {
        l.state.choice.weather = f;
        l.state.touched();
    }
}

fn step_roadbook(l: &mut Launcher, r: Rect) {
    let (Some(line), Some(tour), false) = (l.state.line().cloned(), l.state.tour().cloned(), l.state.choice.free) else {
        let map = l.state.map().map(|m| if m.friendly.is_empty() { m.name.clone() } else { m.friendly.clone() }).unwrap_or_default();
        l.ui.paragraph(&format!("No duty chosen: free driving on {map}. Pick a line and a tour under Route to see the roadbook here."), Vec2::new(r.x, r.y), r.w, 13.0, Weight::Regular, TEXT_DIM);
        ibis_box(l, Rect::new(r.x, r.y + 60.0, r.w, 160.0));
        return;
    };
    let from = l.state.first_trip().unwrap_or(0);
    l.ui.text_in(&format!("Line {} · tour {} · from {}", line.name, tour.number, hhmm(l.state.choice.time as f64 * 60.0)), Rect::new(r.x, r.y, r.w, 22.0), 14.0, Weight::Bold, TEXT, Align::Left);
    let trips: Vec<omsi_launcher_lib::TripInfo> = tour.trips.iter().skip(from).cloned().collect();
    let ibis_h = 150.0;
    let list = Rect::new(r.x - 4.0, r.y + 30.0, r.w + 8.0, r.h - 30.0 - ibis_h - 10.0);
    l.ui.scroll_area("roadbook", list, &mut |ui, v| {
        let mut y = v.y;
        for (k, t) in trips.iter().enumerate() {
            let head = Rect::new(v.x + 4.0, y, v.w - 12.0, 46.0);
            ui.p().rounded(head, 6.0, if k == 0 { SELECTED } else { FIELD });
            ui.text_in(&format!("{} · {} → {}", if k == 0 { "Your first trip" } else { "Then" }, if t.from.is_empty() { "?" } else { &t.from }, t.terminus), Rect::new(head.x + 10.0, head.y + 4.0, head.w - 20.0, 20.0), 13.0, Weight::Bold, TEXT, Align::Left);
            ui.text_in(&format!("{} - {} · {:.1} km · {}{}", hhmm(t.departure), hhmm(t.arrival), t.km, if t.line.is_empty() { "depot run".to_string() } else { format!("line {}", t.line) }, format!(" · {}", t.name)), Rect::new(head.x + 10.0, head.y + 24.0, head.w - 20.0, 18.0), 11.0, Weight::Regular, TEXT_DIM, Align::Left);
            y += 52.0;
            let n = t.stops.len();
            for (s, st) in t.stops.iter().enumerate() {
                let rr = Rect::new(v.x + 4.0, y, v.w - 12.0, 24.0);
                // the timeline: a line with a dot per stop
                let cx = rr.x + 60.0;
                if s + 1 < n {
                    ui.p().rect(Rect::new(cx - 1.0, rr.center().y, 2.0, 24.0), Color::WHITE.alpha(0.12));
                }
                let end = s == 0 || s + 1 == n;
                ui.p().circle(Vec2::new(cx, rr.center().y), if end { 4.0 } else { 3.0 }, if end { TEXT } else { TEXT_FAINT });
                ui.text_in(&hhmm(st.arr), Rect::new(rr.x + 6.0, rr.y, 42.0, rr.h), 12.0, Weight::Condensed, if end { TEXT } else { TEXT_SOFT }, Align::Left);
                ui.text_in(&st.name, Rect::new(cx + 14.0, rr.y, rr.w - 140.0, rr.h), 12.5, if end { Weight::Bold } else { Weight::Regular }, TEXT, Align::Left);
                if s == 0 {
                    ui.text_in(&format!("dep {}", hhmm(st.dep)), Rect::new(rr.right() - 80.0, rr.y, 76.0, rr.h), 11.0, Weight::Medium, TEXT_DIM, Align::Right);
                }
                y += 24.0;
            }
            y += 12.0;
        }
        y - v.y
    });
    ibis_box(l, Rect::new(r.x, r.bottom() - ibis_h, r.w, ibis_h));
}

fn ibis_box(l: &mut Launcher, r: Rect) {
    l.ui.p().rounded(r, 6.0, FIELD);
    let inner = l.ui.heading(Rect::new(r.x + 12.0, r.y + 10.0, r.w - 24.0, r.h - 20.0), "IBIS", Some("keyboard"));
    let Some((_, info)) = l.state.ibis.clone() else {
        l.ui.paragraph("Pick a line to see what to type into the IBIS. Shift+U in the game types it for you and puts the bus into service.", Vec2::new(inner.x, inner.y - 4.0), inner.w, 12.0, Weight::Regular, TEXT_DIM);
        return;
    };
    match info {
        Err(e) => {
            l.ui.paragraph(&e, Vec2::new(inner.x, inner.y - 4.0), inner.w, 12.0, Weight::Regular, DANGER);
        }
        Ok(i) if i.routes.is_empty() => {
            l.ui.paragraph(&format!("Depot file {} has no entries for this line. Type line {} and the terminus code by hand, or press Shift+U.", i.hof, i.line_code), Vec2::new(inner.x, inner.y - 4.0), inner.w, 12.0, Weight::Regular, TEXT_DIM);
        }
        Ok(i) => {
            let mut y = inner.y - 2.0;
            for rt in i.routes.iter().take(3) {
                let code = rt.code.clone();
                let (lc, rc) = if code.len() > 2 { (code[..code.len() - 2].to_string(), code[code.len() - 2..].to_string()) } else { (i.line_code.clone(), code.clone()) };
                let name = if rt.name.is_empty() { rt.terminus.clone() } else { rt.name.clone() };
                l.ui.text_in(&name, Rect::new(inner.x, y, inner.w - 170.0, 22.0), 12.5, Weight::Medium, TEXT, Align::Left);
                let mut x = inner.right() - 160.0;
                for (label, v) in [("line", lc), ("route", rc)] {
                    l.ui.text_in(label, Rect::new(x, y, 34.0, 22.0), 11.0, Weight::Regular, TEXT_DIM, Align::Left);
                    let cw = l.ui.width(&v, 13.0, Weight::Black) + 12.0;
                    let cr = Rect::new(x + 34.0, y + 2.0, cw, 18.0);
                    l.ui.p().rounded(cr, 4.0, SELECTED);
                    l.ui.text_in(&v, cr, 12.5, Weight::Bold, TEXT, Align::Center);
                    x += 80.0;
                }
                y += 26.0;
            }
            l.ui.text_in(&format!("Depot file {} · Shift+U in the game types it for you", i.hof), Rect::new(inner.x, y + 2.0, inner.w, 18.0), 11.0, Weight::Regular, TEXT_FAINT, Align::Left);
        }
    }
}

/// The right column: the bus preview, the duty in short, the start button.
fn summary(l: &mut Launcher, side: Rect) {
    let pw = side.w;
    let ph = (pw / 1.6).min(side.h * 0.55);
    let pr = Rect::new(side.x, side.y, pw, ph);
    l.preview(pr);
    let mut y = pr.bottom() + 18.0;
    let (bus_name, maker) = l.state.bus().map(|b| (b.name.clone(), b.manufacturer.clone())).unwrap_or_else(|| ("No bus chosen".into(), String::new()));
    if !maker.is_empty() {
        l.ui.text_in(&maker, Rect::new(side.x, y, pw, 16.0), 12.0, Weight::Regular, TEXT_DIM, Align::Left);
        y += 18.0;
    }
    l.ui.text_in(&bus_name, Rect::new(side.x, y, pw, 24.0), 18.0, Weight::Bold, TEXT, Align::Left);
    y += 26.0;
    let paint = if l.state.choice.paint.is_empty() { "Default paint".to_string() } else { l.state.choice.paint.clone() };
    l.ui.text_in(&paint, Rect::new(side.x, y, pw, 18.0), 12.5, Weight::Regular, TEXT_DIM, Align::Left);
    y += 30.0;
    let map = l.state.map().map(|m| if m.friendly.is_empty() { m.name.clone() } else { m.friendly.clone() }).unwrap_or_else(|| "-".into());
    let duty = match (&l.state.choice.line, &l.state.choice.tour, l.state.choice.free) {
        (_, _, true) | (None, _, _) => "Free drive".to_string(),
        (Some(line), Some(t), _) => format!("Line {line}, tour {t}"),
        (Some(line), None, _) => format!("Line {line}, choose a tour"),
    };
    let weather = match l.state.choice.weather.strip_prefix("metar:") {
        Some(code) => format!("Current weather at {code}"),
        None if l.state.choice.weather == "cycle" => "Weather cycle".into(),
        None => l.state.weathers.iter().find(|w| w.file == l.state.choice.weather).map(|w| w.name.clone()).unwrap_or_else(|| "Map default".into()),
    };
    let (yy, mm, dd) = super::ui::parse_date(&l.state.choice.date);
    let mut start_at = format!("{:02}:{:02}, {dd} {} {yy}", l.state.choice.time / 60, l.state.choice.time % 60, super::ui::MONTHS[(mm as usize).clamp(1, 12) - 1]);
    let mut weather = weather;
    // on a server: its clock and weather
    if let Some(i) = l.state.joined_server.as_ref().and_then(|a| l.state.server_info.get(a)).and_then(|x| x.1.as_ref().ok()) {
        start_at = format!("{} (the server's clock)", i.time);
        weather = if i.weather.is_empty() { "the map's (the server's)".into() } else { format!("{} (the server's)", i.weather) };
    }
    let rows = [
        ("Map", map),
        ("Duty", duty),
        ("Start", start_at),
        ("Weather", weather),
    ];
    for (k, v) in rows {
        l.ui.text_in(k, Rect::new(side.x, y, 90.0, 22.0), 12.5, Weight::Regular, TEXT_DIM, Align::Left);
        l.ui.text_in(&v, Rect::new(side.x + 90.0, y, pw - 90.0, 22.0), 12.5, Weight::Regular, TEXT, Align::Left);
        y += 24.0;
    }
    if let Some(b) = l.state.bus().filter(|b| !b.missing_packs.is_empty()) {
        y += 8.0;
        let text = format!("Parts missing: needs {}", b.missing_packs.join(", "));
        l.ui.icon("warning", Vec2::new(side.x + 9.0, y + 10.0), 16.0, WARN);
        l.ui.text_in(&text, Rect::new(side.x + 24.0, y, pw - 24.0, 20.0), 12.5, Weight::Medium, WARN, Align::Left);
    }
    // on a server: which, and the way back to playing alone
    if let Some(name) = joined_server_name(l) {
        y += 12.0;
        l.ui.icon("dns", Vec2::new(side.x + 9.0, y + 11.0), 16.0, OK);
        l.ui.text_in(&format!("Server: {name}"), Rect::new(side.x + 24.0, y, pw - 170.0, 22.0), 13.0, Weight::Medium, OK, Align::Left);
        if l.ui.button("leave-server", Rect::new(side.x + pw - 150.0, y - 6.0, 150.0, 34.0), "Leave Server", Some("logout"), ButtonKind::Danger) {
            l.state.leave_server();
        }
    }
    let running = l.state.instances.iter().filter(|i| i.running).count();
    // the start: the whole width of the column, at its foot
    let note_h = if running > 0 { 24.0 } else { 0.0 };
    let btn = Rect::new(side.x, side.bottom() - 48.0 - note_h, pw, 48.0);
    let free = l.state.choice.free || l.state.choice.line.is_none();
    let label = if running > 0 && l.state.second_armed.map(|t| t.elapsed().as_secs() < 6).unwrap_or(false) {
        "Start another game"
    } else if free && l.state.joined_server.is_none() {
        "Drive"
    } else {
        "Start the duty"
    };
    if l.ui.button("launch", btn, label, Some("play_arrow"), ButtonKind::Primary) {
        start(l);
    }
    // where the last game on this map was left (`laststn.osn`): a second way in
    if l.state.joined_server.is_none() && l.state.has_last_situation() {
        let cont = Rect::new(btn.x, btn.y - 44.0, pw, 38.0);
        if l.ui.button("continue", cont, "Continue where you left off", Some("history"), ButtonKind::Normal) {
            l.state.launch_last_situation();
        }
    }
    if running > 0 {
        let note = format!("{running} game{} running - see Sessions", if running > 1 { "s" } else { "" });
        let nr = Rect::new(btn.x, btn.bottom() + 4.0, pw, 20.0);
        let (h, _, clicked) = l.ui.interact(id_of("running-note"), nr);
        if clicked {
            l.go(super::Page::Sessions);
        }
        l.ui.text_in(&note, nr, 12.5, Weight::Regular, if h { TEXT } else { OK }, Align::Center);
    }
    let _ = fmt_bytes;
}

/// The phone's Start: as the desktop's.
pub(super) fn start_from_phone(l: &mut Launcher) {
    start(l);
}

fn start(l: &mut Launcher) {
    if l.state.bus().is_none() || l.state.map().is_none() {
        l.state.set_status("Choose a bus and a map first.", true);
        return;
    }
    if l.state.choice.lan_mode == "join" && !l.state.join.0 {
        let t = l.state.join.1.clone();
        l.state.set_status(format!("LAN: {t}"), true);
        return;
    }
    let running = l.state.instances.iter().filter(|i| i.running).count();
    // a second game on one computer is for testing LAN play, not something to do by
    // accident: with one running, the button asks for a second click
    if running > 0 && l.state.second_armed.map(|t| t.elapsed().as_secs() >= 6).unwrap_or(true) {
        l.state.second_armed = Some(std::time::Instant::now());
        l.state.set_status("A game is running already (its window may be behind this one - see Sessions). Click again to start another one anyway.", true);
        return;
    }
    l.state.second_armed = None;
    l.state.choice.save();
    l.state.launch();
    if l.state.choice.lan_mode != "off" {
        l.go(super::Page::Sessions);
    }
}

/// The airport of OMSI's METAR list (`Weather/ICAO.txt`) nearest to where map `map` lies
/// (its `timezone.txt`), else Berlin's; read once per map.
fn nearest_airport(root: &str, map: &str) -> String {
    static CACHE: std::sync::Mutex<Option<hashbrown::HashMap<String, String>>> = std::sync::Mutex::new(None);
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let cache = cache.get_or_insert_with(Default::default);
    if let Some(c) = cache.get(map) {
        return c.clone();
    }
    // (the airports of the list round Europe and a little beyond, where OMSI's maps lie)
    const AIRPORTS: &[(&str, f64, f64)] = &[
        ("BKPR", 42.57, 21.04), ("EBBR", 50.90, 4.48), ("EDDH", 53.63, 9.99), ("EDDM", 48.35, 11.79),
        ("EDDN", 49.50, 11.08), ("EDDP", 51.42, 12.24), ("EDDS", 48.69, 9.22), ("EDDB", 52.37, 13.52),
        ("EDOP", 53.43, 11.78), ("EETN", 59.41, 24.83), ("EFHF", 60.25, 25.04), ("EGAA", 54.66, -6.22),
        ("EGCC", 53.35, -2.27), ("EGLL", 51.47, -0.45), ("EGPH", 55.95, -3.37), ("EHAM", 52.31, 4.76),
        ("EIDW", 53.42, -6.27), ("EKBI", 55.74, 9.15), ("ELLX", 49.63, 6.21), ("ENBR", 60.29, 5.22),
        ("ENGM", 60.19, 11.10), ("ESKN", 58.79, 16.91), ("EPKK", 50.08, 19.78), ("EPWA", 52.17, 20.97),
        ("EVRA", 56.92, 23.97), ("EYVI", 54.63, 25.29), ("LBSF", 42.70, 23.41), ("LDZA", 45.74, 16.07),
        ("LEAL", 38.28, -0.56), ("LEBB", 43.30, -2.91), ("LEMD", 40.47, -3.56), ("LEPA", 39.55, 2.74),
        ("LFBD", 44.83, -0.72), ("LFLY", 45.73, 5.08), ("LFMD", 43.54, 6.95), ("LFPG", 49.01, 2.55),
        ("LGAV", 37.94, 23.94), ("LHBP", 47.44, 19.26), ("LICJ", 38.18, 13.10), ("LIMC", 45.63, 8.72),
        ("LIRA", 41.80, 12.59), ("LOWI", 47.26, 11.34), ("LOWL", 48.23, 14.19), ("LOWW", 48.11, 16.57),
        ("LPPT", 38.78, -9.14), ("LQSA", 43.82, 18.33), ("LSGG", 46.24, 6.11), ("LSZH", 47.46, 8.55),
        ("LTAC", 40.13, 32.99), ("LWSK", 41.96, 21.62), ("LYBE", 44.82, 20.31), ("LYTV", 42.40, 18.72),
        ("LZIB", 48.17, 17.21), ("UKKK", 50.40, 30.45), ("ULLI", 59.80, 30.26), ("UMKK", 54.89, 20.59),
        ("UMMM", 53.88, 28.03), ("UUEE", 55.97, 37.41), ("USSS", 56.74, 60.80),
    ];
    let dir = std::path::Path::new(map).parent().map(|d| d.to_string_lossy().to_string()).unwrap_or_default();
    let tz = omsi_cfg::resolve_path(std::path::Path::new(root), &format!("{dir}/timezone.txt"));
    let code = omsi_map::TimeZone::load(&tz)
        .ok()
        .and_then(|t| t.lat_lon())
        .and_then(|(lat, lon)| {
            AIRPORTS
                .iter()
                .map(|(c, a, o)| (c, (a - lat).powi(2) + ((o - lon) * lat.to_radians().cos()).powi(2)))
                .min_by(|x, y| x.1.total_cmp(&y.1))
                .map(|x| x.0.to_string())
        })
        .unwrap_or_else(|| "EDDB".into());
    cache.insert(map.to_string(), code.clone());
    code
}
