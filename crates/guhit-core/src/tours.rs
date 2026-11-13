//! Video tours (DECISIONS D54). LANE L9 owns this module.
//!
//! A tour is a view like a camera (`Element::is_view`): on no level, no
//! layer, always allowed in any scope or agent limit. It stores shots only.
//! Frames are worked out here, once, for every reader: the window's
//! playback, timeline and path preview, the video export, MCP and the
//! review all read `Query::TourFrames` (docs/CONTRACT.md, "Tours").
//!
//! - `make_tour` builds the automatic route of `Command::MakeTour`: a drone
//!   fly-in and orbit, a push-in to the front door, the rooms in door order
//!   with a hold in each, up the stairs along the walk line, and a day to
//!   dusk finish outside.
//!   A walk looks at its path a little ahead, so it turns into a corner or
//!   up a stair's next flight before it gets there (GA P1-9), keeps clear of
//!   walls where the room allows, and passes a sliding door on its open half.
//! - `frames` samples a tour: the camera runs through each shot's keys on a
//!   centripetal Catmull-Rom spline, moved along by arc length, eased on arc
//!   length; transitions are worked into the frames (`Fx`). The answer also
//!   says how the doors stand (`door_moves`): open wide for the camera to go
//!   through, shut where it only passes, in the view only.
//! - `review` raises `tour_through_wall` where the camera path passes through
//!   a wall (outside a door) or a floor (outside a stair opening or void),
//!   sampled at 5 frames a second because it runs on every staged step.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use guhit_model::*;
use serde_json::{json, Value};

use crate::error::CoreError;
use crate::exec::{outcome, Outcome};
use crate::geom::*;
use crate::ids::IdGen;
use crate::issues::issue;
use crate::rooms::{assign_by_seed, FaceRef};
use crate::topo::{analyze, Analysis};
use crate::validate::{finite, in_range, require_level};

/// Frame rates a tour can have.
pub const FPS: [u32; 3] = [24, 30, 60];

/// Eye height above the walking surface, as walk mode (DECISIONS D44).
const EYE_MM: f64 = 1600.0;
/// How far ahead a walking camera looks.
const LOOK_MM: f64 = 3000.0;
/// A door is walked through from this far beyond each wall face.
const DOOR_CLEAR_MM: f64 = 550.0;
/// The part of an `InOut` shot spent speeding up, and again slowing down.
/// Its top speed is 1 / (1 - EASE_RAMP) times its average.
pub const EASE_RAMP: f64 = 0.2;
/// A hold in each room, seconds, and how far it pans each way, degrees.
const HOLD_S: f64 = 3.5;
const PAN_DEG: f64 = 22.0;
/// Lenses, vertical degrees: inside, outside, from the drone.
const FOV_IN: f64 = 62.0;
const FOV_OUT: f64 = 50.0;
const FOV_DRONE: f64 = 45.0;
/// Drone speeds, mm per second: flying in, orbiting, pushing in.
const FLY_MMPS: f64 = 9000.0;
const ORBIT_MMPS: f64 = 5000.0;
const PUSH_MMPS: f64 = 3000.0;
/// Samples per spline segment for the arc length tables.
const ARC_SAMPLES: usize = 128;
/// The review samples tours at this rate.
const REVIEW_FPS: f64 = 5.0;
/// Shots meeting closer than this run on without a jump.
const JOIN_MM: f64 = 100.0;
/// A door is set for the camera before it comes this much nearer than the
/// door's width (open wide to pass through it, shut to pass by it), after a
/// swing this long, seconds.
const DOOR_ZONE_MM: f64 = 600.0;
const DOOR_SWING_S: f64 = 0.7;
/// A leaf opened at least this far, degrees, lies back out of the way and
/// stays open; one held short of it shuts once the camera is this far past.
const PROPPED_DEG: f64 = 150.0;
const CLOSE_BEHIND_MM: f64 = 700.0;
/// Walks keep this far from a room's walls where they can: each mm nearer
/// costs as much as this many mm more walking.
const WALL_CLEAR_MM: f64 = 600.0;
const WALL_SHY: f64 = 3.0;
/// A walking camera looks at its path this far ahead, tilted by this much of
/// the path's rise; on a stair further ahead and up the flight, so it turns
/// early and smoothly at a landing and looks toward the floor above. Its
/// keys are this far apart, and it eases out of the last look and into the
/// next over this much of the path.
const WALK_LEAD_MM: f64 = 1500.0;
const WALK_TILT: f64 = 0.6;
const STAIR_LEAD_MM: f64 = 1800.0;
const STAIR_TILT: f64 = 0.7;
const WALK_STEP_MM: f64 = 250.0;
const WALK_BLEND_MM: f64 = 900.0;

/// Give every shot with an empty id a new one; `renew` gives every shot a
/// new one (a copied tour).
pub fn assign_ids(t: &mut Tour, ids: &mut IdGen, renew: bool) {
    for s in t.shots.iter_mut() {
        if renew || s.id.is_empty() {
            s.id = ids.next_id();
        } else {
            ids.reserve(&s.id);
        }
    }
}

/// The rules of one tour (validate hub, `Element::Tour`).
pub fn validate(project: &Project, t: &Tour) -> Result<(), CoreError> {
    let ids = [t.id.clone()];
    if t.name.trim().is_empty() {
        return Err(CoreError::invalid_for("bad_name", "A tour needs a name.", ids.to_vec()));
    }
    if !FPS.contains(&t.fps) {
        return Err(CoreError::invalid_for(
            "tour_fps",
            format!("A tour runs at 24, 30 or 60 frames per second, not {}.", t.fps),
            ids.to_vec(),
        ));
    }
    let others: BTreeSet<&Id> = project
        .elements
        .iter()
        .filter_map(|e| match e {
            Element::Tour(o) if o.id != t.id => Some(o.shots.iter().map(|s| &s.id)),
            _ => None,
        })
        .flatten()
        .collect();
    let mut seen = BTreeSet::new();
    for s in &t.shots {
        if s.id.is_empty() || !seen.insert(&s.id) || others.contains(&s.id) {
            return Err(CoreError::invalid_for("duplicate_id", "Every shot needs its own id.", ids.to_vec()));
        }
        in_range("tour_shot", "Shot length", s.seconds, 0.1, 600.0, "s", &ids)?;
        in_range("tour_shot", "Transition", s.transition.seconds, 0.0, 10.0, "s", &ids)?;
        if s.keys.is_empty() {
            return Err(CoreError::invalid_for("tour_shot", "A shot needs at least one camera key.", ids.to_vec()));
        }
        for k in &s.keys {
            in_range("tour_shot", "Key time", k.at, 0.0, 1.0, "", &ids)?;
            in_range("camera_fov", "Field of view", k.fov_deg, 1.0, 179.0, "degrees", &ids)?;
            for v in [k.position, k.target] {
                finite("Camera key x", v.x, &ids)?;
                finite("Camera key y", v.y, &ids)?;
                finite("Camera key z", v.z, &ids)?;
            }
        }
    }
    Ok(())
}

/// A tour for `Query::Describe` and `ListElements`.
pub fn describe(t: &Tour) -> Value {
    let seconds: f64 = t.shots.iter().map(|s| s.seconds).sum();
    json!({
        "id": t.id,
        "kind": "tour",
        "name": t.name,
        "fps": t.fps,
        "shot_count": t.shots.len(),
        "seconds": (seconds * 10.0).round() / 10.0,
        "shots": t.shots.iter().map(|s| json!({ "id": s.id, "name": s.name, "move": s.kind, "seconds": s.seconds, "transition": s.transition.kind })).collect::<Vec<_>>(),
    })
}

// ------------------------------------------------------------- vectors

fn v3(x: f64, y: f64, z: f64) -> Vec3 {
    Vec3 { x, y, z }
}

fn vadd(a: Vec3, b: Vec3) -> Vec3 {
    v3(a.x + b.x, a.y + b.y, a.z + b.z)
}

fn vsub(a: Vec3, b: Vec3) -> Vec3 {
    v3(a.x - b.x, a.y - b.y, a.z - b.z)
}

fn vscale(a: Vec3, k: f64) -> Vec3 {
    v3(a.x * k, a.y * k, a.z * k)
}

fn vlen(a: Vec3) -> f64 {
    (a.x * a.x + a.y * a.y + a.z * a.z).sqrt()
}

fn vdist(a: Vec3, b: Vec3) -> f64 {
    vlen(vsub(a, b))
}

fn vlerp(a: Vec3, b: Vec3, t: f64) -> Vec3 {
    v3(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t, a.z + (b.z - a.z) * t)
}

fn flat(a: Vec3) -> Point {
    pt(a.x, a.y)
}

fn at_z(p: Point, z: f64) -> Vec3 {
    v3(p.x, p.y, z)
}

fn dir_deg(deg: f64) -> Point {
    let a = deg.to_radians();
    pt(a.cos(), a.sin())
}

fn angle_of(d: Point) -> f64 {
    d.y.atan2(d.x).to_degrees()
}

/// `target` turned about `position` by `deg` in plan (counter-clockwise).
fn yawed(position: Vec3, target: Vec3, deg: f64) -> Vec3 {
    let p = rotate_about(flat(target), flat(position), deg);
    v3(p.x, p.y, target.z)
}

fn unit3(a: Vec3) -> Option<Vec3> {
    let l = vlen(a);
    (l > 1e-6).then(|| vscale(a, 1.0 / l))
}

// ------------------------------------------------------------ the path

/// One segment of a centripetal Catmull-Rom spline (alpha 0.5), from p1 to
/// p2 at t 0 to 1. Centripetal knots never loop or overshoot at tight keys.
fn catmull_rom(p0: Vec3, p1: Vec3, p2: Vec3, p3: Vec3, t: f64) -> Vec3 {
    if vdist(p1, p2) < 1e-9 {
        return p1;
    }
    let knot = |a: Vec3, b: Vec3| vdist(a, b).sqrt().max(1e-3);
    let t0 = 0.0;
    let t1 = knot(p0, p1);
    let t2 = t1 + knot(p1, p2);
    let t3 = t2 + knot(p2, p3);
    let u = t1 + (t2 - t1) * t;
    let mix = |a: Vec3, b: Vec3, ta: f64, tb: f64| vlerp(a, b, (u - ta) / (tb - ta));
    let a1 = mix(p0, p1, t0, t1);
    let a2 = mix(p1, p2, t1, t2);
    let a3 = mix(p2, p3, t2, t3);
    let b1 = mix(a1, a2, t0, t2);
    let b2 = mix(a2, a3, t1, t3);
    mix(b1, b2, t1, t2)
}

/// A spline through points, with an arc length table per segment.
struct Spline {
    pts: Vec<Vec3>,
    /// Per segment: the length at each of ARC_SAMPLES + 1 evenly spaced
    /// parameters, from 0.
    arcs: Vec<Vec<f64>>,
}

impl Spline {
    fn new(pts: Vec<Vec3>) -> Spline {
        let mut s = Spline { pts, arcs: vec![] };
        let segs = s.pts.len().saturating_sub(1);
        for j in 0..segs {
            let mut table = Vec::with_capacity(ARC_SAMPLES + 1);
            let mut prev = s.point(j, 0.0);
            let mut sum = 0.0;
            table.push(0.0);
            for i in 1..=ARC_SAMPLES {
                let p = s.point(j, i as f64 / ARC_SAMPLES as f64);
                sum += vdist(prev, p);
                table.push(sum);
                prev = p;
            }
            s.arcs.push(table);
        }
        s
    }

    /// Segment j at t; the ends are mirrored, so a two point spline is straight.
    fn point(&self, j: usize, t: f64) -> Vec3 {
        let n = self.pts.len();
        let p1 = self.pts[j];
        let p2 = self.pts[j + 1];
        let p0 = if j == 0 { vsub(vscale(p1, 2.0), p2) } else { self.pts[j - 1] };
        let p3 = if j + 2 < n { self.pts[j + 2] } else { vsub(vscale(p2, 2.0), p1) };
        catmull_rom(p0, p1, p2, p3, t)
    }

    fn seg_len(&self, j: usize) -> f64 {
        *self.arcs[j].last().unwrap_or(&0.0)
    }

    fn len(&self) -> f64 {
        (0..self.arcs.len()).map(|j| self.seg_len(j)).sum()
    }

    /// The parameter of segment j at fraction f of its length.
    fn param_at(&self, j: usize, f: f64) -> f64 {
        let table = &self.arcs[j];
        let total = *table.last().unwrap_or(&0.0);
        let s = f.clamp(0.0, 1.0) * total;
        let i = table.partition_point(|&v| v < s).clamp(1, ARC_SAMPLES);
        let (a, b) = (table[i - 1], table[i]);
        let local = if b - a > 1e-12 { (s - a) / (b - a) } else { 0.0 };
        ((i - 1) as f64 + local) / ARC_SAMPLES as f64
    }
}

/// The point `at` mm along a spline.
fn spline_at(s: &Spline, at: f64) -> Vec3 {
    let mut left = at.max(0.0);
    for j in 0..s.arcs.len() {
        let l = s.seg_len(j);
        if left <= l || j + 1 == s.arcs.len() {
            let f = if l > 1e-9 { (left / l).min(1.0) } else { 1.0 };
            return s.point(j, s.param_at(j, f));
        }
        left -= l;
    }
    s.pts[0]
}

/// `p` through a shot of easing `e` at time fraction `u`. InOut speeds up
/// over the first EASE_RAMP, runs at an even pace, then slows down: speed
/// is continuous and peaks at 1 / (1 - EASE_RAMP) times the average.
pub fn ease(e: Ease, u: f64) -> f64 {
    let u = u.clamp(0.0, 1.0);
    match e {
        Ease::Linear => u,
        Ease::InOut => {
            let a = EASE_RAMP;
            let k = 2.0 * a * (1.0 - a);
            if u < a {
                u * u / k
            } else if u > 1.0 - a {
                1.0 - (1.0 - u) * (1.0 - u) / k
            } else {
                (u - a / 2.0) / (1.0 - a)
            }
        }
    }
}

/// A smooth 0 to 1 for blends.
fn smooth(w: f64) -> f64 {
    let w = w.clamp(0.0, 1.0);
    w * w * (3.0 - 2.0 * w)
}

/// One shot's camera: position and target splines through its keys in
/// `at` order, their `at` knots and lenses.
struct Track {
    at: Vec<f64>,
    pos: Spline,
    tgt: Spline,
    fov: Vec<f64>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Pose {
    pub position: Vec3,
    pub target: Vec3,
    pub fov: f64,
}

impl Track {
    fn new(shot: &Shot) -> Track {
        let mut keys = shot.keys.clone();
        keys.sort_by(|a, b| a.at.total_cmp(&b.at));
        Track {
            at: keys.iter().map(|k| k.at).collect(),
            pos: Spline::new(keys.iter().map(|k| k.position).collect()),
            tgt: Spline::new(keys.iter().map(|k| k.target).collect()),
            fov: keys.iter().map(|k| k.fov_deg).collect(),
        }
    }

    fn key(&self, i: usize) -> Pose {
        Pose { position: self.pos.pts[i], target: self.tgt.pts[i], fov: self.fov[i] }
    }

    /// The camera at progress p (0 to 1, already eased). Between two keys it
    /// moves at an even pace along the position path, or along the target
    /// path while the camera stands still (a pan), or evenly in time when
    /// neither moves (a zoom).
    fn pose(&self, p: f64) -> Pose {
        let n = self.at.len();
        if n == 1 || p <= self.at[0] {
            return self.key(0);
        }
        if p >= self.at[n - 1] {
            return self.key(n - 1);
        }
        let j = (self.at.partition_point(|&a| a <= p).max(1) - 1).min(n - 2);
        let span = self.at[j + 1] - self.at[j];
        let f = if span > 1e-9 { ((p - self.at[j]) / span).clamp(0.0, 1.0) } else { 1.0 };
        let t = if self.pos.seg_len(j) > 1.0 {
            self.pos.param_at(j, f)
        } else if self.tgt.seg_len(j) > 1.0 {
            self.tgt.param_at(j, f)
        } else {
            f
        };
        Pose {
            position: self.pos.point(j, t),
            target: self.tgt.point(j, t),
            fov: self.fov[j] + (self.fov[j + 1] - self.fov[j]) * t,
        }
    }
}

/// Key times by arc length along `positions`, so a shot made of them moves at
/// an even pace. Evenly in index when they do not move.
fn arc_times(positions: &[Vec3]) -> Vec<f64> {
    let n = positions.len();
    if n < 2 {
        return vec![0.0; n];
    }
    let s = Spline::new(positions.to_vec());
    let total = s.len();
    let mut out = vec![0.0];
    let mut sum = 0.0;
    for j in 0..n - 1 {
        sum += s.seg_len(j);
        out.push(if total > 1.0 { sum / total } else { (j + 1) as f64 / (n - 1) as f64 });
    }
    *out.last_mut().expect("keys") = 1.0;
    out
}

// ------------------------------------------------------------- frames

/// How a frame is shown when a transition is under way. The window draws it
/// (tour/player.ts, tour/export.ts):
/// - dissolve: the held frame `from_frame` (black for a first shot) blends
///   into this one by `mix`;
/// - dip_black, dip_white: the held frame fades to the color over the first
///   half of `mix`, the color into this frame over the second;
/// - whip_pan: the camera is already whipped round in the frame, `blur` is
///   the sideways smear, 0 to 1;
/// - plan_lift: the camera is already lifting from straight above, the 2D
///   plan lies over the picture at `1 - mix` and fades out early.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Fx {
    pub kind: TransitionKind,
    pub mix: f64,
    pub blur: f64,
    pub from_frame: Option<usize>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Frame {
    pub t: f64,
    pub shot: usize,
    pub pose: Pose,
    pub light: Option<ViewLight>,
    pub fx: Option<Fx>,
}

/// Start time of each shot and the tour's length, seconds. A shot with no
/// camera key (a damaged file; the rules refuse one) plays for no time.
fn starts(t: &Tour) -> (Vec<f64>, f64) {
    let mut out = Vec::with_capacity(t.shots.len());
    let mut sum = 0.0;
    for s in &t.shots {
        out.push(sum);
        if !s.keys.is_empty() {
            sum += s.seconds.max(0.0);
        }
    }
    (out, sum)
}

/// A shot's light at time fraction u: day to dusk moves the clock (and the
/// exposure when both are locked); the sky and lamps switch halfway.
fn light_at(s: &Shot, u: f64) -> Option<ViewLight> {
    let a = s.light?;
    let Some(b) = s.light_end else { return Some(a) };
    let u = u.clamp(0.0, 1.0);
    let minutes = a.minutes as f64 + (b.minutes as f64 - a.minutes as f64) * u;
    let half = if u < 0.5 { a } else { b };
    Some(ViewLight {
        month: a.month,
        day: a.day,
        minutes: minutes.round().clamp(0.0, 1439.0) as u16,
        sky: half.sky,
        exposure_ev: match (a.exposure_ev, b.exposure_ev) {
            (Some(x), Some(y)) => Some(x + (y - x) * u),
            _ => half.exposure_ev,
        },
        lamps: half.lamps,
    })
}

/// The camera a plan lift starts from: straight above the building, its
/// whole plan in view, north up the picture.
fn plan_camera(project: &Project) -> Pose {
    let mut lo = pt(f64::INFINITY, f64::INFINITY);
    let mut hi = pt(f64::NEG_INFINITY, f64::NEG_INFINITY);
    let mut top: f64 = 3000.0;
    for el in &project.elements {
        if let Element::Wall(w) = el {
            for p in [w.start, w.end] {
                lo = pt(lo.x.min(p.x), lo.y.min(p.y));
                hi = pt(hi.x.max(p.x), hi.y.max(p.y));
            }
            if let Some(l) = project.levels.iter().find(|l| l.id == w.level_id) {
                top = top.max(l.elevation_mm + w.height_mm.unwrap_or(l.height_mm));
            }
        }
    }
    if !lo.x.is_finite() {
        lo = pt(-5000.0, -5000.0);
        hi = pt(5000.0, 5000.0);
    }
    let c = lerp(lo, hi, 0.5);
    let fov: f64 = 40.0;
    let half = ((hi.y - lo.y).max((hi.x - lo.x) / 1.78) / 2.0 * 1.25).max(4000.0);
    let h = top + half / (fov / 2.0).to_radians().tan();
    // A degree of tilt to the north keeps north up the picture.
    let lean = (h - top) * 1f64.to_radians().tan();
    Pose { position: v3(c.x, c.y - lean, h), target: v3(c.x, c.y, top), fov }
}

/// Every frame of a tour at `fps`, from time 0, one every 1 / fps seconds.
pub(crate) fn sample(project: &Project, t: &Tour, fps: f64) -> Vec<Frame> {
    let (start, total) = starts(t);
    if t.shots.is_empty() || total <= 0.0 || !(fps > 0.0) {
        return vec![];
    }
    let tracks: Vec<Track> = t.shots.iter().map(Track::new).collect();
    let count = ((total * fps).round() as usize).max(1);
    let first_frame: Vec<usize> = start.iter().map(|s| (s * fps - 1e-6).ceil().max(0.0) as usize).collect();
    let mut plan: Option<Pose> = None;
    let mut out = Vec::with_capacity(count);
    let mut j = 0;
    for i in 0..count {
        let time = i as f64 / fps;
        while j + 1 < t.shots.len() && (time >= start[j + 1] - 1e-9 || tracks[j].at.is_empty()) {
            j += 1;
        }
        // Key-less shots take no time, so this holds unless every shot is
        // key-less, and then `total` is 0.
        if tracks[j].at.is_empty() {
            break;
        }
        let shot = &t.shots[j];
        let local = time - start[j];
        let u = if shot.seconds > 0.0 { local / shot.seconds } else { 1.0 };
        let mut pose = tracks[j].pose(ease(shot.ease, u));
        let mut fx = None;
        let d = shot.transition.seconds.min(shot.seconds);
        if shot.transition.kind != TransitionKind::Cut && d > 0.0 && local < d {
            let w = (local / d).clamp(0.0, 1.0);
            let from_frame = if j > 0 && first_frame[j] > 0 { Some(first_frame[j] - 1) } else { None };
            let mut f = Fx { kind: shot.transition.kind, mix: smooth(w), blur: 0.0, from_frame };
            match shot.transition.kind {
                TransitionKind::WhipPan => {
                    // Out of the last shot's end turning away, in to this one
                    // turning back; the swing is smeared at its fastest.
                    const SWING: f64 = 80.0;
                    let prev = (0..j).rev().find(|&k| !tracks[k].at.is_empty());
                    if let (true, Some(prev)) = (w < 0.5, prev) {
                        let k = w / 0.5;
                        let end = tracks[prev].pose(1.0);
                        pose = Pose { target: yawed(end.position, end.target, -SWING * k * k), ..end };
                    } else {
                        let k = ((w - 0.5) / 0.5).max(0.0);
                        let back = 1.0 - (1.0 - k) * (1.0 - k);
                        pose.target = yawed(pose.position, pose.target, SWING * (1.0 - back));
                    }
                    f.mix = w;
                    f.blur = (std::f64::consts::PI * w).sin();
                }
                TransitionKind::PlanLift => {
                    let from = *plan.get_or_insert_with(|| plan_camera(project));
                    let k = smooth(w);
                    pose = Pose {
                        position: vlerp(from.position, pose.position, k),
                        target: vlerp(from.target, pose.target, k),
                        fov: from.fov + (pose.fov - from.fov) * k,
                    };
                    f.mix = w;
                }
                _ => {}
            }
            fx = Some(f);
        }
        out.push(Frame { t: time, shot: j, pose, light: light_at(shot, u), fx });
    }
    out
}

/// A door the tour moves in the view only (a tour is a view: it never edits
/// the model): from where it was to `open` (0 closed, 1 fully open) between
/// `from` and `to` seconds, smoothly (`smooth`), then held. Before its first
/// move a leaf rests as the model draws it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct DoorMove {
    pub id: Id,
    pub from: f64,
    pub to: f64,
    pub open: f64,
}

/// The 3D view's door frame and the opening a swing leaf rests at, a share
/// of 90 degrees (scene/openings.ts `FRAME`, `AJAR`): the window keeps its
/// own rest, the tests measure with this one.
const DOOR_FRAME_MM: f64 = 45.0;
#[cfg(test)]
const LEAF_REST: f64 = 32.0 / 90.0;
/// The widest a tour opens a swing leaf: back toward the wall beside the
/// doorway, as a photographer props it, short of lying on the wall face.
const LEAF_WIDE_DEG: f64 = 165.0;

/// A door's swing leaves in plan at `open` (0 closed, 1 at 90 degrees, more
/// on round toward the wall), exactly as scene/openings.ts builds them:
/// hinged a frame in from a jamb, 20 mm in from the face on the swing side.
/// None for a sliding door (its panel slides in the wall's line) or a
/// cased opening.
pub(crate) fn leaf_lines(o: &Opening, w: &Wall, open: f64) -> Vec<(Point, Point)> {
    if o.opening_type != OpeningType::Door || matches!(o.style, OpeningStyle::Fixed | OpeningStyle::Sliding) {
        return vec![];
    }
    let Some(d) = unit(sub(w.end, w.start)) else { return vec![] };
    let n = perp_left(d);
    let swing = if o.flip_side { scale(n, -1.0) } else { n };
    let clear = o.width_mm - 2.0 * DOOR_FRAME_MM;
    let theta = open * std::f64::consts::FRAC_PI_2;
    let leaf = |at_start: bool, lw: f64| {
        let u = if at_start { o.offset_mm - o.width_mm / 2.0 + DOOR_FRAME_MM } else { o.offset_mm + o.width_mm / 2.0 - DOOR_FRAME_MM };
        let hinge = add(add(w.start, scale(d, u)), scale(swing, w.thickness_mm.max(20.0) / 2.0 - 20.0));
        let closed = if at_start { d } else { scale(d, -1.0) };
        let along = add(scale(closed, theta.cos()), scale(swing, theta.sin()));
        (hinge, add(hinge, scale(along, lw)))
    };
    // A casement or jalousie door is drawn as a single swing leaf.
    if o.style == OpeningStyle::SwingDouble {
        vec![leaf(true, clear / 2.0 - 4.0), leaf(false, clear / 2.0 - 4.0)]
    } else {
        vec![leaf(!o.flip_hinge, clear - 6.0)]
    }
}

/// How far a tour opens a door: a swing leaf as far round as LEAF_WIDE_DEG
/// while it meets no other wall and no object standing on its level, never
/// less than 90 degrees; a sliding panel all the way.
fn open_wide(o: &Opening, w: &Wall, level: &Level, derived: &Derived, project: &Project) -> f64 {
    if o.style == OpeningStyle::Sliding {
        return 1.0;
    }
    let (z0, z1) = (level.elevation_mm + o.sill_mm, level.elevation_mm + o.sill_mm + o.height_mm);
    let mut blocks: Vec<Vec<Point>> = vec![];
    for g in &derived.walls {
        if g.wall_id == w.id || g.outline.len() < 3 {
            continue;
        }
        let same_level = project.elements.iter().any(|e| matches!(e, Element::Wall(x) if x.id == g.wall_id && x.level_id == w.level_id));
        if same_level {
            blocks.push(g.outline.clone());
        }
    }
    for el in &project.elements {
        if let Element::Asset(a) = el {
            let base = project.levels.iter().find(|l| l.id == a.level_id).map(|l| l.elevation_mm).unwrap_or(f64::NAN);
            let (lo, hi) = (base + a.elevation_mm, base + a.elevation_mm + a.height_mm);
            if a.level_id == w.level_id && hi > z0 + 50.0 && lo < z1 {
                let (s, c) = a.rotation_deg.to_radians().sin_cos();
                let (hw, hd) = (a.width_mm / 2.0, a.depth_mm / 2.0);
                blocks.push([(-hw, -hd), (hw, -hd), (hw, hd), (-hw, hd)].iter().map(|(x, y)| pt(a.position.x + x * c - y * s, a.position.y + x * s + y * c)).collect());
            }
        }
    }
    let meets = |(a, b): (Point, Point)| {
        // The leaf with its thickness and a little room round it.
        let Some(dir) = unit(sub(b, a)) else { return false };
        let side = scale(perp_left(dir), 30.0);
        let tip = add(b, scale(dir, 40.0));
        let lines = [(a, tip), (add(a, side), add(tip, side)), (sub(a, side), sub(tip, side))];
        blocks.iter().any(|poly| {
            point_in_polygon(tip, poly)
                || lines.iter().any(|&(p, q)| (0..poly.len()).any(|i| segments_touch(p, q, poly[i], poly[(i + 1) % poly.len()])))
        })
    };
    let mut deg = LEAF_WIDE_DEG;
    while deg > 90.0 {
        if !leaf_lines(o, w, deg / 90.0).into_iter().any(meets) {
            return deg / 90.0;
        }
        deg -= 5.0;
    }
    1.0
}

/// The doors the camera comes near (GA P1-9: a leaf at its ajar rest stood
/// in the doorway and filled the frame). Each stretch of the path near a
/// door (within its width and DOOR_ZONE_MM of its middle, on its level)
/// wants it open wide (`open_wide`) when the camera goes through it, and
/// shut, flush in the wall, when the camera only passes by. A door first
/// wanted shut is shut from the start, so it never moves on screen; a door
/// swings only when what it is wanted for changes, done as the camera comes
/// near. A jump between shots ends a stretch.
pub(crate) fn door_moves(project: &Project, derived: &Derived, list: &[Frame]) -> Vec<DoorMove> {
    let levels: BTreeMap<&str, &Level> = project.levels.iter().map(|l| (l.id.as_str(), l)).collect();
    let walls: BTreeMap<&str, &Wall> = project
        .elements
        .iter()
        .filter_map(|e| match e {
            Element::Wall(w) => Some((w.id.as_str(), w)),
            _ => None,
        })
        .collect();
    let jump = |i: usize| vdist(list[i].pose.position, list[i + 1].pose.position) > JOIN_MM * 10.0;
    let mut out = vec![];
    for el in &project.elements {
        let Element::Opening(o) = el else { continue };
        // A cased opening ("fixed") has no leaf to move.
        if o.opening_type != OpeningType::Door || o.style == OpeningStyle::Fixed {
            continue;
        }
        let Some(w) = walls.get(o.wall_id.as_str()) else { continue };
        let Some(level) = levels.get(w.level_id.as_str()) else { continue };
        let Some(d) = unit(sub(w.end, w.start)) else { continue };
        let n = perp_left(d);
        let center = add(w.start, scale(d, o.offset_mm));
        let eye = level.elevation_mm + EYE_MM;
        let near = |p: Vec3| dist(flat(p), center) <= o.width_mm + DOOR_ZONE_MM && (p.z - eye).abs() < 1500.0;
        let side = |p: Vec3| dot(sub(flat(p), w.start), n);
        let crosses = |i: usize| {
            let (a, b) = (list[i].pose.position, list[i + 1].pose.position);
            let (sa, sb) = (side(a), side(b));
            if sa * sb > 0.0 || (sa - sb).abs() < 1e-9 {
                return false;
            }
            let p = vlerp(a, b, sa / (sa - sb));
            let along = dot(sub(flat(p), w.start), d);
            let h = p.z - level.elevation_mm;
            (along - o.offset_mm).abs() <= o.width_mm / 2.0 && h >= o.sill_mm - 100.0 && h <= o.sill_mm + o.height_mm
        };
        // The stretches near the door: their first frame, and the frames
        // whose move goes through it.
        let mut stretches: Vec<(usize, Vec<usize>)> = vec![];
        let mut i = 0;
        while i < list.len() {
            if !near(list[i].pose.position) {
                i += 1;
                continue;
            }
            let first = i;
            let mut through = vec![];
            while i + 1 < list.len() && !jump(i) && (near(list[i + 1].pose.position) || crosses(i)) {
                if crosses(i) {
                    through.push(i);
                }
                i += 1;
            }
            stretches.push((first, through));
            i += 1;
        }
        let wide = open_wide(o, w, level, derived, project);
        // Back toward the wall it is out of the way; held short of that by a
        // wall or an object it stands out in the room, so it shuts behind
        // the camera once the camera is past it.
        let propped = wide >= PROPPED_DEG / 90.0;
        let mut now: Option<f64> = None;
        let set = |out: &mut Vec<DoorMove>, now: &mut Option<f64>, to: f64, want: f64| {
            if *now != Some(want) {
                out.push(DoorMove { id: o.id.clone(), from: to - DOOR_SWING_S, to, open: want });
                *now = Some(want);
            }
        };
        for (first, through) in &stretches {
            if through.is_empty() {
                // Passed by: shut, from the start when it has not moved yet.
                match now {
                    None => {
                        out.push(DoorMove { id: o.id.clone(), from: -1.0, to: -1.0, open: 0.0 });
                        now = Some(0.0);
                    }
                    Some(v) if v > 0.0 && !propped => set(&mut out, &mut now, list[*first].t, 0.0),
                    _ => {}
                }
                continue;
            }
            for (k, &c) in through.iter().enumerate() {
                // Open, done as the camera comes near: at the stretch's start
                // for its first pass, a second before a later one.
                let to = if k == 0 { list[*first].t } else { list[c].t - 1.0 };
                set(&mut out, &mut now, to, wide);
                if propped {
                    continue;
                }
                // Shut behind the camera once it is CLOSE_BEHIND_MM past, unless
                // it comes back through soon.
                let mut left = CLOSE_BEHIND_MM;
                let mut m = c + 1;
                while left > 0.0 && m + 1 < list.len() && !jump(m) {
                    left -= vdist(list[m].pose.position, list[m + 1].pose.position);
                    m += 1;
                }
                let shut = list[m].t;
                let back = through.get(k + 1).map(|&n| list[n].t);
                if back.is_none_or(|b| b - shut > 2.0 * DOOR_SWING_S + 1.5) {
                    set(&mut out, &mut now, shut + DOOR_SWING_S, 0.0);
                }
            }
        }
    }
    out.sort_by(|a, b| a.from.total_cmp(&b.from).then(a.id.cmp(&b.id)));
    out
}

/// How open a door is at `t` after `moves` (its own, in time order), `rest`
/// before the first. The window does the same (tour/frames.ts `doorsAt`).
#[cfg(test)]
fn door_open_at(moves: &[&DoorMove], rest: f64, t: f64) -> f64 {
    let mut held = rest;
    for m in moves {
        if t >= m.to {
            held = m.open;
            continue;
        }
        if t > m.from {
            return held + (m.open - held) * smooth((t - m.from) / (m.to - m.from).max(1e-6));
        }
        break;
    }
    held
}

fn round(v: f64, k: f64) -> f64 {
    (v * k).round() / k
}

fn vec_json(v: Vec3) -> Value {
    json!({ "x": round(v.x, 10.0), "y": round(v.y, 10.0), "z": round(v.z, 10.0) })
}

/// `Query::TourFrames`: the camera at every frame (centripetal Catmull-Rom
/// by arc length, eased on arc length), and the doors it opens on its way
/// (`door_moves`). `fps` None: the tour's own.
pub fn frames(project: &Project, derived: &Derived, tour_id: &Id, fps: Option<u32>) -> Result<Value, CoreError> {
    let Some(Element::Tour(t)) = project.elements.iter().find(|e| e.id() == tour_id) else {
        return Err(CoreError::NotFound(tour_id.clone()));
    };
    let fps = fps.unwrap_or(t.fps);
    if !(1..=120).contains(&fps) {
        return Err(CoreError::invalid("tour_fps", format!("Ask for 1 to 120 frames per second, not {fps}.")));
    }
    let list = sample(project, t, fps as f64);
    let doors: Vec<Value> = door_moves(project, derived, &list)
        .iter()
        .map(|m| json!({ "id": m.id, "from": round(m.from, 1000.0), "to": round(m.to, 1000.0), "open": round(m.open, 1000.0) }))
        .collect();
    let (start, total) = starts(t);
    let shots: Vec<Value> = t
        .shots
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let first = list.iter().position(|f| f.shot == i);
            let count = list.iter().filter(|f| f.shot == i).count();
            json!({
                "id": s.id, "name": s.name, "move": s.kind,
                "start": round(start[i], 1000.0), "seconds": s.seconds,
                "transition": s.transition,
                "first_frame": first, "frame_count": count,
            })
        })
        .collect();
    let frames: Vec<Value> = list
        .iter()
        .map(|f| {
            let mut v = json!({
                "t": round(f.t, 1000.0),
                "shot": f.shot,
                "position": vec_json(f.pose.position),
                "target": vec_json(f.pose.target),
                "fov_deg": round(f.pose.fov, 100.0),
            });
            if let Some(l) = f.light {
                v["light"] = json!(l);
            }
            if let Some(x) = f.fx {
                let mut fx = json!({ "kind": x.kind, "mix": round(x.mix, 10000.0) });
                if x.blur > 0.0 {
                    fx["blur"] = json!(round(x.blur, 10000.0));
                }
                if let Some(from) = x.from_frame {
                    fx["from_frame"] = json!(from);
                }
                v["fx"] = fx;
            }
            v
        })
        .collect();
    Ok(json!({
        "tour_id": t.id,
        "name": t.name,
        "fps": fps,
        "seconds": round(total, 1000.0),
        "frame_count": frames.len(),
        "shots": shots,
        "frames": frames,
        "doors": doors,
    }))
}

// --------------------------------------------------------------- solids

/// A wall as the camera meets it: its outline, its height range and the
/// doorways it can be walked through.
struct WallSolid<'a> {
    wall: &'a Wall,
    outline: Vec<Point>,
    floor: f64,
    z0: f64,
    z1: f64,
    /// Along the centerline from the start: (center, half width, sill, head)
    /// above the level floor. Doors only: a window is glass.
    doors: Vec<(f64, f64, f64, f64)>,
}

/// An upper floor: where it is solid, and its openings (stairs, voids).
struct FloorSolid<'a> {
    level: &'a Level,
    areas: Vec<Vec<Point>>,
    holes: Vec<Vec<Point>>,
}

pub(crate) struct Solids<'a> {
    walls: Vec<WallSolid<'a>>,
    floors: Vec<FloorSolid<'a>>,
}

/// What the camera hit.
#[derive(Clone, Debug)]
pub(crate) enum Hit {
    Wall(Id),
    Floor(Id),
}

fn grow(poly: &[Point], by: f64) -> Vec<Point> {
    if poly.is_empty() {
        return vec![];
    }
    let n = poly.len() as f64;
    let c = poly.iter().fold(pt(0.0, 0.0), |a, p| add(a, scale(*p, 1.0 / n)));
    poly.iter()
        .map(|p| match unit(sub(*p, c)) {
            Some(d) => add(*p, scale(d, by)),
            None => *p,
        })
        .collect()
}

impl<'a> Solids<'a> {
    pub(crate) fn new(project: &'a Project, derived: &Derived) -> Solids<'a> {
        let levels: BTreeMap<&str, &Level> = project.levels.iter().map(|l| (l.id.as_str(), l)).collect();
        let outlines: BTreeMap<&str, &Vec<Point>> =
            derived.walls.iter().map(|w| (w.wall_id.as_str(), &w.outline)).collect();
        let mut doors: BTreeMap<&str, Vec<(f64, f64, f64, f64)>> = BTreeMap::new();
        for el in &project.elements {
            if let Element::Opening(o) = el {
                if o.opening_type == OpeningType::Door {
                    doors.entry(o.wall_id.as_str()).or_default().push((
                        o.offset_mm,
                        o.width_mm / 2.0,
                        o.sill_mm,
                        o.sill_mm + o.height_mm,
                    ));
                }
            }
        }
        let mut walls = vec![];
        for el in &project.elements {
            let Element::Wall(w) = el else { continue };
            let Some(level) = levels.get(w.level_id.as_str()) else { continue };
            let outline = match outlines.get(w.id.as_str()) {
                Some(o) if o.len() >= 3 => (*o).clone(),
                _ => {
                    let Some(d) = unit(sub(w.end, w.start)) else { continue };
                    let n = scale(perp_left(d), w.thickness_mm / 2.0);
                    vec![sub(w.start, n), sub(w.end, n), add(w.end, n), add(w.start, n)]
                }
            };
            walls.push(WallSolid {
                wall: w,
                outline,
                floor: level.elevation_mm,
                z0: level.elevation_mm,
                z1: level.elevation_mm + w.height_mm.unwrap_or(level.height_mm),
                doors: doors.remove(w.id.as_str()).unwrap_or_default(),
            });
        }
        // Floors of every level but the lowest: solid over the level's
        // buildings and floor slabs, open at stairs that arrive there and voids.
        let lowest = project.levels.iter().map(|l| l.elevation_mm).fold(f64::INFINITY, f64::min);
        let mut floors = vec![];
        for level in project.levels.iter().filter(|l| l.elevation_mm > lowest + 1.0) {
            let mut areas: Vec<Vec<Point>> = derived
                .footprints
                .iter()
                .filter(|f| f.level_id == level.id && f.polygon.len() >= 3)
                .map(|f| f.polygon.clone())
                .collect();
            let mut holes = vec![];
            for el in &project.elements {
                match el {
                    Element::Slab(s) if s.level_id == level.id && s.polygon.len() >= 3 => {
                        if s.kind == SlabKind::Void {
                            holes.push(s.polygon.clone());
                        } else {
                            areas.push(s.polygon.clone());
                        }
                    }
                    Element::Stair(s) => {
                        let arrives = derived
                            .stairs
                            .iter()
                            .find(|g| g.stair_id == s.id)
                            .and_then(|g| g.arrives_at.clone())
                            .or_else(|| crate::slabs::stair_top_level(project, s).map(|l| l.id.clone()));
                        if arrives.as_deref() == Some(level.id.as_str()) {
                            holes.push(grow(&crate::slabs::stair_rect(s), 150.0));
                            if let Some(g) = derived.stairs.iter().find(|g| g.stair_id == s.id) {
                                if g.opening.len() >= 3 {
                                    holes.push(grow(&g.opening, 150.0));
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
            if !areas.is_empty() {
                floors.push(FloorSolid { level, areas, holes });
            }
        }
        Solids { walls, floors }
    }

    /// The first solid the straight move from a to b passes through, with
    /// where. `from_inside`: also count a start inside a wall.
    pub(crate) fn hit(&self, a: Vec3, b: Vec3, from_inside: bool) -> Option<(Hit, Vec3)> {
        let (pa, pb) = (flat(a), flat(b));
        for s in &self.walls {
            let (zlo, zhi) = (a.z.min(b.z), a.z.max(b.z));
            if zhi < s.z0 || zlo > s.z1 {
                continue;
            }
            let w = s.wall;
            let axis = sub(w.end, w.start);
            let Some(d) = unit(axis) else { continue };
            let passable = |p: Point, z: f64| {
                let along = dot(sub(p, w.start), d);
                let h = z - s.floor;
                s.doors.iter().any(|&(c, hw, lo, hi)| (along - c).abs() <= hw - 20.0 && h >= lo && h <= hi)
            };
            // Through the centerline.
            let r = sub(pb, pa);
            let denom = cross(r, axis);
            if denom.abs() > 1e-9 {
                let q = sub(w.start, pa);
                let s_cam = cross(q, axis) / denom;
                let s_wall = cross(q, r) / denom;
                if (0.0..=1.0).contains(&s_cam) && (-1e-6..=1.0 + 1e-6).contains(&s_wall) {
                    let z = a.z + (b.z - a.z) * s_cam;
                    let p = lerp(pa, pb, s_cam);
                    if z >= s.z0 && z <= s.z1 && !passable(p, z) {
                        return Some((Hit::Wall(w.id.clone()), at_z(p, z)));
                    }
                }
            }
            // Standing in the wall.
            let ends: &[(Point, f64)] = if from_inside { &[(pa, a.z), (pb, b.z)] } else { &[(pb, b.z)] };
            for &(p, z) in ends {
                if z >= s.z0 && z <= s.z1 && point_in_polygon(p, &s.outline) && dist_point_segment(p, w.start, w.end) < w.thickness_mm / 2.0 - 1.0 && !passable(p, z) {
                    return Some((Hit::Wall(w.id.clone()), at_z(p, z)));
                }
            }
        }
        for f in &self.floors {
            let e = f.level.elevation_mm;
            if (a.z - e) * (b.z - e) > 0.0 || (a.z - b.z).abs() < 1e-9 {
                continue;
            }
            let s = (e - a.z) / (b.z - a.z);
            let p = lerp(pa, pb, s);
            let solid = f.areas.iter().any(|poly| point_in_polygon(p, poly));
            let open = f.holes.iter().any(|poly| point_in_polygon(p, poly));
            if solid && !open {
                return Some((Hit::Floor(f.level.id.clone()), at_z(p, e)));
            }
        }
        None
    }
}

/// `tour_through_wall` items. Sampled coarsely (REVIEW_FPS): this runs on
/// every staged step. A move across a shot boundary counts only where the
/// shots join, and never under a whip pan.
pub fn review(project: &Project, derived: &Derived) -> Vec<Issue> {
    let tours: Vec<&Tour> = project
        .elements
        .iter()
        .filter_map(|e| match e {
            Element::Tour(t) => Some(t),
            _ => None,
        })
        .collect();
    if tours.is_empty() {
        return vec![];
    }
    let solids = Solids::new(project, derived);
    let mut out = vec![];
    for t in tours {
        let list = sample(project, t, REVIEW_FPS);
        let mut seen: BTreeSet<String> = BTreeSet::new();
        for pair in list.windows(2) {
            let (a, b) = (&pair[0], &pair[1]);
            let whip = |f: &Frame| f.fx.map(|x| x.kind == TransitionKind::WhipPan).unwrap_or(false);
            if whip(a) || whip(b) {
                continue;
            }
            if a.shot != b.shot && vdist(a.pose.position, b.pose.position) > JOIN_MM * 10.0 {
                continue;
            }
            let Some((hit, at)) = solids.hit(a.pose.position, b.pose.position, a.t == 0.0) else { continue };
            let (key, what) = match &hit {
                Hit::Wall(id) => (id.clone(), "a wall".to_string()),
                Hit::Floor(id) => {
                    let name = project.levels.iter().find(|l| &l.id == id).map(|l| l.name.as_str()).unwrap_or("a level");
                    (id.clone(), format!("the floor of {name}"))
                }
            };
            if !seen.insert(key.clone()) {
                continue;
            }
            let shot = &t.shots[b.shot];
            let mut ids = vec![t.id.clone()];
            if matches!(hit, Hit::Wall(_)) {
                ids.push(key.clone());
            }
            let mut i = issue(
                "tour_through_wall",
                Severity::Warning,
                format!(
                    "Tour \"{}\" passes through {what} in shot \"{}\" at {:.1} s. Move a camera key, or the camera shows the inside of the wall.",
                    t.name, shot.name, b.t
                ),
                ids,
            );
            if matches!(hit, Hit::Floor(_)) {
                i.id = format!("tour_through_wall:{},{}", t.id, key);
            }
            i.location = Some(at);
            out.push(i);
        }
    }
    out
}

// ----------------------------------------------------------- make_tour

/// Where a door leads, on its level: a face of the plan, or outside.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Node {
    Face(usize),
    Outside,
}

struct Door<'a> {
    opening: &'a Opening,
    level: usize,
    center: Point,
    /// Where the camera walks through: the middle, or for a sliding door the
    /// middle of the half its panel opens, clear of the fixed panel.
    pass: Point,
    /// Unit normal to the left of the wall, towards side `left`.
    n: Point,
    half: f64,
    left: Node,
    right: Node,
}

impl Door<'_> {
    fn other(&self, from: Node) -> Node {
        if self.left == from {
            self.right
        } else {
            self.left
        }
    }

    /// Its normal pointing into `side`.
    fn towards(&self, side: Node) -> Point {
        if self.left == side {
            self.n
        } else {
            scale(self.n, -1.0)
        }
    }

    /// The points that walk through it from `from`: before, in, after.
    fn crossing(&self, from: Node) -> [Point; 3] {
        let out = self.towards(from);
        let k = self.half + DOOR_CLEAR_MM;
        [add(self.pass, scale(out, k)), self.pass, sub(self.pass, scale(out, k))]
    }
}

struct Planner<'a> {
    project: &'a Project,
    analysis: Analysis,
    derived: Derived,
    rooms: BTreeMap<FaceRef, &'a Room>,
    doors: Vec<Door<'a>>,
    pace: f64,
    shots: Vec<Shot>,
    /// The camera where the last shot ended.
    last: Option<ShotKey>,
    /// Rooms given a hold.
    visited: BTreeSet<FaceRef>,
    climbed: usize,
}

fn key(at: f64, position: Vec3, target: Vec3, fov: f64) -> ShotKey {
    ShotKey { at, position, target, fov_deg: fov }
}

fn shot(name: &str, kind: ShotMove, keys: Vec<ShotKey>, seconds: f64, ease: Ease) -> Shot {
    Shot {
        id: String::new(),
        name: name.to_string(),
        kind,
        keys,
        // Up to the next tenth: a walk never runs faster than its pace.
        seconds: (seconds * 10.0 - 1e-6).ceil().clamp(1.0, 6000.0) / 10.0,
        ease,
        light: None,
        light_end: None,
        transition: Transition { kind: TransitionKind::Cut, seconds: 0.0 },
    }
}

impl<'a> Planner<'a> {
    fn new(project: &'a Project, pace_mps: f64) -> Planner<'a> {
        let analysis = analyze(project);
        let derived = crate::derive::derived_from(project, &analysis);
        let assigned = assign_by_seed(project, &analysis);
        let mut rooms = BTreeMap::new();
        for el in &project.elements {
            if let Element::Room(r) = el {
                if let Some(f) = assigned.get(&r.id) {
                    rooms.insert(*f, r);
                }
            }
        }
        let walls: BTreeMap<&str, &Wall> = project
            .elements
            .iter()
            .filter_map(|e| match e {
                Element::Wall(w) => Some((w.id.as_str(), w)),
                _ => None,
            })
            .collect();
        let mut doors = vec![];
        for el in &project.elements {
            let Element::Opening(o) = el else { continue };
            if o.opening_type != OpeningType::Door {
                continue;
            }
            let Some(w) = walls.get(o.wall_id.as_str()) else { continue };
            let Some(li) = analysis.levels.iter().position(|l| l.level_id == w.level_id) else { continue };
            let Some(d) = unit(sub(w.end, w.start)) else { continue };
            let center = add(w.start, scale(d, o.offset_mm));
            let n = perp_left(d);
            let half = w.thickness_mm / 2.0;
            let side = |sign: f64| {
                let p = add(center, scale(n, sign * (half + 300.0)));
                match analysis.face_at(&w.level_id, p) {
                    Some(f) => Node::Face(f),
                    None => Node::Outside,
                }
            };
            let (left, right) = (side(1.0), side(-1.0));
            if left == right {
                continue;
            }
            // A sliding panel opens over the fixed one (scene/openings.ts),
            // which stands at the start of the wall unless the hinge is flipped.
            let pass = if o.style == OpeningStyle::Sliding {
                let clear = o.width_mm - 2.0 * DOOR_FRAME_MM;
                let open_half = clear / 4.0 + 15.0;
                add(center, scale(d, if o.flip_hinge { -open_half } else { open_half }))
            } else {
                center
            };
            doors.push(Door { opening: o, level: li, center, pass, n, half, left, right });
        }
        Planner {
            project,
            analysis,
            derived,
            rooms,
            doors,
            pace: pace_mps * 1000.0,
            shots: vec![],
            last: None,
            visited: BTreeSet::new(),
            climbed: 0,
        }
    }

    fn level(&self, li: usize) -> &'a Level {
        let id = &self.analysis.levels[li].level_id;
        self.project.levels.iter().find(|l| &l.id == id).expect("analysis levels are project levels")
    }

    fn eye(&self, li: usize) -> f64 {
        self.level(li).elevation_mm + EYE_MM
    }

    fn face_at(&self, li: usize, p: Point) -> Node {
        match self.analysis.face_at(&self.analysis.levels[li].level_id, p) {
            Some(f) => Node::Face(f),
            None => Node::Outside,
        }
    }

    fn room_name(&self, f: FaceRef) -> String {
        self.rooms.get(&f).map(|r| r.name.clone()).unwrap_or_else(|| "Room".to_string())
    }

    /// True when walking straight from a to b meets a wall.
    fn blocked(&self, solids: &Solids, a: Vec3, b: Vec3) -> bool {
        solids.hit(a, b, false).is_some()
    }

    /// A path in plan from a to b inside one face (or outside): straight, or
    /// round the corners when straight meets a wall or runs close along one.
    /// The shortest path through the points 450 and 800 mm in from the face's
    /// corners and its label point (outside: 600 mm out from each wall's
    /// corners) that meets no wall, each stretch costing more the nearer it
    /// comes to the face's walls (GA P1-9: a wall a hand's width away fills
    /// the frame). Straight when there is none. `out` gets the points after a.
    fn inside(&self, solids: &Solids, li: usize, face: Node, a: Point, b: Point, out: &mut Vec<Point>) {
        let z = self.eye(li);
        let clear = |p: Point, q: Point| !self.blocked(solids, at_z(p, z), at_z(q, z));
        let ring = match face {
            Node::Face(f) => Some(&self.analysis.levels[li].faces[f].net),
            Node::Outside => None,
        };
        let cost = |p: Point, q: Point| {
            let l = dist(p, q);
            let Some(ring) = ring else { return l };
            let steps = (l / 200.0).ceil().max(1.0) as usize;
            let worst = (0..=steps).map(|k| dist_to_boundary(lerp(p, q, k as f64 / steps as f64), ring)).fold(f64::INFINITY, f64::min);
            l + WALL_SHY * (WALL_CLEAR_MM - worst).max(0.0)
        };
        let mut nodes = vec![a, b];
        match face {
            Node::Face(f) => {
                let face = &self.analysis.levels[li].faces[f];
                let ring = &face.net;
                let n = ring.len();
                for i in 0..n {
                    let c = ring[i];
                    let (Some(u), Some(w)) = (unit(sub(ring[(i + n - 1) % n], c)), unit(sub(ring[(i + 1) % n], c))) else { continue };
                    let Some(mid) = unit(add(u, w)) else { continue };
                    for d in [mid, scale(mid, -1.0)] {
                        for k in [450.0, 800.0] {
                            let p = add(c, scale(d, k));
                            if face.contains_net(p) {
                                nodes.push(p);
                            }
                        }
                    }
                }
                nodes.push(face.label);
            }
            Node::Outside => {
                let level = &self.analysis.levels[li].level_id;
                for el in &self.project.elements {
                    let Element::Wall(w) = el else { continue };
                    if &w.level_id != level {
                        continue;
                    }
                    let Some(d) = unit(sub(w.end, w.start)) else { continue };
                    let side = scale(perp_left(d), w.thickness_mm / 2.0 + 600.0);
                    for (end, out_dir) in [(w.start, scale(d, -600.0)), (w.end, scale(d, 600.0))] {
                        for s in [1.0, -1.0] {
                            let p = add(add(end, out_dir), scale(side, s));
                            if self.face_at(li, p) == Node::Outside {
                                nodes.push(p);
                            }
                        }
                    }
                }
            }
        }
        // Dijkstra over the points that see each other.
        let m = nodes.len();
        let mut best = vec![f64::INFINITY; m];
        let mut prev = vec![usize::MAX; m];
        let mut done = vec![false; m];
        best[0] = 0.0;
        loop {
            let Some(i) = (0..m).filter(|&i| !done[i] && best[i].is_finite()).min_by(|&x, &y| best[x].total_cmp(&best[y])) else { break };
            done[i] = true;
            if i == 1 {
                break;
            }
            for j in 0..m {
                if done[j] || j == i {
                    continue;
                }
                if best[i] + dist(nodes[i], nodes[j]) >= best[j] {
                    continue;
                }
                let d = best[i] + cost(nodes[i], nodes[j]);
                if d < best[j] && clear(nodes[i], nodes[j]) {
                    best[j] = d;
                    prev[j] = i;
                }
            }
        }
        if !best[1].is_finite() {
            out.push(b);
            return;
        }
        let mut path = vec![];
        let mut at = 1;
        while at != 0 {
            path.push(nodes[at]);
            at = prev[at];
        }
        path.reverse();
        out.extend(path);
    }

    /// Doors from `from` to `to` on a level by fewest doors, never through
    /// outside: the door indices in walking order.
    fn route(&self, li: usize, from: Node, to: Node) -> Option<Vec<usize>> {
        if from == to {
            return Some(vec![]);
        }
        let mut prev: BTreeMap<Node, (Node, usize)> = BTreeMap::new();
        let mut queue = VecDeque::from([from]);
        let mut seen = BTreeSet::from([from]);
        while let Some(n) = queue.pop_front() {
            if n == to {
                break;
            }
            if n == Node::Outside && n != from {
                continue;
            }
            for (i, d) in self.doors.iter().enumerate() {
                if d.level != li || (d.left != n && d.right != n) {
                    continue;
                }
                let m = d.other(n);
                if seen.insert(m) {
                    prev.insert(m, (n, i));
                    queue.push_back(m);
                }
            }
        }
        if !seen.contains(&to) {
            return None;
        }
        let mut doors = vec![];
        let mut at = to;
        while at != from {
            let (p, i) = prev[&at];
            doors.push(i);
            at = p;
        }
        doors.reverse();
        Some(doors)
    }

    fn push(&mut self, mut s: Shot) {
        self.last = s.keys.iter().max_by(|a, b| a.at.total_cmp(&b.at)).copied();
        if self.shots.is_empty() && s.transition.kind == TransitionKind::Cut {
            s.transition = Transition { kind: TransitionKind::DipBlack, seconds: 1.0 };
        }
        self.shots.push(s);
    }

    /// A walk through `points` (eye height over the walking surface), from
    /// the last camera, ending looking at `end_target`. The camera looks at
    /// the path `lead` mm ahead of it, tilted by `tilt` of the path's rise
    /// (GA P1-9): it turns into a corner or up a stair's next flight before
    /// it gets there, instead of facing the wall in front until it turns. It
    /// eases from the last camera's look over the first WALK_BLEND_MM and
    /// into `end_target` over the last. It never looks through a wall at the
    /// path beyond: the look comes in until it sees where it aims (a camera in
    /// a doorway looks along the doorway, not back through the wall). Keys
    /// every WALK_STEP_MM by arc length.
    #[allow(clippy::too_many_arguments)]
    fn walk(&mut self, solids: &Solids, name: &str, kind: ShotMove, points: Vec<Vec3>, end_target: Option<Vec3>, fov: f64, mps: Option<f64>, lead: f64, tilt: f64) {
        let mut pts: Vec<Vec3> = vec![];
        if let Some(k) = self.last {
            pts.push(k.position);
        }
        for p in points {
            if pts.last().map(|q| vdist(*q, p) > 50.0).unwrap_or(true) {
                pts.push(p);
            }
        }
        if pts.len() < 2 {
            return;
        }
        let path = Spline::new(pts.clone());
        let length = path.len();
        let along = |at: f64| -> Vec3 {
            if at >= length {
                let end = path.point(path.arcs.len() - 1, 1.0);
                let before = spline_at(&path, (length - 200.0).max(0.0));
                return match unit3(vsub(end, before)) {
                    Some(d) => vadd(end, vscale(v3(d.x, d.y, 0.0), at - length)),
                    None => end,
                };
            }
            spline_at(&path, at.max(0.0))
        };
        let count = (length / WALK_STEP_MM).ceil().max(1.0) as usize;
        let first_fov = self.last.map(|k| k.fov_deg).unwrap_or(fov);
        // Where the camera looks along the path, as a heading and a tilt. The
        // headings are unwound along the walk, so every turn, the first and
        // the last included, goes the way the walk itself turns: out of a
        // room's hold it never swings back across the doorway it came in by.
        let yaw = |d: Vec3| d.y.atan2(d.x);
        let tilt_of = |d: Vec3| d.z.clamp(-1.0, 1.0).asin();
        let near_to = |a: f64, to: f64| {
            let mut a = a;
            while a - to > std::f64::consts::PI {
                a -= 2.0 * std::f64::consts::PI;
            }
            while to - a > std::f64::consts::PI {
                a += 2.0 * std::f64::consts::PI;
            }
            a
        };
        let mut looks: Vec<(f64, Vec3, f64, f64)> = Vec::with_capacity(count + 1);
        for i in 0..=count {
            let at = length * i as f64 / count as f64;
            let p = along(at);
            let mut reach = lead;
            while reach > 300.0 && solids.hit(p, along(at + reach), false).is_some() {
                reach *= 0.7;
            }
            let mut d = vsub(along(at + reach), p);
            d.z *= tilt;
            let dir = unit3(d).or(looks.last().map(|l| v3(l.2.cos(), l.2.sin(), 0.0))).unwrap_or(v3(1.0, 0.0, 0.0));
            let h = match looks.last() {
                Some(l) => near_to(yaw(dir), l.2),
                None => yaw(dir),
            };
            looks.push((at, p, h, tilt_of(dir)));
        }
        let start = self.last.and_then(|k| unit3(vsub(k.target, k.position))).map(|d| (near_to(yaw(d), looks[0].2), tilt_of(d)));
        let end = end_target.and_then(|t| unit3(vsub(t, looks[count].1))).map(|d| (near_to(yaw(d), looks[count].2), tilt_of(d)));
        let mut keys = Vec::with_capacity(count + 1);
        for (i, &(at, p, mut h, mut v)) in looks.iter().enumerate() {
            if let Some((sh, sv)) = start {
                let k = smooth(at / WALK_BLEND_MM);
                h = sh + (h - sh) * k;
                v = sv + (v - sv) * k;
            }
            if let Some((eh, ev)) = end {
                let k = smooth((length - at) / WALK_BLEND_MM);
                let eh = near_to(eh, h);
                h = eh + (h - eh) * k;
                v = ev + (v - ev) * k;
            }
            let dir = v3(h.cos() * v.cos(), h.sin() * v.cos(), v.sin());
            let mut target = vadd(p, vscale(dir, LOOK_MM));
            if i == 0 {
                if let Some(k) = self.last {
                    target = k.target;
                }
            }
            if i == count {
                if let Some(t) = end_target {
                    target = t;
                }
            }
            keys.push(key(i as f64 / count as f64, p, target, if i == 0 { first_fov } else { fov }));
        }
        let speed = mps.unwrap_or(self.pace);
        // InOut peaks at 1 / (1 - EASE_RAMP) of its average: the top speed is the pace.
        let seconds = length / (speed * (1.0 - EASE_RAMP));
        self.push(shot(name, kind, keys, seconds, Ease::InOut));
    }

    /// A hold at the last camera: a slow pan across `look` (a plan direction).
    fn hold(&mut self, name: &str, look: Point) {
        let Some(k) = self.last else { return };
        let p = k.position;
        let base = angle_of(look);
        let t = |deg: f64| {
            let d = dir_deg(base + deg);
            v3(p.x + d.x * LOOK_MM, p.y + d.y * LOOK_MM, p.z - 150.0)
        };
        let keys = vec![key(0.0, p, k.target, k.fov_deg), key(0.5, p, t(0.0), FOV_IN), key(1.0, p, t(-PAN_DEG), FOV_IN)];
        self.push(shot(name, ShotMove::Hold, keys, HOLD_S, Ease::InOut));
    }

    /// Across the room to its farthest corner, the widest view a property
    /// photographer takes from a doorway; the farthest corner well away
    /// from the leaf of the door just come through, when that leaf stands
    /// near (GA P1-9: the hold and the turn after it looked into it).
    fn hold_look(&self, li: usize, face: usize, at: Point, fallback: Point, door: Option<usize>) -> Point {
        let f = &self.analysis.levels[li].faces[face];
        let mut corners: Vec<Point> = f.net.iter().copied().filter(|c| dist(*c, at) > 900.0).collect();
        corners.sort_by(|a, b| dist(*b, at).total_cmp(&dist(*a, at)));
        let leaves: Vec<Point> = door.map(|di| self.open_leaves(di)).unwrap_or_default().into_iter().filter_map(|(a, b)| {
            let m = lerp(a, b, 0.5);
            if dist(m, at) < 2000.0 { unit(sub(m, at)) } else { None }
        }).collect();
        let clear = |c: &Point| unit(sub(*c, at)).is_some_and(|d| leaves.iter().all(|l| dot(d, *l) < 70f64.to_radians().cos()));
        corners.iter().find(|c| clear(c)).or(corners.first()).and_then(|c| unit(sub(*c, at))).unwrap_or(fallback)
    }

    /// The leaves of door `di` as the tour opens it to pass (`open_wide`).
    fn open_leaves(&self, di: usize) -> Vec<(Point, Point)> {
        let o = self.doors[di].opening;
        let Some(Element::Wall(w)) = self.project.elements.iter().find(|e| e.id() == &o.wall_id) else { return vec![] };
        let level = self.level(self.doors[di].level);
        leaf_lines(o, w, open_wide(o, w, level, &self.derived, self.project))
    }

    /// The start of a hold's pan, so the walk before it ends looking there.
    fn hold_start(&self, li: usize, at: Point, look: Point) -> Vec3 {
        let d = dir_deg(angle_of(look) + PAN_DEG);
        at_z(add(at, scale(d, LOOK_MM)), self.eye(li) - 150.0)
    }

    /// Walk from where the camera is (in `from` on level li) to room `to`,
    /// through doors, and hold there. False when no door leads there.
    fn visit(&mut self, solids: &Solids, li: usize, from: Node, at: Point, to: usize) -> bool {
        let Some(route) = self.route(li, from, Node::Face(to)) else { return false };
        self.visit_via(solids, li, from, at, to, &route);
        true
    }

    /// `visit` through these doors, in order.
    fn visit_via(&mut self, solids: &Solids, li: usize, from: Node, at: Point, to: usize, route: &[usize]) {
        let z = self.eye(li);
        let mut plan: Vec<Point> = vec![];
        let mut here = from;
        let mut pos = at;
        let mut last_in = None;
        for &di in route {
            let d = &self.doors[di];
            let [before, mid, after] = d.crossing(here);
            self.inside(solids, li, here, pos, before, &mut plan);
            plan.push(mid);
            plan.push(after);
            last_in = Some(d.towards(d.other(here)));
            here = d.other(here);
            pos = after;
        }
        let into = last_in.unwrap_or(pt(0.0, 1.0));
        let last_door = route.last().copied();
        // Stand a little way in, towards the middle of the room.
        let label = self.analysis.levels[li].faces[to].label;
        let toward = sub(label, pos);
        let step = (length(toward) * 0.35).min(1200.0);
        let stand = match unit(toward) {
            Some(u) if step > 200.0 => add(pos, scale(u, step)),
            _ => pos,
        };
        let stand = if self.analysis.levels[li].faces[to].contains_net(stand) { stand } else { pos };
        if dist(stand, pos) > 50.0 {
            plan.push(stand);
        }
        let look = self.hold_look(li, to, stand, into, last_door);
        let end = self.hold_start(li, stand, look);
        let name = self.room_name((li, to));
        let points = plan.into_iter().map(|p| at_z(p, z)).collect();
        self.walk(solids, &format!("Walk to {name}"), ShotMove::Walk, points, Some(end), FOV_IN, None, WALK_LEAD_MM, WALK_TILT);
        self.hold(&name, look);
        self.visited.insert((li, to));
    }

    /// Every room with a door on level li, nearest by doors first, from
    /// `start`. Returns where the camera ends: its face and plan point.
    fn tour_level(&mut self, solids: &Solids, li: usize, mut here: Node, mut at: Point) -> (Node, Point) {
        loop {
            let mut best: Option<(usize, usize)> = None;
            for (&(l, f), _) in self.rooms.iter() {
                if l != li || self.visited.contains(&(l, f)) {
                    continue;
                }
                if let Some(r) = self.route(li, here, Node::Face(f)) {
                    if best.map(|(_, n)| r.len() < n).unwrap_or(true) {
                        best = Some((f, r.len()));
                    }
                }
            }
            let Some((f, _)) = best else { break };
            if !self.visit(solids, li, here, at, f) {
                break;
            }
            here = Node::Face(f);
            at = self.last.map(|k| flat(k.position)).unwrap_or(at);
        }
        (here, at)
    }

    /// A stair from level li up to level lj: its walk line from the bottom,
    /// z the walking surface above level li's floor.
    fn stair_to(&self, li: usize, lj: usize) -> Option<(&'a Stair, Vec<Vec3>)> {
        let from = &self.analysis.levels[li].level_id;
        let to = &self.analysis.levels[lj].level_id;
        for el in &self.project.elements {
            let Element::Stair(s) = el else { continue };
            if &s.level_id != from {
                continue;
            }
            let geo = self.derived.stairs.iter().find(|g| g.stair_id == s.id);
            let arrives = geo
                .and_then(|g| g.arrives_at.clone())
                .or_else(|| crate::slabs::stair_top_level(self.project, s).map(|l| l.id.clone()));
            if arrives.as_ref() != Some(to) {
                continue;
            }
            let line = match geo {
                Some(g) if g.walk_line.len() >= 2 => g.walk_line.clone(),
                // Straight until Derived::stairs lands (L2): up the flight's middle.
                _ => {
                    let (o, up, _) = crate::slabs::stair_frame(s);
                    let rise = self.level(li).height_mm;
                    vec![at_z(o, 0.0), at_z(add(o, scale(up, s.run_mm)), rise)]
                }
            };
            return Some((s, line));
        }
        None
    }

    /// Walk to the bottom of the stair from li to lj, climb it and step off.
    /// Returns where the camera stands on lj.
    fn climb(&mut self, solids: &Solids, li: usize, lj: usize, here: Node, at: Point) -> Option<(Node, Point)> {
        let (_, line) = self.stair_to(li, lj)?;
        let base = self.level(li).elevation_mm;
        let first = flat(line[0]);
        let second = flat(*line.get(1)?);
        let up_in = unit(sub(second, first))?;
        let n = line.len();
        let top = flat(line[n - 1]);
        let top_dir = unit(sub(top, flat(line[n - 2]))).unwrap_or(up_in);
        let approach = sub(first, scale(up_in, 700.0));
        // Off the top: 700 mm on, or short of that to keep 800 mm from a wall
        // ahead (GA P1-9: a stair that arrives facing a wall).
        let top_z = base + line[n - 1].z + EYE_MM;
        let ahead = solids.hit(at_z(top, top_z), at_z(add(top, scale(top_dir, 1500.0)), top_z), false).map(|(_, at)| dist(flat(at), top));
        let step_off = add(top, scale(top_dir, ahead.map(|h| (h - 800.0).clamp(150.0, 700.0)).unwrap_or(700.0)));
        // To the stair's foot: through doors to its face, then across.
        let foot = self.face_at(li, approach);
        let mut plan: Vec<Point> = vec![];
        let mut here = here;
        let mut pos = at;
        if let Some(route) = self.route(li, here, foot) {
            for &di in &route {
                let d = &self.doors[di];
                let [before, mid, after] = d.crossing(here);
                self.inside(solids, li, here, pos, before, &mut plan);
                plan.push(mid);
                plan.push(after);
                here = d.other(here);
                pos = after;
            }
        }
        self.inside(solids, li, here, pos, approach, &mut plan);
        let z = self.eye(li);
        let mut points: Vec<Vec3> = plan.into_iter().map(|p| at_z(p, z)).collect();
        for p in &line {
            points.push(v3(p.x, p.y, base + p.z + EYE_MM));
        }
        let rise_top = base + line[n - 1].z;
        points.push(at_z(step_off, rise_top + EYE_MM));
        self.walk(solids, "Up the stairs", ShotMove::Walk, points, None, FOV_IN, None, STAIR_LEAD_MM, STAIR_TILT);
        self.climbed += 1;
        let face = self.face_at(lj, step_off);
        Some((face, step_off))
    }

    /// The front door: an outside door of level li, into the living room or
    /// hall when there is one, else the widest. With its face inside.
    fn front_door(&self, li: usize) -> Option<usize> {
        let mut best: Option<(usize, f64)> = None;
        for (i, d) in self.doors.iter().enumerate() {
            if d.level != li || (d.left != Node::Outside && d.right != Node::Outside) {
                continue;
            }
            let inside = d.other(Node::Outside);
            let usage = match inside {
                Node::Face(f) => self.rooms.get(&(li, f)).map(|r| r.usage),
                Node::Outside => None,
            };
            let score = d.opening.width_mm
                + match usage {
                    Some(RoomUsage::Living) | Some(RoomUsage::Hallway) | Some(RoomUsage::Porch) => 2000.0,
                    Some(RoomUsage::Garage) | Some(RoomUsage::Laundry) | Some(RoomUsage::Storage) => -3000.0,
                    _ => 0.0,
                }
                + if d.opening.style == OpeningStyle::SwingDouble || d.opening.style == OpeningStyle::SwingSingle { 100.0 } else { 0.0 };
            if best.map(|(_, s)| score > s).unwrap_or(true) {
                best = Some((i, score));
            }
        }
        best.map(|(i, _)| i)
    }

    /// The building's plan box and the top of its walls.
    fn extent(&self) -> (Point, Point, f64, f64) {
        let mut lo = pt(f64::INFINITY, f64::INFINITY);
        let mut hi = pt(f64::NEG_INFINITY, f64::NEG_INFINITY);
        for f in &self.derived.footprints {
            for p in &f.polygon {
                lo = pt(lo.x.min(p.x), lo.y.min(p.y));
                hi = pt(hi.x.max(p.x), hi.y.max(p.y));
            }
        }
        let mut top: f64 = 0.0;
        let mut ground = f64::INFINITY;
        for el in &self.project.elements {
            if let Element::Wall(w) = el {
                for p in [w.start, w.end] {
                    lo = pt(lo.x.min(p.x), lo.y.min(p.y));
                    hi = pt(hi.x.max(p.x), hi.y.max(p.y));
                }
                if let Some(l) = self.project.levels.iter().find(|l| l.id == w.level_id) {
                    top = top.max(l.elevation_mm + w.height_mm.unwrap_or(l.height_mm));
                    ground = ground.min(l.elevation_mm);
                }
            }
        }
        if !ground.is_finite() {
            ground = 0.0;
        }
        (lo, hi, ground, top)
    }

    /// Fly in, orbit and push in to the front door, ending just outside it.
    fn exterior(&mut self, solids: &Solids, door: Option<usize>) {
        let (lo, hi, ground, top) = self.extent();
        if !lo.x.is_finite() {
            return;
        }
        let c = lerp(lo, hi, 0.5);
        let radius = (dist(lo, hi) / 2.0).max(4000.0);
        let mid = at_z(c, ground + (top - ground) * 0.45);
        // Where the push-in starts and ends: on the door's axis outside.
        let (door_at, door_out, door_half, door_li) = match door {
            Some(i) => {
                let d = &self.doors[i];
                (d.pass, d.towards(Node::Outside), d.half, d.level)
            }
            None => (pt(c.x, lo.y), pt(0.0, -1.0), 100.0, 0),
        };
        let eye = ground.max(self.level(door_li).elevation_mm) + EYE_MM;
        let o = add(door_at, scale(door_out, door_half + DOOR_CLEAR_MM));
        let mut s = add(door_at, scale(door_out, door_half + 2000.0));
        for push in [7000.0, 6000.0, 5000.0, 4000.0, 3000.0] {
            let cand = add(door_at, scale(door_out, door_half + push));
            if self.face_at(door_li, cand) == Node::Outside && !self.blocked(solids, at_z(cand, eye), at_z(o, eye)) {
                s = cand;
                break;
            }
        }
        let door_look = at_z(door_at, eye - 100.0);
        let r_orbit = (radius * 1.7).max(dist(s, c) + 3000.0);
        let z_orbit = top + 3500.0;
        let end_deg = angle_of(sub(s, c));
        let sweep = 140.0;
        let start_deg = end_deg - sweep;
        let on_orbit = |deg: f64| at_z(add(c, scale(dir_deg(deg), r_orbit)), z_orbit);
        // Fly in from far and high, curving onto the orbit.
        let far = at_z(add(c, scale(dir_deg(start_deg - 25.0), r_orbit * 2.8)), top + 18000.0);
        let near = at_z(add(c, scale(dir_deg(start_deg - 10.0), r_orbit * 1.7)), top + 9000.0);
        let fly = vec![far, near, on_orbit(start_deg)];
        let at = arc_times(&fly);
        let len = Spline::new(fly.clone()).len();
        let keys = fly.iter().zip(&at).map(|(p, a)| key(*a, *p, mid, FOV_DRONE)).collect();
        let mut first = shot("Fly in", ShotMove::FlyIn, keys, (len / FLY_MMPS).clamp(5.0, 9.0), Ease::InOut);
        first.transition = Transition { kind: TransitionKind::PlanLift, seconds: 2.0 };
        self.push(first);
        // Orbit: seven keys over the sweep, looking at the middle.
        let ring: Vec<Vec3> = (0..=7).map(|i| on_orbit(start_deg + sweep * i as f64 / 7.0)).collect();
        let at = arc_times(&ring);
        let len = Spline::new(ring.clone()).len();
        let keys = ring.iter().zip(&at).map(|(p, a)| key(*a, *p, mid, FOV_DRONE)).collect();
        self.push(shot("Orbit", ShotMove::Orbit, keys, (len / ORBIT_MMPS).clamp(7.0, 12.0), Ease::InOut));
        // Down and in to the front door.
        let pts = vec![on_orbit(end_deg), at_z(s, ground + 2400.0), at_z(o, eye)];
        let at = arc_times(&pts);
        let len = Spline::new(pts.clone()).len();
        let tg = [mid, door_look, door_look];
        let fov = [FOV_DRONE, FOV_OUT, FOV_IN];
        let keys = (0..3).map(|i| key(at[i], pts[i], tg[i], fov[i])).collect();
        self.push(shot("Push in to the front door", ShotMove::PushIn, keys, (len / PUSH_MMPS).clamp(4.0, 12.0), Ease::InOut));
    }

    /// Outside at dusk: a slow pull back from the front, day to dusk.
    fn dusk(&mut self, door: Option<usize>) {
        let (lo, hi, ground, top) = self.extent();
        if !lo.x.is_finite() {
            return;
        }
        let c = lerp(lo, hi, 0.5);
        let (door_at, out, half) = match door {
            Some(i) => {
                let d = &self.doors[i];
                (d.center, d.towards(Node::Outside), d.half)
            }
            None => (pt(c.x, lo.y), pt(0.0, -1.0), 100.0),
        };
        let size = dist(lo, hi);
        let near = add(door_at, scale(out, half + (size * 0.6).max(7000.0)));
        let far = add(c, scale(unit(sub(rotate_about(near, c, 12.0), c)).unwrap_or(out), dist(near, c) + 6000.0));
        let look_a = at_z(door_at, ground + 1800.0);
        let look_b = at_z(c, ground + (top - ground) * 0.45);
        let keys = vec![
            key(0.0, at_z(near, ground + 1700.0), look_a, FOV_OUT),
            key(1.0, at_z(far, ground + 3800.0), look_b, FOV_OUT),
        ];
        let mut s = shot("Dusk", ShotMove::PullOut, keys, 9.0, Ease::InOut);
        let light = |minutes: u16| ViewLight { month: 4, day: 15, minutes, sky: SkyKind::Clear, exposure_ev: None, lamps: LampMode::Auto };
        // From late afternoon to the sunset glow with the lamps coming on;
        // much past sunset the live view goes to night.
        s.light = Some(light(16 * 60 + 50));
        s.light_end = Some(light(18 * 60));
        s.transition = Transition { kind: TransitionKind::Dissolve, seconds: 1.5 };
        self.push(s);
    }
}

/// `Command::MakeTour`: the automatic route through the house, as one new
/// tour element.
pub fn make_tour(project: &mut Project, ids: &mut IdGen, name: &Option<String>, options: &AutoTour) -> Result<Outcome, CoreError> {
    in_range("tour_pace", "Walking pace", options.pace_mps, 0.3, 3.0, "m/s", &[])?;
    for l in &options.levels {
        require_level(project, l)?;
    }
    let mut order: Vec<&Level> = if options.levels.is_empty() {
        let with_rooms: BTreeSet<&str> = project
            .elements
            .iter()
            .filter_map(|e| match e {
                Element::Room(r) => Some(r.level_id.as_str()),
                _ => None,
            })
            .collect();
        let mut v: Vec<&Level> = project.levels.iter().filter(|l| with_rooms.contains(l.id.as_str())).collect();
        v.sort_by(|a, b| a.elevation_mm.total_cmp(&b.elevation_mm));
        v
    } else {
        options.levels.iter().filter_map(|id| project.levels.iter().find(|l| &l.id == id)).collect()
    };
    order.dedup_by(|a, b| a.id == b.id);
    if !project.elements.iter().any(|e| matches!(e, Element::Wall(_))) || order.is_empty() {
        return Err(CoreError::invalid(
            "tour_empty",
            "A tour walks through rooms: draw walls and rooms first.",
        ));
    }
    let order: Vec<Id> = order.iter().map(|l| l.id.clone()).collect();

    let snapshot = project.clone();
    let mut p = Planner::new(&snapshot, options.pace_mps);
    let solids = Solids::new(&snapshot, &p.derived);
    let level_ix = |p: &Planner, id: &Id| p.analysis.levels.iter().position(|l| &l.level_id == id);
    let Some(first) = level_ix(&p, &order[0]) else {
        return Err(CoreError::invalid("tour_empty", "A tour walks through rooms: draw walls and rooms first."));
    };
    let door = p.front_door(first);
    if options.exterior {
        p.exterior(&solids, door);
    }
    // In through the front door, or from the biggest room when there is none.
    let (mut here, mut at) = match door {
        Some(i) => {
            let d = &p.doors[i];
            let o = add(d.pass, scale(d.towards(Node::Outside), d.half + DOOR_CLEAR_MM));
            if p.last.is_none() {
                let eye = p.eye(first);
                let look = at_z(d.pass, eye - 100.0);
                let k = key(0.0, at_z(o, eye), look, FOV_IN);
                p.last = Some(k);
            }
            (Node::Outside, o)
        }
        None => {
            let biggest = p
                .rooms
                .keys()
                .filter(|(l, _)| *l == first)
                .max_by(|a, b| p.analysis.levels[a.0].faces[a.1].area_mm2.total_cmp(&p.analysis.levels[b.0].faces[b.1].area_mm2))
                .copied();
            match biggest {
                Some((l, f)) => {
                    let label = p.analysis.levels[l].faces[f].label;
                    let eye = p.eye(l);
                    let k = key(0.0, at_z(label, eye), at_z(add(label, pt(0.0, LOOK_MM)), eye - 150.0), FOV_IN);
                    p.last = Some(k);
                    (Node::Face(f), label)
                }
                None => (Node::Outside, pt(0.0, 0.0)),
            }
        }
    };
    if door.is_none() {
        if let Node::Face(f) = here {
            let name = p.room_name((first, f));
            let look = pt(0.0, 1.0);
            // A still first shot to hold on, faded in.
            let k = p.last.expect("set above");
            p.push(shot(&name, ShotMove::Hold, vec![key(0.0, k.position, k.target, FOV_IN)], 0.5, Ease::Linear));
            p.hold(&name, look);
            p.visited.insert((first, f));
        }
    }
    // The first room is the one behind the front door, whatever else opens outside.
    if let Some(i) = door {
        if let Node::Face(f) = p.doors[i].other(Node::Outside) {
            p.visit_via(&solids, first, Node::Outside, at, f, &[i]);
            here = Node::Face(f);
            at = p.last.map(|k| flat(k.position)).unwrap_or(at);
        }
    }
    let mut li = first;
    for (n, level_id) in order.iter().enumerate() {
        let Some(lj) = level_ix(&p, level_id) else { continue };
        if n > 0 {
            match p.climb(&solids, li, lj, here, at) {
                Some((h, a)) => {
                    here = h;
                    at = a;
                }
                None => {
                    // No stair between them: cut to the level's biggest room.
                    let biggest = p
                        .rooms
                        .keys()
                        .filter(|(l, f)| *l == lj && !p.visited.contains(&(*l, *f)))
                        .max_by(|a, b| p.analysis.levels[a.0].faces[a.1].area_mm2.total_cmp(&p.analysis.levels[b.0].faces[b.1].area_mm2))
                        .copied();
                    let Some((l, f)) = biggest else { continue };
                    let label = p.analysis.levels[l].faces[f].label;
                    let eye = p.eye(l);
                    let name = p.room_name((l, f));
                    let k = key(0.0, at_z(label, eye), at_z(add(label, pt(0.0, LOOK_MM)), eye - 150.0), FOV_IN);
                    let mut s = shot(&name, ShotMove::Hold, vec![k], 0.5, Ease::Linear);
                    s.transition = Transition { kind: TransitionKind::Dissolve, seconds: 0.5 };
                    p.push(s);
                    p.hold(&name, pt(0.0, 1.0));
                    p.visited.insert((l, f));
                    here = Node::Face(f);
                    at = label;
                }
            }
        }
        let (h, a) = p.tour_level(&solids, lj, here, at);
        here = h;
        at = a;
        li = lj;
        // Rooms only reached from outside: cut to their own door.
        let rest: Vec<usize> = p
            .rooms
            .keys()
            .filter(|(l, f)| *l == lj && !p.visited.contains(&(*l, *f)))
            .map(|(_, f)| *f)
            .collect();
        for f in rest {
            if p.visited.contains(&(lj, f)) {
                continue;
            }
            let Some(di) = p.doors.iter().position(|d| d.level == lj && ((d.left == Node::Outside && d.right == Node::Face(f)) || (d.right == Node::Outside && d.left == Node::Face(f)))) else {
                continue;
            };
            let d = &p.doors[di];
            let eye = p.eye(lj);
            let out = d.towards(Node::Outside);
            let start = add(d.pass, scale(out, d.half + DOOR_CLEAR_MM + 1500.0));
            let k = key(0.0, at_z(start, eye), at_z(d.pass, eye - 100.0), FOV_IN);
            let mut s = shot("Outside", ShotMove::Hold, vec![k], 0.5, Ease::Linear);
            s.transition = Transition { kind: TransitionKind::Dissolve, seconds: 0.5 };
            p.push(s);
            p.visit_via(&solids, lj, Node::Outside, start, f, &[di]);
            {
                here = Node::Face(f);
                at = p.last.map(|k| flat(k.position)).unwrap_or(at);
                let (h, a) = p.tour_level(&solids, lj, here, at);
                here = h;
                at = a;
            }
        }
    }
    let _ = li;
    if options.day_to_dusk {
        p.dusk(door);
    }
    if p.shots.is_empty() {
        return Err(CoreError::invalid("tour_empty", "A tour walks through rooms: draw walls and rooms first."));
    }

    let taken: BTreeSet<&str> = project
        .elements
        .iter()
        .filter_map(|e| match e {
            Element::Tour(t) => Some(t.name.as_str()),
            _ => None,
        })
        .collect();
    let wanted = name.as_deref().map(str::trim).filter(|n| !n.is_empty()).unwrap_or("Tour");
    let mut tour_name = wanted.to_string();
    let mut i = 2;
    while taken.contains(tour_name.as_str()) {
        tour_name = format!("{wanted} {i}");
        i += 1;
    }
    let rooms = p.visited.len();
    let climbed = p.climbed;
    let mut tour = Tour { id: ids.next_id(), name: tour_name.clone(), shots: p.shots, fps: 30 };
    assign_ids(&mut tour, ids, false);
    let seconds: f64 = tour.shots.iter().map(|s| s.seconds).sum();
    let skipped = snapshot
        .elements
        .iter()
        .filter(|e| matches!(e, Element::Room(_)))
        .count()
        .saturating_sub(rooms);
    let mut summary = format!(
        "Made tour \"{tour_name}\": {} shots, {:.0} s, {rooms} room{}",
        tour.shots.len(),
        seconds,
        if rooms == 1 { "" } else { "s" }
    );
    if climbed > 0 {
        summary.push_str(&format!(", up {climbed} stair{}", if climbed == 1 { "" } else { "s" }));
    }
    if skipped > 0 {
        summary.push_str(&format!("; {skipped} room{} with no door left out", if skipped == 1 { "" } else { "s" }));
    }
    project.elements.push(Element::Tour(tour));
    let mut o = outcome("Make tour");
    o.summary = Some(format!("{summary}."));
    Ok(o)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn k(at: f64, x: f64, y: f64) -> ShotKey {
        key(at, v3(x, y, 1600.0), v3(x, y + 3000.0, 1600.0), 60.0)
    }

    fn tour(shots: Vec<Shot>) -> Tour {
        Tour { id: "t".into(), name: "T".into(), shots, fps: 30 }
    }

    #[test]
    fn inout_ease_is_continuous_and_peaks_at_the_ramp_speed() {
        let mut prev = 0.0;
        let mut top: f64 = 0.0;
        let n = 10_000;
        for i in 1..=n {
            let u = i as f64 / n as f64;
            let p = ease(Ease::InOut, u);
            top = top.max((p - prev) * n as f64);
            assert!(p >= prev);
            prev = p;
        }
        assert!((prev - 1.0).abs() < 1e-12);
        assert!((top - 1.0 / (1.0 - EASE_RAMP)).abs() < 1e-3, "peak {top}");
    }

    #[test]
    fn a_straight_shot_moves_at_an_even_pace() {
        let s = shot("w", ShotMove::Walk, vec![k(0.0, 0.0, 0.0), k(0.25, 0.0, 1000.0), k(1.0, 0.0, 4000.0)], 4.0, Ease::Linear);
        let t = tour(vec![s]);
        let p = crate::templates::sample_bungalow();
        let f = sample(&p, &t, 10.0);
        assert_eq!(f.len(), 40);
        for w in f.windows(2) {
            let step = vdist(w[0].pose.position, w[1].pose.position);
            assert!((step - 100.0).abs() < 0.5, "step {step}");
        }
    }

    #[test]
    fn centripetal_spline_passes_through_its_keys() {
        let pts = vec![v3(0.0, 0.0, 0.0), v3(1000.0, 0.0, 0.0), v3(1000.0, 10.0, 0.0), v3(3000.0, 2000.0, 0.0)];
        let s = Spline::new(pts.clone());
        for j in 0..3 {
            assert!(vdist(s.point(j, 0.0), pts[j]) < 1e-6);
            assert!(vdist(s.point(j, 1.0), pts[j + 1]) < 1e-6);
        }
        // No loop at the tight middle segment: it stays near its chord.
        for i in 0..=20 {
            let p = s.point(1, i as f64 / 20.0);
            assert!(p.x > 990.0 && p.x < 1010.0, "{p:?}");
        }
    }

    // ------------------------------------------- what a tour frame shows
    //
    // GA P1-9: a door leaf filled the frame at each doorway and the stair
    // climb looked at bare walls. Each frame's view is cast as a grid of rays
    // over the 16:9 picture against the walls (their doors and windows open)
    // and the door leaves as the 3D view draws them at that frame: at rest
    // ajar (scene/openings.ts), or as the tour's `door_moves` swing them.

    /// Rays across and down a frame, and how far they look.
    const RAYS: (usize, usize) = (24, 14);
    const REACH_MM: f64 = 8000.0;
    /// A wall nearer than this fills the frame; so does any door leaf.
    const NEAR_WALL_MM: f64 = 600.0;

    struct Face {
        a: Point,
        b: Point,
        z0: f64,
        z1: f64,
        leaf: bool,
        /// The door a leaf belongs to.
        door: String,
        /// A wall face: its wall's start and direction, and its doors and
        /// windows, (along from, along to, z from, z to).
        start: Point,
        dir: Point,
        holes: Vec<(f64, f64, f64, f64)>,
    }

    struct WallBody {
        outline: Vec<Point>,
        z0: f64,
        z1: f64,
        start: Point,
        dir: Point,
        holes: Vec<(f64, f64, f64, f64)>,
    }

    impl WallBody {
        fn hole_at(&self, p: Point, z: f64) -> bool {
            let along = dot(sub(p, self.start), self.dir);
            self.holes.iter().any(|&(a, b, lo, hi)| along > a && along < b && z > lo && z < hi)
        }
    }

    /// The walls, and the door leaves at `open(opening id)`.
    fn faces(project: &Project, derived: &Derived, open: &dyn Fn(&str) -> f64) -> (Vec<Face>, Vec<WallBody>) {
        let levels: BTreeMap<&str, &Level> = project.levels.iter().map(|l| (l.id.as_str(), l)).collect();
        let mut out = vec![];
        let mut bodies = vec![];
        for el in &project.elements {
            let Element::Wall(w) = el else { continue };
            let Some(level) = levels.get(w.level_id.as_str()) else { continue };
            let Some(d) = unit(sub(w.end, w.start)) else { continue };
            let Some(g) = derived.walls.iter().find(|g| g.wall_id == w.id) else { continue };
            let (z0, z1) = (level.elevation_mm, level.elevation_mm + w.height_mm.unwrap_or(level.height_mm));
            let mut holes = vec![];
            for o in project.elements.iter().filter_map(|e| match e {
                Element::Opening(o) if o.wall_id == w.id => Some(o),
                _ => None,
            }) {
                let (a, b) = (o.offset_mm - o.width_mm / 2.0, o.offset_mm + o.width_mm / 2.0);
                holes.push((a, b, z0 + o.sill_mm, z0 + o.sill_mm + o.height_mm));
                for (p, q) in leaf_lines(o, w, open(&o.id)) {
                    out.push(Face { a: p, b: q, z0: z0 + o.sill_mm, z1: z0 + o.sill_mm + o.height_mm - DOOR_FRAME_MM - 10.0, leaf: true, door: o.id.clone(), start: p, dir: d, holes: vec![] });
                }
            }
            let ring = &g.outline;
            for i in 0..ring.len() {
                out.push(Face { a: ring[i], b: ring[(i + 1) % ring.len()], z0, z1, leaf: false, door: String::new(), start: w.start, dir: d, holes: holes.clone() });
            }
            bodies.push(WallBody { outline: ring.clone(), z0, z1, start: w.start, dir: d, holes });
        }
        (out, bodies)
    }

    /// The share of a frame's view whose first hit within REACH_MM is a door
    /// leaf, or a wall nearer than NEAR_WALL_MM; the share that is leaves;
    /// and the distance the middle of the view sees to its first wall or
    /// leaf (REACH_MM: none).
    fn filled(faces: &[Face], bodies: &[WallBody], pose: &Pose) -> (f64, f64, f64) {
        let o = pose.position;
        let po = flat(o);
        if bodies.iter().any(|w| o.z > w.z0 && o.z < w.z1 && point_in_polygon(po, &w.outline) && !w.hole_at(po, o.z)) {
            return (1.0, 0.0, 0.0);
        }
        let near: Vec<&Face> = faces.iter().filter(|f| dist_point_segment(po, f.a, f.b) < REACH_MM).collect();
        let f = vsub(pose.target, o);
        let f = vscale(f, 1.0 / vlen(f).max(1e-9));
        let right = v3(f.y, -f.x, 0.0);
        let right = vscale(right, 1.0 / vlen(right).max(1e-9));
        let up = v3(right.y * f.z - right.z * f.y, right.z * f.x - right.x * f.z, right.x * f.y - right.y * f.x);
        let ty = (pose.fov / 2.0).to_radians().tan();
        let tx = ty * 16.0 / 9.0;
        let cast = |x: f64, y: f64| -> Option<(f64, bool)> {
            let r = vadd(f, vadd(vscale(right, x * tx), vscale(up, y * ty)));
            let r = vscale(r, 1.0 / vlen(r));
            let rp = pt(r.x, r.y);
            let mut best: Option<(f64, bool)> = None;
            for face in &near {
                let e = sub(face.b, face.a);
                let den = cross(rp, e);
                if den.abs() < 1e-12 {
                    continue;
                }
                let q = sub(face.a, po);
                let lam = cross(q, e) / den;
                let mu = cross(q, rp) / den;
                if lam <= 1e-6 || lam > REACH_MM || !(0.0..=1.0).contains(&mu) || best.is_some_and(|(b, _)| lam >= b) {
                    continue;
                }
                let z = o.z + lam * r.z;
                if z < face.z0 || z > face.z1 {
                    continue;
                }
                let p = add(face.a, scale(e, mu));
                let along = dot(sub(p, face.start), face.dir);
                if !face.leaf && face.holes.iter().any(|&(a, b, lo, hi)| along > a && along < b && z > lo && z < hi) {
                    continue;
                }
                best = Some((lam, face.leaf));
            }
            best
        };
        let (nx, ny) = RAYS;
        let (mut bad, mut leaves) = (0, 0);
        for i in 0..nx {
            for j in 0..ny {
                let x = 2.0 * (i as f64 + 0.5) / nx as f64 - 1.0;
                let y = 2.0 * (j as f64 + 0.5) / ny as f64 - 1.0;
                if let Some((lam, leaf)) = cast(x, y) {
                    if leaf || lam < NEAR_WALL_MM {
                        bad += 1;
                    }
                    if leaf {
                        leaves += 1;
                    }
                }
            }
        }
        let middle = cast(0.0, 0.0).map(|(lam, _)| lam).unwrap_or(REACH_MM);
        let all = (nx * ny) as f64;
        (bad as f64 / all, leaves as f64 / all, middle)
    }

    /// One frame as the test sees it.
    #[derive(Clone, Copy, Debug)]
    struct View {
        t: f64,
        shot: usize,
        /// Filled by a leaf or a near wall; by a leaf alone.
        share: f64,
        leaf: f64,
        /// How far the middle of the view sees.
        middle: f64,
        pose: Pose,
    }

    /// Every frame of the tour at 10 frames a second, with its doors as the
    /// tour opens them.
    fn views(project: &Project, t: &Tour) -> Vec<View> {
        let derived = crate::derive::derived_from(project, &analyze(project));
        // The report can show the doors left as drawn (before GA P1-9).
        let moves = if std::env::var("GUHIT_TOUR_DOORS_AS_DRAWN").is_ok() { vec![] } else { door_moves(project, &derived, &sample(project, t, t.fps as f64)) };
        let list = sample(project, t, 10.0);
        list.iter()
            .map(|fr| {
                let open = |id: &str| {
                    let own: Vec<&DoorMove> = moves.iter().filter(|m| m.id == id).collect();
                    door_open_at(&own, LEAF_REST, fr.t)
                };
                let (fs, bodies) = faces(project, &derived, &open);
                let (share, leaf, middle) = filled(&fs, &bodies, &fr.pose);
                View { t: fr.t, shot: fr.shot, share, leaf, middle, pose: fr.pose }
            })
            .collect()
    }

    fn made(project: Project, exterior: bool) -> (Project, Tour) {
        let mut p = project;
        let options = AutoTour { exterior, day_to_dusk: true, pace_mps: 1.0, levels: vec![] };
        let mut ids = IdGen::new(&p, &Command::MakeTour { name: None, options: options.clone() });
        make_tour(&mut p, &mut ids, &None, &options).expect("a tour");
        let t = p.elements.iter().rev().find_map(|e| match e {
            Element::Tour(t) => Some(t.clone()),
            _ => None,
        });
        (p, t.expect("a tour"))
    }

    /// No frame more than half filled by a door leaf or a wall within 600 mm.
    fn assert_clear_views(name: &str, project: &Project, t: &Tour) {
        let v = views(project, t);
        let worst = v.iter().copied().max_by(|a, b| a.share.total_cmp(&b.share)).expect("frames");
        let over: Vec<String> = v.iter().filter(|x| x.share > 0.5).map(|x| format!("{:.1} s {} {:.0}%", x.t, t.shots[x.shot].name, x.share * 100.0)).collect();
        assert!(over.is_empty(), "{name}: {} frames over half filled, the worst {:.0}% at {:.1} s in \"{}\": {over:?}", over.len(), worst.share * 100.0, worst.t, t.shots[worst.shot].name);
    }

    /// The bungalow with a second floor reached by a stair in the living
    /// room (an L or a straight one): a hall and a bedroom upstairs.
    fn two_storey(shape: &str) -> Project {
        let mut p = crate::templates::sample_bungalow();
        let ground = p.levels[0].id.clone();
        let up = "lvl-up";
        let mut add = |v: Value| p.elements.push(serde_json::from_value(v).expect("an element"));
        for (i, (a, b)) in [((0.0, 0.0), (8000.0, 0.0)), ((8000.0, 0.0), (8000.0, 6000.0)), ((8000.0, 6000.0), (0.0, 6000.0)), ((0.0, 6000.0), (0.0, 0.0)), ((5000.0, 0.0), (5000.0, 6000.0))].iter().enumerate() {
            add(json!({ "kind": "wall", "id": format!("up-wall-{i}"), "level_id": up, "start": { "x": a.0, "y": a.1 }, "end": { "x": b.0, "y": b.1 }, "thickness_mm": 150.0, "height_mm": null, "material_id": null }));
        }
        add(json!({ "kind": "opening", "id": "up-door", "wall_id": "up-wall-4", "opening_type": "door", "style": "swing_single", "offset_mm": 3000.0, "width_mm": 900.0, "height_mm": 2100.0, "sill_mm": 0.0, "flip_side": false, "flip_hinge": false, "material_id": null }));
        add(json!({ "kind": "room", "id": "up-hall", "level_id": up, "name": "Hall", "usage": "hallway", "seed": { "x": 2500.0, "y": 3000.0 }, "floor_material_id": null, "auto_named": false }));
        add(json!({ "kind": "room", "id": "up-bed", "level_id": up, "name": "Bedroom 2", "usage": "bedroom", "seed": { "x": 6500.0, "y": 3000.0 }, "floor_material_id": null, "auto_named": false }));
        let mut stair = json!({ "kind": "stair", "id": "stair-1", "level_id": ground, "origin": { "x": 4000.0, "y": 1500.0 }, "rotation_deg": 0.0, "width_mm": 1000.0, "run_mm": 3000.0, "riser_count": 17, "railing": null });
        if shape != "straight" {
            stair["shape"] = json!(shape);
            stair["turn"] = json!("right");
            stair["first_flight_risers"] = json!(9);
            stair["landing_mm"] = json!(1000.0);
            stair["origin"] = json!({ "x": 900.0, "y": 1300.0 });
            stair["run_mm"] = json!(3500.0);
        }
        add(stair);
        p.levels.push(Level { id: up.into(), name: "Second Floor".into(), elevation_mm: 3000.0, height_mm: 3000.0 });
        p
    }

    fn lovell() -> Project {
        serde_json::from_str(include_str!("../tests/data/wave1-g5-lovell.project.json")).expect("the Lovell house reads")
    }

    /// The Wave 2 showcase house the GA review toured (an L stair, 17 doors),
    /// when its file is in tests/data (or at GUHIT_SHOWCASE_PROJECT).
    fn showcase() -> Option<Project> {
        let path = std::env::var("GUHIT_SHOWCASE_PROJECT").map(std::path::PathBuf::from).unwrap_or_else(|_| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/reyes-residence-gs.project.json"));
        let text = std::fs::read_to_string(path).ok()?;
        Some(serde_json::from_str(&text).expect("the showcase house reads"))
    }

    /// The frames of a tour's climbs on the flights (off the floors).
    fn on_flights(project: &Project, t: &Tour) -> Vec<View> {
        let v = views(project, t);
        let mut out = vec![];
        for (i, _) in t.shots.iter().enumerate().filter(|(_, s)| s.name == "Up the stairs") {
            let own: Vec<View> = v.iter().copied().filter(|x| x.shot == i).collect();
            let (Some(lo), Some(hi)) = (own.first().map(|x| x.pose.position.z), own.last().map(|x| x.pose.position.z)) else { continue };
            out.extend(own.into_iter().filter(|x| x.pose.position.z > lo + 100.0 && x.pose.position.z < hi - 100.0));
        }
        out
    }

    #[test]
    fn no_tour_frame_is_half_filled_by_a_door_leaf_or_a_near_wall() {
        // GA P1-9. As drawn, the bungalow's doorways were 96 to 100 percent
        // door leaf (`tour_views_report` with GUHIT_TOUR_DOORS_AS_DRAWN).
        for (name, project) in [("bungalow", crate::templates::sample_bungalow()), ("two storey", two_storey("straight")), ("L stair", two_storey("l"))] {
            let (p, t) = made(project, true);
            assert_clear_views(name, &p, &t);
        }
        if let Some(p) = showcase() {
            let (p, t) = made(p, true);
            assert_clear_views("showcase", &p, &t);
        }
    }

    #[test]
    fn the_climb_looks_up_the_flight_and_never_at_a_wall_beside_it() {
        let mut houses = vec![("straight", two_storey("straight")), ("L", two_storey("l"))];
        if let Some(p) = showcase() {
            houses.push(("showcase", p));
        }
        for (name, project) in houses {
            let (p, t) = made(project, true);
            let on = on_flights(&p, &t);
            assert!(on.len() > 20, "{name}: frames on the flights: {}", on.len());
            // The middle of the view sees past a metre all the way up.
            let near: Vec<String> = on.iter().filter(|x| x.middle < 1000.0).map(|x| format!("{:.1} s {:.0} mm", x.t, x.middle)).collect();
            assert!(near.is_empty(), "{name}: the climb looks at a wall: {near:?}");
            // Up the flight: tilted up while it climbs.
            let up = on.iter().filter(|x| x.pose.target.z > x.pose.position.z).count();
            assert!(up * 10 >= on.len() * 8, "{name}: {up} of {} frames look up the flight", on.len());
        }
    }

    #[test]
    fn a_tour_opens_the_doors_it_passes_through_and_shuts_the_ones_it_passes() {
        let (p, t) = made(crate::templates::sample_bungalow(), true);
        let derived = crate::derive::derived_from(&p, &analyze(&p));
        let list = sample(&p, &t, 30.0);
        let moves = door_moves(&p, &derived, &list);
        let doors: Vec<&Opening> = p
            .elements
            .iter()
            .filter_map(|e| match e {
                Element::Opening(o) if o.opening_type == OpeningType::Door => Some(o),
                _ => None,
            })
            .collect();
        assert!(!doors.is_empty());
        for o in doors {
            let own: Vec<&DoorMove> = moves.iter().filter(|m| m.id == o.id).collect();
            assert!(!own.is_empty(), "door {} never set", o.id);
            for m in &own {
                assert!(m.to - m.from <= DOOR_SWING_S + 1e-9 && m.open >= 0.0 && m.open <= LEAF_WIDE_DEG / 90.0 + 1e-9, "{m:?}");
            }
            // Opened wide, done before the camera reaches the door.
            let opened = own.iter().find(|m| m.open >= 1.0).expect("the tour walks through every door of the bungalow");
            let w = p
                .elements
                .iter()
                .find_map(|e| match e {
                    Element::Wall(w) if w.id == o.wall_id => Some(w),
                    _ => None,
                })
                .expect("its wall");
            let center = add(w.start, scale(unit(sub(w.end, w.start)).expect("a wall"), o.offset_mm));
            let reach = list.iter().find(|f| dist(flat(f.pose.position), center) < 400.0).expect("the camera reaches it").t;
            assert!(opened.to <= reach, "door {} opens at {:.2} s, the camera is there at {:.2} s", o.id, opened.to, reach);
        }
        // The answer carries them, for playback and the video alike.
        let f = frames(&p, &derived, &t.id, None).expect("frames");
        assert_eq!(f["doors"].as_array().map(|d| d.len()), Some(moves.len()));
        // A door shut from the start never moves on screen; a swing eases.
        let shut = DoorMove { id: "x".into(), from: -1.0, to: -1.0, open: 0.0 };
        assert_eq!(door_open_at(&[&shut], LEAF_REST, 0.0), 0.0);
        let swing = DoorMove { id: "y".into(), from: 2.0, to: 2.7, open: 1.5 };
        assert!((door_open_at(&[&swing], LEAF_REST, 2.35) - (LEAF_REST + (1.5 - LEAF_REST) * 0.5)).abs() < 1e-9);
        assert_eq!(door_open_at(&[&swing], LEAF_REST, 9.0), 1.5);
    }

    #[test]
    #[ignore = "a report: cargo test -p guhit-core --lib tours::tests::tour_views_report -- --ignored --nocapture"]
    fn tour_views_report() {
        let mut list = vec![("bungalow", made(crate::templates::sample_bungalow(), true)), ("two storey", made(two_storey("straight"), true)), ("L stair", made(two_storey("l"), true)), ("Lovell", made(lovell(), false))];
        if let Ok(path) = std::env::var("GUHIT_TOUR_PROJECT") {
            let p: Project = serde_json::from_str(&std::fs::read_to_string(path).expect("the project file")).expect("a project");
            // Its own tour as saved (the GA review's), then a new one.
            let stored = p.elements.iter().find_map(|e| match e {
                Element::Tour(t) => Some(t.clone()),
                _ => None,
            });
            if let Some(t) = stored {
                list.push(("saved", (p.clone(), t)));
            }
            list.push(("given", made(p, true)));
        }
        for (name, (p, t)) in &list {
            let v = views(p, t);
            for (i, s) in t.shots.iter().enumerate() {
                let own: Vec<&View> = v.iter().filter(|x| x.shot == i).collect();
                let Some(w) = own.iter().max_by(|a, b| a.share.total_cmp(&b.share)) else { continue };
                let close = own.iter().map(|x| x.middle).fold(f64::INFINITY, f64::min);
                println!("{name:10} {:28} {:5.1} s  worst {:3.0}% ({:3.0}% leaves) at {:5.1} s  middle sees {:5.0} mm at least", s.name, s.seconds, w.share * 100.0, w.leaf * 100.0, w.t, close);
                if s.name == "Up the stairs" {
                    let (lo, hi) = (own.first().map(|x| x.pose.position.z).unwrap_or(0.0), own.last().map(|x| x.pose.position.z).unwrap_or(0.0));
                    let on: Vec<&&View> = own.iter().filter(|x| x.pose.position.z > lo + 100.0 && x.pose.position.z < hi - 100.0).collect();
                    let short = on.iter().filter(|x| x.middle < 1000.0).count();
                    println!("    on the flights: {} frames, {} look at a wall within 1 m, middle sees {:.0} mm at least", on.len(), short, on.iter().map(|x| x.middle).fold(f64::INFINITY, f64::min));
                }
                if std::env::var("GUHIT_TOUR_DETAIL").is_ok() {
                    let derived = crate::derive::derived_from(p, &analyze(p));
                    let moves = door_moves(p, &derived, &sample(p, t, t.fps as f64));
                    if i == 0 {
                        for m in &moves {
                            println!("    move {} {:6.2} to {:6.2} s open {:.2}", &m.id[..8.min(m.id.len())], m.from, m.to, m.open);
                        }
                    }
                    for x in own.iter().filter(|x| x.share > 0.5) {
                        let c = x.pose.position;
                        let d = vsub(x.pose.target, c);
                        let open = |id: &str| door_open_at(&moves.iter().filter(|m| m.id == id).collect::<Vec<_>>(), LEAF_REST, x.t);
                        let (fs, _) = faces(p, &derived, &open);
                        let leaf = fs.iter().filter(|f| f.leaf).min_by(|a, b| dist_point_segment(flat(c), a.a, a.b).total_cmp(&dist_point_segment(flat(c), b.a, b.b)));
                        let near = leaf.map(|f| format!("{} open {:.2} at {:.0} mm", &f.door[..8.min(f.door.len())], open(&f.door), dist_point_segment(flat(c), f.a, f.b))).unwrap_or_default();
                        println!("    {:5.1} s {:3.0}% ({:3.0}% leaves) at ({:6.0}, {:6.0}, {:5.0}) looking ({:5.2}, {:5.2}, {:5.2}) middle {:5.0}  nearest leaf {near}", x.t, x.share * 100.0, x.leaf * 100.0, c.x, c.y, c.z, d.x / vlen(d), d.y / vlen(d), d.z / vlen(d), x.middle);
                    }
                }
            }
        }
    }

    #[test]
    fn dissolves_point_at_the_held_frame() {
        let a = shot("a", ShotMove::Hold, vec![k(0.0, 0.0, 0.0)], 1.0, Ease::Linear);
        let mut b = shot("b", ShotMove::Hold, vec![k(0.0, 5000.0, 0.0)], 1.0, Ease::Linear);
        b.transition = Transition { kind: TransitionKind::Dissolve, seconds: 0.5 };
        let f = sample(&crate::templates::sample_bungalow(), &tour(vec![a, b]), 10.0);
        assert_eq!(f.len(), 20);
        let fx = f[10].fx.expect("dissolve");
        assert_eq!(fx.from_frame, Some(9));
        assert!(fx.mix < 0.01);
        assert!(f[15].fx.is_none());
    }
}
