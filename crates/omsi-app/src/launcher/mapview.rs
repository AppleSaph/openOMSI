//! The launcher's map picture: the chosen map as its own data has it - every spline's centre
//! line, where its entry points stand, where its objects are, and the road pieces the chosen
//! trip drives - read from the tile files alone. No `.sli`, no `.sco`, no lane network, no
//! meshes: the picture is a plan of the map's road data, drawn on a worker thread and kept
//! as one texture the interface shows in a card (`Launcher::map_preview`).
//!
//! It answers what the Route step's lists cannot: on the left a line, its termini and a
//! count of tours; here where that line actually goes, which way it turns, where the bus
//! would be put down, and how big the map around it is.
//!
//! Reading every tile is the one thing here that costs anything (0.1 s for Grundorf's 19
//! tiles, 0.9 s for a mod map's 891) and it runs once per map selection, off the interface
//! thread, with the picture saying so while it does. Everything else - the trip's route, the
//! markers, the drawing - is a few milliseconds on the thread that asked.

use glam::{DVec2, Mat4, Vec3};
use hashbrown::HashMap;
use omsi_render::Renderer;
use omsi_ui::{Color, Draw, Gpu, Layer, Painter, Vertex};
use rayon::prelude::*;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;

// The launcher is dark (see `launcher/theme.rs`): light roads, one red route over them.
const ROAD: Color = Color::rgba(150, 154, 162, 1.0);
const ROUTE: Color = Color::rgba(224, 70, 58, 1.0);
const STOP: Color = Color::rgba(238, 238, 238, 1.0);
const ENTRY: Color = Color::rgba(96, 160, 232, 1.0);
const HERE: Color = Color::rgba(255, 255, 255, 1.0);
const HERE_RING: Color = Color::rgba(18, 18, 18, 1.0);
/// Roads are drawn this thin, the route this thick (pixels at whatever zoom).
const ROAD_PX: f32 = 1.5;
const ROUTE_PX: f32 = 3.2;
const STOP_PX: f32 = 2.8;
const ENTRY_PX: f32 = 3.2;

/// What the picture should show. Changing `map` reads the tiles again; changing `trip` only
/// walks the timetable; changing `entry` only moves the ring.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Look {
    /// The map as the launcher names it (`maps/<name>/global.cfg`), and where that is.
    pub map: String,
    pub global: PathBuf,
    /// The date the duty starts on (the chrono folders change the map and the timetable).
    pub date: String,
    /// The trip whose route is drawn (empty: the map alone).
    pub trip: String,
    /// The entry point the player picked, -1 = automatic (nothing is ringed).
    pub entry: i32,
}

/// A map as the picture needs it: the splines' centre lines, the entry points and the
/// objects' places, all in world metres.
#[derive(Default)]
struct Roads {
    /// Every road piece's points, one after another.
    points: Vec<[f32; 2]>,
    /// (tile x, tile y, spline id) → its range in `points`.
    spans: HashMap<(i32, i32, i64), (u32, u32)>,
    /// The world rectangle the splines cover.
    lo: DVec2,
    hi: DVec2,
    /// The entry points (world place, name), in `global.cfg`'s order.
    entries: Vec<(DVec2, String)>,
    /// Every placed object's world place: the trip's stops are found here.
    objects: HashMap<i64, DVec2>,
    tiles: usize,
    splines: usize,
}

/// What a worker read.
struct Reply {
    look: Look,
    roads: Option<Arc<Roads>>,
    trip: omsi_launcher_lib::TripPath,
    /// What went wrong, when nothing could be read at all.
    error: Option<String>,
}

/// The map picture: what it shows, what it was asked for, and the texture.
pub struct MapView {
    want: Option<Look>,
    shown: Option<Look>,
    roads: Option<Arc<Roads>>,
    trip: omsi_launcher_lib::TripPath,
    error: Option<String>,
    loading: Option<Receiver<Reply>>,
    /// The picture must be drawn again (the choice changed or the card was resized).
    dirty: bool,
    gpu: Option<Gpu>,
    target: Option<(wgpu::Texture, wgpu::TextureView, u32, u32)>,
    /// Bumped whenever `target` was made anew (the interface binds it again).
    pub generation: u64,
}

impl MapView {
    pub fn new() -> MapView {
        MapView { want: None, shown: None, roads: None, trip: Default::default(), error: None, loading: None, dirty: false, gpu: None, target: None, generation: 0 }
    }

    /// What the picture should show. Called every frame with the current choice; only a
    /// change starts any work.
    pub fn want(&mut self, look: Look) {
        // (before the map list has arrived: nothing to read yet)
        if look.map.trim().is_empty() {
            return;
        }
        if self.want.as_ref() != Some(&look) {
            self.want = Some(look);
        }
    }

    /// What to say while there is no picture yet.
    pub fn status(&self) -> &'static str {
        if self.loading.is_some() || (self.want.is_some() && self.shown.is_none()) {
            "Reading the map…"
        } else if self.error.is_some() {
            "The map cannot be read"
        } else if self.roads.as_deref().map(|r| r.splines == 0).unwrap_or(false) {
            "This map has no road splines"
        } else {
            ""
        }
    }

    /// A worker is reading (the card shows its little spinner).
    pub fn busy(&self) -> bool {
        self.loading.is_some()
    }

    /// The counts the legend says: (road pieces, the trip's stops, the map's entry points).
    pub fn counts(&self) -> Option<(usize, usize, usize)> {
        let r = self.roads.as_deref()?;
        Some((r.splines, self.trip.stops.len(), r.entries.len()))
    }

    /// Start whatever the current choice still needs, and take what a worker finished.
    fn pump(&mut self) {
        if let Some(rx) = self.loading.as_ref() {
            match rx.try_recv() {
                Ok(r) => {
                    self.loading = None;
                    if let Some(roads) = r.roads {
                        self.roads = Some(roads);
                    }
                    self.trip = r.trip;
                    self.error = r.error;
                    self.shown = Some(r.look);
                    self.dirty = true;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => return,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.loading = None;
                    self.error = Some("the worker went".into());
                }
            }
        }
        let Some(want) = self.want.clone() else { return };
        if self.shown.as_ref() == Some(&want) {
            return;
        }
        // only the ring moved: nothing has to be read again
        if self.shown.as_ref().map(|s| s.map == want.map && s.date == want.date && s.trip == want.trip).unwrap_or(false) {
            self.shown = Some(want);
            self.dirty = true;
            return;
        }
        // the tiles of the map are read once; another trip on the same map only walks the
        // timetable again
        let have = self.roads.clone().filter(|_| self.shown.as_ref().map(|s| s.global == want.global && s.date == want.date).unwrap_or(false));
        let (tx, rx) = channel();
        let w = want.clone();
        let spawned = std::thread::Builder::new().name("launcher map".into()).spawn(move || {
            let _ = tx.send(read(w, have));
        });
        match spawned {
            Ok(_) => self.loading = Some(rx),
            Err(e) => {
                self.error = Some(e.to_string());
                self.shown = Some(want);
            }
        }
    }

    /// The map at `w` x `h` pixels, drawn again when the choice changed.
    pub fn picture(&mut self, renderer: &Renderer, w: u32, h: u32) -> Option<wgpu::TextureView> {
        self.pump();
        let (w, h) = (w.max(64), h.max(64));
        if self.target.as_ref().map(|t| (t.2, t.3) != (w, h)).unwrap_or(true) {
            let tex = renderer.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("launcher map"),
                size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: renderer.format(),
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            let view = tex.create_view(&Default::default());
            self.target = Some((tex, view, w, h));
            self.generation += 1;
            self.dirty = true;
        }
        self.roads.as_ref()?;
        if self.dirty {
            self.dirty = false;
            let view = self.target.as_ref()?.1.clone();
            self.draw(renderer, &view, w, h);
        }
        Some(self.target.as_ref()?.1.clone())
    }

    /// Build the vertex list (roads, the route, the markers) and draw the one layer.
    fn draw(&mut self, renderer: &Renderer, target: &wgpu::TextureView, w: u32, h: u32) {
        let (w, h) = (w as f32, h as f32);
        let roads = self.roads.clone().unwrap();
        // the view: the trip's own route when there is one (that is what the player is
        // choosing), else the whole map
        let (mut lo, mut hi) = (roads.lo, roads.hi);
        let route_pts: Vec<Vec<DVec2>> = self
            .trip
            .route
            .iter()
            .filter_map(|p| roads.spans.get(&(p.tile_x, p.tile_y, p.spline)).copied())
            .map(|(a, n)| roads.points[a as usize..(a + n) as usize].iter().map(|q| DVec2::new(q[0] as f64, q[1] as f64)).collect())
            .collect();
        if !route_pts.is_empty() {
            let (mut l, mut h2) = (DVec2::splat(f64::MAX), DVec2::splat(f64::MIN));
            for p in route_pts.iter().flatten() {
                l = l.min(*p);
                h2 = h2.max(*p);
            }
            // (room around it: the line alone says nothing about where in the map it runs)
            let pad = (h2 - l).max_element().max(200.0) * 0.2;
            lo = l - DVec2::splat(pad);
            hi = h2 + DVec2::splat(pad);
            log::info!("map picture: the route is {} of {} pieces (the map's own links may name pieces it no longer has)", route_pts.len(), self.trip.route.len());
        }
        let span = (hi - lo).max(DVec2::splat(1.0));
        let mpp = (span.x / w as f64).max(span.y / h as f64) * 1.04;
        let centre = (lo + hi) * 0.5;
        let (hw, hh) = (w as f64 * 0.5 * mpp, h as f64 * 0.5 * mpp);
        let proj = Mat4::orthographic_rh((centre.x - hw) as f32, (centre.x + hw) as f32, (centre.y - hh) as f32, (centre.y + hh) as f32, -100.0, 100.0);
        let rel = |p: DVec2| Vec3::new((p.x - centre.x) as f32, (p.y - centre.y) as f32, 0.0);
        // (a spline's points are 8 m apart: at this zoom most of them are inside one pixel)
        let step = ((mpp / 8.0).floor() as usize).max(1);

        let mut p = Painter::new();
        // 1. the map's roads
        for &(a, n) in roads.spans.values() {
            let pts = &roads.points[a as usize..(a + n) as usize];
            let mut v: Vec<Vec3> = Vec::with_capacity(pts.len() / step + 1);
            let mut k = 0;
            while k < pts.len() {
                v.push(rel(DVec2::new(pts[k][0] as f64, pts[k][1] as f64)));
                k += step;
            }
            v.push(rel(DVec2::new(pts[pts.len() - 1][0] as f64, pts[pts.len() - 1][1] as f64)));
            p.ribbon(&v, 0.0, ROAD_PX, ROAD, false);
        }
        // 2. the trip's route over them
        for pts in &route_pts {
            let mut v: Vec<Vec3> = Vec::with_capacity(pts.len() / step + 1);
            let mut k = 0;
            while k < pts.len() {
                v.push(rel(pts[k]));
                k += step;
            }
            v.push(rel(pts[pts.len() - 1]));
            p.ribbon(&v, 0.0, ROUTE_PX, ROUTE, false);
        }
        // 3. the stops it calls at
        for id in &self.trip.stops {
            if let Some(q) = roads.objects.get(id) {
                p.world_disc(rel(*q), 0.0, STOP_PX, STOP);
            }
        }
        // 4. where a bus can be put down, and which one is chosen
        let chosen = self.shown.as_ref().map(|s| s.entry).unwrap_or(-1);
        for (i, (q, _)) in roads.entries.iter().enumerate() {
            if chosen == i as i32 {
                p.world_disc(rel(*q), 0.0, ENTRY_PX + 2.0, HERE_RING);
                p.world_disc(rel(*q), 0.0, ENTRY_PX, HERE);
            } else {
                p.world_disc(rel(*q), 0.0, ENTRY_PX, ENTRY);
            }
        }
        let verts: Vec<Vertex> = p.verts;
        let count = verts.len() as u32;
        let layer = Layer { view_proj: proj, viewport: [0.0, 0.0, w, h], clip: [0.0, 0.0, w, h], radius: 0.0, opacity: 1.0, px_scale: mpp as f32 };
        let draws = [Draw { buffer: 0, range: 0..count, layer: 0, texture: 0 }];

        let device = &renderer.device;
        let queue = &renderer.queue;
        // (the pipeline wants an atlas whatever is drawn: the map has no text of its own)
        let gpu = self.gpu.get_or_insert_with(|| Gpu::new(device, renderer.format(), 1, 64));
        gpu.upload(device, queue, 0, &verts);
        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("launcher map") });
        gpu.render(device, queue, &mut enc, target, (w as u32, h as u32), Some(wgpu::Color::TRANSPARENT), &[layer], &draws);
        queue.submit([enc.finish()]);
    }
}

/// Read a map the way the picture needs it: every tile's splines as centre lines, where its
/// objects stand, where its entry points are - and the chosen trip's route with them.
fn read(look: Look, have: Option<Arc<Roads>>) -> Reply {
    let mut reply = Reply { look, roads: None, trip: Default::default(), error: None };
    if let Some(r) = have {
        reply.trip = trip_of(&reply.look);
        reply.roads = Some(r);
        return reply;
    }
    let roads = read_map(&reply.look);
    if roads.splines == 0 && roads.tiles == 0 {
        reply.error = Some(format!("{}", reply.look.global.display()));
    }
    reply.trip = trip_of(&reply.look);
    reply.roads = Some(Arc::new(roads));
    reply
}

/// The route of the chosen trip (nothing when no trip is chosen or the map has a timetable
/// that does not know it).
fn trip_of(look: &Look) -> omsi_launcher_lib::TripPath {
    if look.trip.trim().is_empty() {
        return Default::default();
    }
    match omsi_launcher_lib::trip_path(&look.map, &look.date, &look.trip) {
        Ok(t) => t,
        Err(e) => {
            log::debug!("trip path for {}: {e}", look.trip);
            Default::default()
        }
    }
}

/// The map's own data, read tile by tile (in parallel: a big map is 891 files).
fn read_map(look: &Look) -> Roads {
    let Ok(global) = omsi_map::GlobalCfg::load(&look.global) else { return Roads::default() };
    // everything that turns a tile index and a place in the tile into world metres needs
    // this first (`[worldcoordinates]` maps use another grid)
    omsi_map::configure_grid(&global);
    let Some(map_dir) = look.global.parent().map(Path::to_path_buf) else { return Roads::default() };
    let chrono = omsi_map::date_code(&look.date).map(|c| omsi_map::active_chrono_dirs(&map_dir, c)).unwrap_or_default();
    let size = omsi_map::tile_size();
    // per tile: its splines' points, its objects, and the rectangle they cover
    struct Part {
        points: Vec<[f32; 2]>,
        spans: Vec<((i32, i32, i64), (u32, u32))>,
        objects: Vec<(i64, DVec2)>,
        lo: DVec2,
        hi: DVec2,
        read: bool,
    }
    let parts: Vec<Part> = global
        .tiles
        .par_iter()
        .map(|t| {
            let mut part = Part { points: Vec::new(), spans: Vec::new(), objects: Vec::new(), lo: DVec2::splat(f64::MAX), hi: DVec2::splat(f64::MIN), read: false };
            let path = omsi_cfg::resolve_path(&map_dir, &t.file);
            let Some(tile) = crate::tiles::read_tile(&path, &chrono) else { return part };
            part.read = true;
            let origin = DVec2::new(t.x as f64 * size, t.y as f64 * size);
            for s in tile.splines.iter().filter(|s| !s.deleted && !s.file.trim().is_empty()) {
                let curve = omsi_geometry::SplineCurve::from_map(s, origin);
                // a point every 8 m is finer than any zoom the card is shown at
                let n = ((curve.length / 8.0).ceil() as usize).clamp(1, 64);
                let first = part.points.len() as u32;
                for k in 0..=n {
                    let q = curve.point_at(curve.length * k as f64 / n as f64);
                    if !(q.x.is_finite() && q.y.is_finite()) {
                        continue;
                    }
                    part.points.push([q.x as f32, q.y as f32]);
                    part.lo = part.lo.min(q.truncate());
                    part.hi = part.hi.max(q.truncate());
                }
                if part.points.len() as u32 - first >= 2 {
                    part.spans.push(((t.x, t.y, s.id), (first, part.points.len() as u32 - first)));
                }
            }
            for o in &tile.objects {
                part.objects.push((o.id, DVec2::new(origin.x + o.pos[0], origin.y + o.pos[1])));
            }
            part
        })
        .collect();

    let mut roads = Roads { lo: DVec2::splat(f64::MAX), hi: DVec2::splat(f64::MIN), ..Default::default() };
    for part in parts {
        roads.tiles += part.read as usize;
        roads.splines += part.spans.len();
        let base = roads.points.len() as u32;
        roads.points.extend_from_slice(&part.points);
        roads.spans.extend(part.spans.into_iter().map(|(k, (a, n))| (k, (a + base, n))));
        roads.objects.extend(part.objects);
        if part.lo.x != f64::MAX {
            roads.lo = roads.lo.min(part.lo);
            roads.hi = roads.hi.max(part.hi);
        }
    }
    // the entry points: on the tile the record names, else at their object (`World::
    // entry_point_place` reads them the same way)
    for ep in &global.entry_points {
        let p = usize::try_from(ep.group)
            .ok()
            .and_then(|i| global.raw_tiles.get(i))
            .map(|t| DVec2::new(t.0 as f64 * size + ep.pos[0], t.1 as f64 * size + ep.pos[1]))
            .or_else(|| roads.objects.get(&ep.object_id).copied());
        if let Some(p) = p {
            roads.entries.push((p, ep.name.clone()));
        }
    }
    if roads.lo.x == f64::MAX {
        roads.lo = DVec2::ZERO;
        roads.hi = DVec2::ZERO;
    } else {
        // Where the roads really are: one spline left in a far corner (an editor's marker, a
        // piece of the next town) or one entry point off the map would shrink everything
        // else to a dot, so the picture takes the 1 - 99 % box instead of the extremes.
        let mut xs: Vec<f32> = roads.points.iter().map(|p| p[0]).collect();
        let mut ys: Vec<f32> = roads.points.iter().map(|p| p[1]).collect();
        xs.sort_by(f32::total_cmp);
        ys.sort_by(f32::total_cmp);
        let at = |v: &Vec<f32>, t: f64| v[((v.len() - 1) as f64 * t) as usize] as f64;
        let (x0, x1) = (at(&xs, 0.01), at(&xs, 0.99));
        let (y0, y1) = (at(&ys, 0.01), at(&ys, 0.99));
        log::info!(
            "map picture: the roads cover ({:.0}, {:.0}) - ({:.0}, {:.0}), 1 - 99 %: ({:.0}, {:.0}) - ({:.0}, {:.0}), entry points ({:.0}, {:.0}) - ({:.0}, {:.0})",
            roads.lo.x, roads.lo.y, roads.hi.x, roads.hi.y, x0, y0, x1, y1,
            roads.entries.iter().map(|e| e.0.x).fold(f64::MAX, f64::min),
            roads.entries.iter().map(|e| e.0.y).fold(f64::MAX, f64::min),
            roads.entries.iter().map(|e| e.0.x).fold(f64::MIN, f64::max),
            roads.entries.iter().map(|e| e.0.y).fold(f64::MIN, f64::max)
        );
        roads.lo = DVec2::new(x0, y0);
        roads.hi = DVec2::new(x1, y1);
    }
    log::info!("map picture: {} tiles, {} splines, {} objects, {} entry points in 1 read", roads.tiles, roads.splines, roads.objects.len(), roads.entries.len());
    roads
}
