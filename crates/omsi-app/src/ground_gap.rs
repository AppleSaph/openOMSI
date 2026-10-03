//! `OMSI_GROUND_GAP=<csv>` (offscreen runs): how far every drawn tyre stands over or sinks
//! into what is drawn under it, frame by frame, for the player's vehicle and the AI cars
//! near it, and how much the bodies jump in height. A summary goes to the log at the end;
//! the csv (if the path is not `-`) has one line per wheel and frame.

use std::collections::HashMap;
use std::io::Write;
use std::sync::Arc;

use glam::{DVec3, Mat4, Vec3};

use crate::scene::World;

/// One wheel of a model: the meshes turned by the same `Wheel_Rotation_*` variable, taken
/// by the tallest (the tyre, not its hub cap): its centre and half height at rest.
type Wheels = Vec<(usize, Vec3, f32)>;

#[derive(Default)]
struct Stats {
    n: usize,
    sum: f64,
    sum_abs: f64,
    max_over: f64,
    max_under: f64,
    over5: usize,
    under5: usize,
    no_ground: usize,
    /// (gap, where, frame time)
    worst: Vec<(f64, DVec3, f32)>,
    jumps: usize,
    max_jump: f64,
    bodies: usize,
}

impl Stats {
    fn add(&mut self, gap: f64, at: DVec3, t: f32) {
        self.n += 1;
        self.sum += gap;
        self.sum_abs += gap.abs();
        self.max_over = self.max_over.max(gap);
        self.max_under = self.max_under.min(gap);
        if gap > 0.05 {
            self.over5 += 1;
        }
        if gap < -0.05 {
            self.under5 += 1;
        }
        if gap.abs() > 0.05 {
            self.worst.push((gap, at, t));
            if self.worst.len() > 4000 {
                self.trim();
            }
        }
    }

    fn trim(&mut self) {
        self.worst.sort_by(|a, b| b.0.abs().total_cmp(&a.0.abs()));
        // one per place (10 m apart), the worst first
        let mut kept: Vec<(f64, DVec3, f32)> = Vec::new();
        for w in self.worst.drain(..) {
            if kept.len() < 40 && !kept.iter().any(|k| (k.1 - w.1).truncate().length() < 10.0) {
                kept.push(w);
            }
        }
        self.worst = kept;
    }
}

pub struct GroundGap {
    file: Option<std::fs::File>,
    wheels: HashMap<usize, Arc<Wheels>>,
    player: Stats,
    ai: Stats,
    /// Last two heights of each body (key: car id, or u64::MAX for the player and the
    /// part index below it for its trailers).
    heights: HashMap<u64, (f64, f64, usize)>,
    radius: f64,
}

impl GroundGap {
    pub fn from_env() -> Option<GroundGap> {
        let path = omsi_cfg::env::var("OMSI_GROUND_GAP").ok()?;
        let file = (path != "-").then(|| std::fs::File::create(&path).ok()).flatten();
        let mut g = GroundGap {
            file,
            wheels: HashMap::new(),
            player: Stats::default(),
            ai: Stats::default(),
            heights: HashMap::new(),
            radius: omsi_cfg::env::var("OMSI_GROUND_GAP_RADIUS").ok().and_then(|v| v.parse().ok()).unwrap_or(150.0),
        };
        if let Some(f) = g.file.as_mut() {
            let _ = writeln!(f, "t,who,wheel,x,y,tyre_z,drawn_z,wheel_ground_z,gap,body_z");
        }
        Some(g)
    }

    fn wheels_of(&mut self, ty: &Arc<omsi_sim::VehicleType>) -> Arc<Wheels> {
        let key = Arc::as_ptr(ty) as usize;
        if let Some(w) = self.wheels.get(&key) {
            return w.clone();
        }
        let mut by_var: HashMap<String, (usize, Vec3, f32)> = HashMap::new();
        for (i, vm) in ty.meshes.iter().enumerate() {
            let def = &ty.model.meshes[vm.def_index];
            let Some(an) = def.animations.iter().find(|a| a.variable.to_ascii_lowercase().starts_with("wheel_rotation_")) else { continue };
            let Some(data) = ty.mesh_data(i) else { continue };
            if data.positions.is_empty() {
                continue;
            }
            let (lo, hi) = data.positions.iter().fold((Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)), |(lo, hi), p| (lo.min(*p), hi.max(*p)));
            let half = (hi.z - lo.z) * 0.5;
            let e = by_var.entry(an.variable.to_ascii_lowercase()).or_insert((i, (lo + hi) * 0.5, half));
            if half > e.2 {
                *e = (i, (lo + hi) * 0.5, half);
            }
        }
        let w = Arc::new(by_var.into_values().collect::<Wheels>());
        self.wheels.insert(key, w.clone());
        w
    }

    #[allow(clippy::too_many_arguments)]
    fn body(&mut self, world: &World, t: f32, key: u64, ai: bool, ty: &Arc<omsi_sim::VehicleType>, position: DVec3, xf: &dyn Fn(usize) -> Mat4) {
        let wheels = self.wheels_of(ty);
        if wheels.is_empty() {
            return;
        }
        for (k, &(i, c, r)) in wheels.iter().enumerate() {
            let centre = position + xf(i).transform_point3(c).as_dvec3();
            let tyre = centre.z - r as f64;
            // what is drawn under the tyre, from 2 m over its hub: a tyre sunk into the road
            // is under the road, and asked from the hub that found the ground below it
            let drawn = crate::scene::drawn_ground(&world.terrains, &world.surfaces, centre.x, centre.y, centre.z + 2.0);
            let wheel = crate::scene::drive_probe(&world.terrains, &world.surfaces, centre.x, centre.y, centre.z).below;
            let stats = if ai { &mut self.ai } else { &mut self.player };
            match drawn {
                Some(d) => stats.add(tyre - d, DVec3::new(centre.x, centre.y, tyre), t),
                None => stats.no_ground += 1,
            }
            if let Some(f) = self.file.as_mut() {
                let o = |v: Option<f64>| v.map(|z| format!("{z:.4}")).unwrap_or_default();
                let _ = writeln!(f, "{t:.3},{key},{k},{:.2},{:.2},{tyre:.4},{},{},{},{:.4}", centre.x, centre.y, o(drawn), o(wheel), o(drawn.map(|d| tyre - d)), position.z);
            }
        }
        // the body's height: a step in its vertical speed of more than 0.6 m/s in one frame
        // (a jump of 2 cm at 30 fps against the frame before) is a jolt
        let e = self.heights.entry(key).or_insert((position.z, position.z, 0));
        let stats = if ai { &mut self.ai } else { &mut self.player };
        if e.2 >= 2 {
            let acc = (position.z - 2.0 * e.1 + e.0).abs();
            stats.max_jump = stats.max_jump.max(acc);
            if acc > 0.02 {
                stats.jumps += 1;
            }
        }
        if e.2 == 0 {
            stats.bodies += 1;
        }
        *e = (e.1, position.z, e.2 + 1);
    }

    pub fn frame(&mut self, world: &World, t: f32, player: Option<&omsi_sim::VehicleInstance>, traffic: Option<&crate::traffic::Traffic>) {
        let centre = player.map(|v| v.position);
        if let Some(v) = player {
            self.body(world, t, u64::MAX, false, &v.ty, v.position, &|i| v.mesh_local_transform(i));
            for (k, p) in v.trailers.iter().enumerate() {
                self.body(world, t, u64::MAX - 1 - k as u64, false, &p.ty, p.position, &|i| p.mesh_local_transform(i));
            }
        }
        if let Some(tr) = traffic {
            for c in &tr.cars {
                let v = &c.vehicle;
                if centre.is_some_and(|p| (p - v.position).truncate().length() > self.radius) {
                    continue;
                }
                if !matches!(c.body.kind, omsi_sim::ai_motion::MotionKind::Road) {
                    continue;
                }
                let id = c.id * 8;
                self.body(world, t, id, true, &v.ty, v.position, &|i| v.mesh_local_transform(i));
                for (k, p) in v.trailers.iter().enumerate() {
                    self.body(world, t, id + 1 + k as u64, true, &p.ty, p.position, &|i| p.mesh_local_transform(i));
                }
            }
        }
    }

    pub fn report(mut self) {
        for (name, s) in [("player", &mut self.player), ("AI", &mut self.ai)] {
            s.trim();
            if s.n == 0 {
                log::info!("ground gap {name}: no wheels measured");
                continue;
            }
            log::info!(
                "ground gap {name}: {} wheel-frames on {} bodies, mean {:+.3} m, mean |gap| {:.3} m, max over {:+.3} m, max under {:+.3} m, {} over +5 cm ({:.1}%), {} under -5 cm ({:.1}%), {} without ground; body jolts (|d2z| > 2 cm/frame) {}, worst {:.3} m",
                s.n, s.bodies, s.sum / s.n as f64, s.sum_abs / s.n as f64, s.max_over, s.max_under, s.over5, s.over5 as f64 * 100.0 / s.n as f64, s.under5, s.under5 as f64 * 100.0 / s.n as f64, s.no_ground, s.jumps, s.max_jump
            );
            for (g, p, t) in s.worst.iter().take(15) {
                log::info!("  {name} tyre {g:+.3} m at ({:.1}, {:.1}, {:.2}) t={t:.1}", p.x, p.y, p.z);
            }
        }
    }
}

/// `OMSI_GROUND_LANES=1`: along every street lane of the loaded tiles, every metre, how far
/// the drawn ground (from 3 m over the lane, as Omsi.exe's wheels ask it) lies over or under
/// the lane's own height - what an AI car's wheels meet where it follows its lane.
pub fn check_lanes(world: &World, traffic: &crate::traffic::Traffic) {
    if omsi_cfg::env::var_os("OMSI_GROUND_LANES").is_none() {
        return;
    }
    let mut hist = [0usize; 12];
    let edges = [-1.0, -0.3, -0.1, -0.05, -0.02, 0.02, 0.05, 0.1, 0.3, 0.6, 1.0];
    let (mut n, mut none) = (0usize, 0usize);
    let mut places: Vec<(f64, DVec3)> = Vec::new();
    for l in traffic.net.lanes.iter().filter(|l| l.kind == omsi_sim::traffic::LaneKind::Street && !l.invisible) {
        let len = l.length();
        let mut s = 0.5f32;
        while s < len {
            let (p, _) = l.at(s);
            s += 1.0;
            let Some(d) = crate::scene::drawn_ground(&world.terrains, &world.surfaces, p.x, p.y, p.z + 3.0) else {
                none += 1;
                continue;
            };
            n += 1;
            let off = d - p.z;
            let k = edges.iter().position(|e| off < *e).unwrap_or(edges.len());
            hist[k] += 1;
            if !(-0.1..=0.6).contains(&off) && !places.iter().any(|q| (q.1 - p).truncate().length() < 15.0) && places.len() < 400 {
                places.push((off, p));
            }
        }
    }
    log::info!("lane ground: {n} lane points with drawn ground, {none} without; drawn minus lane height by bins <-1, -1..-0.3, -0.3..-0.1, -0.1..-0.05, -0.05..-0.02, -0.02..0.02, 0.02..0.05, 0.05..0.1, 0.1..0.3, 0.3..0.6, 0.6..1, >1: {hist:?}");
    places.sort_by(|a, b| b.0.abs().total_cmp(&a.0.abs()));
    for (off, p) in places.iter().take(25) {
        log::info!("  lane at ({:.1}, {:.1}, {:.2}): drawn ground {off:+.2} m", p.x, p.y, p.z);
    }
}
