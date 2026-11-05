//! Hidden line removal for elevations and sections: orthographic, from a
//! `Mesh` of planar faces.
//!
//! 1. Faces go into view space (s across, y up, t depth away from the
//!    viewer) and are clipped to `near <= t <= far`. Where a closed solid
//!    crosses the near plane, the cut is chained into loops: the poche.
//! 2. Vertices are welded at 0.05 mm, back to back face pairs (the shared
//!    end faces of two walls in line, two roof planes under a ridge) are
//!    dropped, and the feature edges are kept: creases over 20 degrees,
//!    silhouettes and boundaries. An edge where a surface simply carries on
//!    into a coplanar face of another solid (two walls in line, a wall over
//!    the slab edge under it) is not a feature and is not drawn.
//! 3. Every feature edge is tested against the faces in front of it,
//!    found through a uniform screen grid; what a face hides by more than
//!    0.5 mm of depth is hidden. Hidden parts are kept for checks.
//!
//! `merge_collinear` joins collinear pieces at 0.01 mm afterwards.

use std::collections::HashMap;

use crate::geom::*;

use super::mesh::{p3, Mesh, P3};

/// A face hides an edge only when it is this much nearer, mm.
pub const DEPTH_EPS: f64 = 0.5;
/// Vertices closer than this are one, mm.
const WELD_MM: f64 = 0.05;
/// Points this close to the near plane are on it, mm.
const ON_PLANE: f64 = 1e-3;
/// Creases sharper than this are drawn, degrees.
pub const CREASE_DEG: f64 = 20.0;
/// Points this close to a triangle's edge count as outside it for point
/// checks, and as inside it for hiding, mm. Hiding takes triangles closed:
/// a line behind a wall that lands exactly on the seam between two of its
/// faces (a back wall's sill behind the front wall's) is still behind it,
/// and the depth test alone keeps a face from hiding its own edges.
const EDGE_TOL: f64 = 1e-5;

/// An orthographic view: screen s along `right`, y along `up`, depth t
/// along `dir`, all from `origin`.
#[derive(Debug, Clone, Copy)]
pub struct ViewFrame {
    pub origin: P3,
    pub right: P3,
    pub up: P3,
    pub dir: P3,
}

impl ViewFrame {
    /// Looking horizontally along the plan direction `look` (unit), from
    /// `origin` (z 0): s runs to the viewer's right, y is the height.
    pub fn horizontal(origin: V, look: V) -> ViewFrame {
        let dir = p3(look.x, look.y, 0.0);
        let up = p3(0.0, 0.0, 1.0);
        ViewFrame { origin: p3(origin.x, origin.y, 0.0), right: dir.cross(up), up, dir }
    }

    /// (s, y, t) of a world point.
    pub fn to_view(&self, p: P3) -> P3 {
        let r = p - self.origin;
        p3(r.dot(self.right), r.dot(self.up), r.dot(self.dir))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeKind {
    Crease,
    Silhouette,
    Boundary,
    /// A line on a surface: sash, louvre, panel.
    Decal,
    /// Entourage outline, tested but hiding nothing.
    Figure,
}

/// An edge in view space: 2D ends (s, y) and their depths.
#[derive(Debug, Clone, Copy)]
pub struct Piece {
    pub a: V,
    pub b: V,
    pub ta: f64,
    pub tb: f64,
    /// Index of its solid in `Mesh::solids`; for figures, the figure index.
    pub solid: u32,
    pub kind: EdgeKind,
}

impl Piece {
    pub fn depth_at(&self, u: f64) -> f64 {
        self.ta + (self.tb - self.ta) * u
    }

    pub fn point(&self, u: f64) -> V {
        self.a + (self.b - self.a) * u
    }

    fn sub(&self, u0: f64, u1: f64) -> Piece {
        Piece {
            a: self.point(u0),
            b: self.point(u1),
            ta: self.depth_at(u0),
            tb: self.depth_at(u1),
            ..*self
        }
    }
}

/// One closed loop where a solid crosses the near plane, (s, y).
#[derive(Debug, Clone)]
pub struct CutLoop {
    pub pts: Vec<V>,
    pub solid: u32,
    /// False when the chain did not close (an open solid).
    pub closed: bool,
}

/// What to clip and whether to hide.
#[derive(Debug, Clone, Copy)]
pub struct Options {
    /// Keep depth >= near (a section cut); None keeps everything.
    pub near: Option<f64>,
    pub far: Option<f64>,
    /// False: every feature edge is visible (the first drop, and checks).
    pub remove_hidden: bool,
}

/// A triangle that hides: screen corners and the depth plane t = a s + b y + c.
#[derive(Debug, Clone, Copy)]
pub struct Tri {
    pub p: [V; 3],
    pub plane: [f64; 3],
    pub tmin: f64,
    pub solid: u32,
}

impl Tri {
    pub fn depth(&self, q: V) -> f64 {
        self.plane[0] * q.x + self.plane[1] * q.y + self.plane[2]
    }

    /// Strictly inside, by more than `tol`.
    pub fn contains(&self, q: V, tol: f64) -> bool {
        (0..3).all(|i| edge_dist(self.p[i], self.p[(i + 1) % 3], q) > tol)
    }
}

/// Signed distance of q from the line a to b, positive on the left.
fn edge_dist(a: V, b: V, q: V) -> f64 {
    let d = b - a;
    let l = d.len();
    if l < 1e-12 {
        return f64::NEG_INFINITY;
    }
    (d.x * (q.y - a.y) - d.y * (q.x - a.x)) / l
}

/// A uniform grid of triangles over the screen.
#[derive(Debug, Clone, Default)]
pub struct Grid {
    pub tris: Vec<Tri>,
    origin: V,
    cell: f64,
    nx: usize,
    ny: usize,
    cells: Vec<Vec<u32>>,
}

impl Grid {
    pub fn new(tris: Vec<Tri>) -> Grid {
        let mut b = Bounds::empty();
        let mut area = 0.0;
        for t in &tris {
            for p in t.p {
                b.add(p);
            }
            area += polygon_area(&t.p);
        }
        if tris.is_empty() || b.is_empty() {
            return Grid { tris, ..Grid::default() };
        }
        // About four triangles' worth of area per cell, at most 512 a side.
        let (w, h) = (b.width().max(1.0), b.height().max(1.0));
        let mut cell = (area / tris.len() as f64 * 4.0).sqrt().max(1.0);
        cell = cell.max(w / 512.0).max(h / 512.0);
        let nx = ((w / cell).ceil() as usize).max(1);
        let ny = ((h / cell).ceil() as usize).max(1);
        let mut g = Grid { tris: Vec::new(), origin: b.min, cell, nx, ny, cells: vec![Vec::new(); nx * ny] };
        for (i, t) in tris.iter().enumerate() {
            let tb = bounds_of(&t.p);
            let (x0, y0) = g.cell_of(tb.min);
            let (x1, y1) = g.cell_of(tb.max);
            for y in y0..=y1 {
                for x in x0..=x1 {
                    g.cells[y * nx + x].push(i as u32);
                }
            }
        }
        g.tris = tris;
        g
    }

    fn cell_of(&self, p: V) -> (usize, usize) {
        let x = ((p.x - self.origin.x) / self.cell).floor();
        let y = ((p.y - self.origin.y) / self.cell).floor();
        (
            (x.max(0.0) as usize).min(self.nx.saturating_sub(1)),
            (y.max(0.0) as usize).min(self.ny.saturating_sub(1)),
        )
    }

    /// Triangles whose cells the box touches, each once.
    pub fn around(&self, b: &Bounds, stamp: &mut Vec<u32>, mark: u32, out: &mut Vec<u32>) {
        out.clear();
        if self.cells.is_empty() || b.is_empty() {
            return;
        }
        if stamp.len() < self.tris.len() {
            stamp.resize(self.tris.len(), 0);
        }
        let (x0, y0) = self.cell_of(b.min);
        let (x1, y1) = self.cell_of(b.max);
        for y in y0..=y1 {
            for x in x0..=x1 {
                for &i in &self.cells[y * self.nx + x] {
                    if stamp[i as usize] != mark {
                        stamp[i as usize] = mark;
                        out.push(i);
                    }
                }
            }
        }
    }

    /// The solid whose face is front-most at q, None for open sky.
    pub fn solid_at(&self, q: V) -> Option<u32> {
        if self.cells.is_empty() {
            return None;
        }
        let (x, y) = self.cell_of(q);
        self.cells[y * self.nx + x]
            .iter()
            .map(|&i| &self.tris[i as usize])
            .filter(|t| t.contains(q, EDGE_TOL))
            .min_by(|a, b| a.depth(q).partial_cmp(&b.depth(q)).unwrap_or(std::cmp::Ordering::Equal))
            .map(|t| t.solid)
    }

    /// The nearest depth of any triangle over q, None for open sky.
    pub fn depth_at(&self, q: V) -> Option<f64> {
        if self.cells.is_empty() {
            return None;
        }
        let (x, y) = self.cell_of(q);
        if q.x < self.origin.x - 1e-9
            || q.y < self.origin.y - 1e-9
            || q.x > self.origin.x + self.cell * self.nx as f64 + 1e-9
            || q.y > self.origin.y + self.cell * self.ny as f64 + 1e-9
        {
            return None;
        }
        self.cells[y * self.nx + x]
            .iter()
            .map(|&i| &self.tris[i as usize])
            .filter(|t| t.contains(q, EDGE_TOL))
            .map(|t| t.depth(q))
            .fold(None, |m: Option<f64>, d| Some(m.map_or(d, |m| m.min(d))))
    }
}

/// The result of one view.
#[derive(Debug, Clone, Default)]
pub struct Hlr {
    pub visible: Vec<Piece>,
    pub hidden: Vec<Piece>,
    pub cuts: Vec<CutLoop>,
    /// The hiding triangles, for point checks (marks, profiles).
    pub grid: Grid,
    /// Feature edges before hiding.
    pub edges: usize,
}

struct VFace {
    pts: Vec<P3>,
    ids: Vec<u32>,
    solid: u32,
    normal: P3,
    /// Clipped by the near plane.
    cut: bool,
}

/// Which closed solids the near plane cuts: those with material on both
/// sides of it, and those with a face on the plane looking at the viewer
/// over another solid's face looking away (two walls that meet on the
/// plane: the plane runs through material). `view` is every face in view
/// space with points on the plane snapped onto it.
fn cut_solids(mesh: &Mesh, view: &[Vec<P3>], n0: f64) -> Vec<bool> {
    let n = mesh.solids.len();
    let (mut front, mut behind) = (vec![false; n], vec![false; n]);
    // Faces on the plane: (solid, polygon, looks at the viewer).
    let mut on: Vec<(usize, Vec<V>, bool)> = Vec::new();
    for (f, pts) in mesh.faces.iter().zip(view) {
        let s = f.solid as usize;
        if !mesh.solids[s].closed {
            continue;
        }
        for p in pts {
            front[s] |= p.z > n0;
            behind[s] |= p.z < n0;
        }
        if pts.len() >= 3 && pts.iter().all(|p| p.z == n0) {
            // Outward normal is minus the view space Newell vector (see below).
            let nz = -newell(pts).z;
            if nz.abs() > 1e-9 {
                on.push((s, pts.iter().map(|p| v(p.x, p.y)).collect(), nz < 0.0));
            }
        }
    }
    let mut cut: Vec<bool> = (0..n).map(|s| front[s] && behind[s]).collect();
    for (s, poly, toward) in &on {
        if !*toward || cut[*s] {
            continue;
        }
        let Some(t) = triangulate(poly).into_iter().next() else { continue };
        let probe = (poly[t[0]] + poly[t[1]] + poly[t[2]]) * (1.0 / 3.0);
        if on.iter().any(|(o, other, away)| !*away && o != s && point_in_polygon(probe, other)) {
            cut[*s] = true;
        }
    }
    cut
}

/// Run one view. `extra` are entourage outlines (kind `Figure`): tested
/// against the faces, hiding nothing.
///
/// The near plane follows one rule. Points within `ON_PLANE` of it are on
/// it. A face with no point beyond the plane is gone, except one lying on
/// it that looks at the viewer from a solid the plane does not cut (a wall
/// face exactly on the line) or from an open solid. Every face of a cut
/// solid that reaches the plane is clipped, and its edges on the plane are
/// the poche's outline; faces on the plane are never dropped as a back to
/// back pair before that.
pub fn hidden_lines(mesh: &Mesh, frame: &ViewFrame, opts: &Options, extra: &[Piece]) -> Hlr {
    let near = opts.near;
    let far = opts.far;

    // 1. View space and clipping.
    let view: Vec<Vec<P3>> = mesh
        .faces
        .iter()
        .map(|f| {
            f.pts
                .iter()
                .map(|p| {
                    let mut q = frame.to_view(*p);
                    if let Some(n0) = near {
                        if (q.z - n0).abs() <= ON_PLANE {
                            q.z = n0;
                        }
                    }
                    q
                })
                .collect()
        })
        .collect();
    let cut_solid = match near {
        Some(n0) => cut_solids(mesh, &view, n0),
        None => vec![false; mesh.solids.len()],
    };
    let mut faces: Vec<VFace> = Vec::with_capacity(mesh.faces.len());
    let mut cut_segs: Vec<(V, V, u32)> = Vec::new();
    for (f, pts) in mesh.faces.iter().zip(view) {
        let mut pts = pts;
        let mut cut = false;
        if let Some(n0) = near {
            let s = f.solid as usize;
            let closed = mesh.solids[s].closed;
            let ahead = pts.iter().any(|p| p.z > n0);
            let before = pts.iter().any(|p| p.z < n0);
            if !ahead {
                let looks_here = pts.len() >= 3 && -newell(&pts).z < -1e-9;
                let keep = !before && (!closed || (looks_here && !cut_solid[s]));
                if !keep {
                    continue;
                }
            } else if before || (cut_solid[s] && pts.iter().any(|p| p.z == n0)) {
                if before {
                    pts = clip_plane(&pts, n0, true);
                }
                cut = true;
                if closed {
                    for i in 0..pts.len() {
                        let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
                        if a.z == n0 && b.z == n0 {
                            cut_segs.push((v(a.x, a.y), v(b.x, b.y), f.solid));
                        }
                    }
                }
            }
        }
        if let Some(f0) = far {
            if pts.iter().any(|p| p.z > f0 + 1e-9) {
                if pts.iter().all(|p| p.z > f0 - 1e-9) {
                    continue;
                }
                pts = clip_plane(&pts, f0, false);
            }
        }
        if pts.len() < 3 {
            continue;
        }
        // The view basis (right, up, dir) is left handed, so a cross
        // product of view coordinates is the mirror of the real one: the
        // outward normal is minus the view space Newell vector.
        let normal = -newell(&pts);
        let Some(normal) = normal.unit() else { continue };
        faces.push(VFace { pts, ids: Vec::new(), solid: f.solid, normal, cut });
    }

    // 2. Weld, then drop faces that sit back to back with another.
    let mut weld = Weld::default();
    for f in faces.iter_mut() {
        let mut ids: Vec<u32> = f.pts.iter().map(|p| weld.id(*p)).collect();
        ids.dedup();
        if ids.len() > 1 && ids.first() == ids.last() {
            ids.pop();
        }
        f.ids = ids;
    }
    faces.retain(|f| f.ids.len() >= 3);
    {
        let mut by_set: HashMap<Vec<u32>, Vec<usize>> = HashMap::new();
        for (i, f) in faces.iter().enumerate() {
            let mut k = f.ids.clone();
            k.sort_unstable();
            by_set.entry(k).or_default().push(i);
        }
        let mut drop = vec![false; faces.len()];
        for list in by_set.values() {
            for (x, &i) in list.iter().enumerate() {
                for &j in &list[x + 1..] {
                    if !drop[i] && !drop[j] && faces[i].normal.dot(faces[j].normal) < -0.999 {
                        drop[i] = true;
                        drop[j] = true;
                    }
                }
            }
        }
        let mut k = 0;
        faces.retain(|_| {
            k += 1;
            !drop[k - 1]
        });
    }

    let closed = |f: &VFace| mesh.solids[f.solid as usize].closed;
    // Faces turned away from the viewer (t grows away, so a face whose
    // normal has positive t looks away).
    let back = |f: &VFace| closed(f) && f.normal.z > 1e-9;

    // 3. Edges with the faces on them.
    let mut adj: HashMap<(u32, u32), Vec<(usize, bool)>> = HashMap::new();
    for (fi, f) in faces.iter().enumerate() {
        let n = f.ids.len();
        for i in 0..n {
            let (a, b) = (f.ids[i], f.ids[(i + 1) % n]);
            if a == b {
                continue;
            }
            let key = (a.min(b), a.max(b));
            adj.entry(key).or_default().push((fi, a < b));
        }
    }
    let index = FaceIndex::new(&faces);
    let cos_crease = CREASE_DEG.to_radians().cos();

    let mut edges: Vec<Piece> = Vec::new();
    let mut keys: Vec<&(u32, u32)> = adj.keys().collect();
    keys.sort_unstable();
    for key in keys {
        let list = &adj[key];
        let (pa, pb) = (weld.pts[key.0 as usize], weld.pts[key.1 as usize]);
        // On the cut plane: the poche draws it.
        if let Some(n0) = near {
            if (pa.z - n0).abs() < 1e-6 && (pb.z - n0).abs() < 1e-6 && list.iter().any(|(fi, _)| faces[*fi].cut) {
                continue;
            }
        }
        if let Some(f0) = far {
            if (pa.z - f0).abs() < 1e-6 && (pb.z - f0).abs() < 1e-6 {
                continue;
            }
        }
        if list.iter().all(|(fi, _)| back(&faces[*fi])) {
            continue;
        }
        let Some(e_dir) = (pb - pa).unit() else { continue };
        let kind = match list.len() {
            1 => EdgeKind::Boundary,
            2 => {
                let (f0, f1) = (&faces[list[0].0], &faces[list[1].0]);
                if f0.normal.dot(f1.normal) < cos_crease {
                    EdgeKind::Crease
                } else if back(f0) != back(f1) {
                    EdgeKind::Silhouette
                } else {
                    continue;
                }
            }
            _ => EdgeKind::Crease,
        };
        // The direction in each face's plane, across the edge, away from
        // the face: d x n for the face's own direction d along the edge,
        // which in the left handed view basis is n x d by components.
        let away: Vec<P3> = list
            .iter()
            .map(|(fi, fwd)| {
                let d = if *fwd { e_dir } else { -e_dir };
                faces[*fi].normal.cross(d)
            })
            .collect();
        // Where a surface carries on across the edge, with everything else
        // at the edge behind it, there is no line: those stretches go.
        let mut seams: Vec<(f64, f64)> = Vec::new();
        for (k, (fi, _)) in list.iter().enumerate() {
            let n = faces[*fi].normal;
            let all_behind = list.iter().enumerate().all(|(j, (fj, _))| {
                if j == k {
                    return true;
                }
                let nj = faces[*fj].normal;
                if nj.dot(n) > 0.9999 {
                    return true;
                }
                // Into face j from the edge is against its `away`.
                n.dot(-away[j]) < -0.02
            });
            if all_behind {
                let off = away[k] * 1.0;
                index.continuation(&faces, *fi, pa + off, pb + off, &mut seams);
            }
        }
        let solid = faces[list[0].0].solid;
        let (a, b) = (v(pa.x, pa.y), v(pb.x, pb.y));
        if (b - a).len() < 0.01 {
            continue;
        }
        let whole = Piece { a, b, ta: pa.z, tb: pb.z, solid, kind };
        if seams.is_empty() {
            edges.push(whole);
        } else {
            let mut gone = Vec::new();
            split(&whole, &mut seams, &mut edges, &mut gone);
        }
    }

    // Lines on surfaces and entourage, clipped like the faces.
    for d in &mesh.decals {
        let (a, b) = (frame.to_view(d.a), frame.to_view(d.b));
        if let Some((a, b)) = clip_seg(a, b, near, far) {
            if (v(b.x, b.y) - v(a.x, a.y)).len() >= 0.01 {
                edges.push(Piece { a: v(a.x, a.y), b: v(b.x, b.y), ta: a.z, tb: b.z, solid: d.solid, kind: EdgeKind::Decal });
            }
        }
    }
    for e in extra {
        let (a, b) = (p3(e.a.x, e.a.y, e.ta), p3(e.b.x, e.b.y, e.tb));
        if let Some((a, b)) = clip_seg(a, b, near, far) {
            edges.push(Piece { a: v(a.x, a.y), b: v(b.x, b.y), ta: a.z, tb: b.z, ..*e });
        }
    }

    // 4. The hiding triangles: faces toward the viewer, open faces either
    // way, and the cut caps at the near plane.
    let cuts = chain_loops(&cut_segs);
    let mut tris: Vec<Tri> = Vec::new();
    for f in &faces {
        if back(f) || f.normal.z.abs() < 1e-9 {
            continue;
        }
        let flat: Vec<V> = f.pts.iter().map(|p| v(p.x, p.y)).collect();
        for t in triangulate(&flat) {
            if let Some(tri) = make_tri([f.pts[t[0]], f.pts[t[1]], f.pts[t[2]]], f.solid) {
                tris.push(tri);
            }
        }
    }
    if let Some(n0) = near {
        for c in cuts.iter().filter(|c| c.closed) {
            for t in triangulate(&c.pts) {
                let q = |i: usize| p3(c.pts[i].x, c.pts[i].y, n0);
                if let Some(tri) = make_tri([q(t[0]), q(t[1]), q(t[2])], c.solid) {
                    tris.push(tri);
                }
            }
        }
    }
    let grid = Grid::new(tris);

    // 5. Hide.
    let count = edges.len();
    let mut visible = Vec::new();
    let mut hidden = Vec::new();
    if !opts.remove_hidden {
        visible = edges;
    } else {
        let mut stamp: Vec<u32> = Vec::new();
        let mut cand: Vec<u32> = Vec::new();
        let mut spans: Vec<(f64, f64)> = Vec::new();
        for (k, e) in edges.iter().enumerate() {
            let mut b = Bounds::empty();
            b.add(e.a);
            b.add(e.b);
            grid.around(&b, &mut stamp, k as u32 + 1, &mut cand);
            spans.clear();
            let tmax = e.ta.max(e.tb);
            for &ti in &cand {
                let t = &grid.tris[ti as usize];
                if t.tmin > tmax - DEPTH_EPS {
                    continue;
                }
                if let Some(span) = hidden_span(e, t) {
                    spans.push(span);
                }
            }
            split(e, &mut spans, &mut visible, &mut hidden);
        }
    }
    Hlr { visible, hidden, cuts, grid, edges: count }
}

fn make_tri(p: [P3; 3], solid: u32) -> Option<Tri> {
    let mut q = [v(p[0].x, p[0].y), v(p[1].x, p[1].y), v(p[2].x, p[2].y)];
    let mut z = [p[0].z, p[1].z, p[2].z];
    let area2 = (q[1] - q[0]).x * (q[2] - q[0]).y - (q[1] - q[0]).y * (q[2] - q[0]).x;
    if area2.abs() < 1e-6 {
        return None;
    }
    if area2 < 0.0 {
        q.swap(1, 2);
        z.swap(1, 2);
    }
    // Solve t = a s + b y + c through the three corners.
    let (d1, d2) = (q[1] - q[0], q[2] - q[0]);
    let det = d1.x * d2.y - d1.y * d2.x;
    let (z1, z2) = (z[1] - z[0], z[2] - z[0]);
    let a = (z1 * d2.y - z2 * d1.y) / det;
    let b = (d1.x * z2 - d2.x * z1) / det;
    let c = z[0] - a * q[0].x - b * q[0].y;
    if !(a.is_finite() && b.is_finite() && c.is_finite()) {
        return None;
    }
    Some(Tri { p: q, plane: [a, b, c], tmin: z[0].min(z[1]).min(z[2]), solid })
}

/// The part of the edge (as parameters) the triangle hides, if any.
fn hidden_span(e: &Piece, t: &Tri) -> Option<(f64, f64)> {
    let (mut u0, mut u1) = (0.0_f64, 1.0_f64);
    for i in 0..3 {
        let (p, q) = (t.p[i], t.p[(i + 1) % 3]);
        let g0 = edge_dist(p, q, e.a) + EDGE_TOL;
        let g1 = edge_dist(p, q, e.b) + EDGE_TOL;
        if !limit(g0, g1, &mut u0, &mut u1) {
            return None;
        }
    }
    // Behind by more than the depth tolerance.
    let f0 = e.ta - t.depth(e.a) - DEPTH_EPS;
    let f1 = e.tb - t.depth(e.b) - DEPTH_EPS;
    if !limit(f0, f1, &mut u0, &mut u1) {
        return None;
    }
    (u1 > u0).then_some((u0, u1))
}

/// Narrow `u0..u1` to where the linear g (g0 at 0, g1 at 1) is positive.
/// False when nothing is left.
fn limit(g0: f64, g1: f64, u0: &mut f64, u1: &mut f64) -> bool {
    if g0 > 0.0 && g1 > 0.0 {
        return true;
    }
    if g0 <= 0.0 && g1 <= 0.0 {
        return false;
    }
    let u = g0 / (g0 - g1);
    if g0 > 0.0 {
        *u1 = u1.min(u);
    } else {
        *u0 = u0.max(u);
    }
    *u1 > *u0
}

/// Cut the edge at the hidden spans into visible and hidden pieces. Gaps
/// and pieces under 0.02 mm are closed up.
fn split(e: &Piece, spans: &mut Vec<(f64, f64)>, visible: &mut Vec<Piece>, hidden: &mut Vec<Piece>) {
    let len = (e.b - e.a).len().max(1e-9);
    let tol = 0.02 / len;
    spans.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    let mut merged: Vec<(f64, f64)> = Vec::new();
    for &(a, b) in spans.iter() {
        match merged.last_mut() {
            Some(last) if a <= last.1 + tol => last.1 = last.1.max(b),
            _ => merged.push((a, b)),
        }
    }
    let mut at = 0.0;
    for (a, b) in merged {
        let a = if a < tol { 0.0 } else { a };
        let b = if b > 1.0 - tol { 1.0 } else { b };
        if a - at > tol {
            visible.push(e.sub(at, a));
        }
        if b - a > tol {
            hidden.push(e.sub(a, b));
        }
        at = at.max(b);
    }
    if 1.0 - at > tol {
        visible.push(e.sub(at, 1.0));
    }
}

/// Keep the part of a 3D segment with near <= t <= far.
fn clip_seg(a: P3, b: P3, near: Option<f64>, far: Option<f64>) -> Option<(P3, P3)> {
    let (mut u0, mut u1) = (0.0, 1.0);
    if let Some(n0) = near {
        if !limit(a.z - n0 + 1e-12, b.z - n0 + 1e-12, &mut u0, &mut u1) {
            return None;
        }
    }
    if let Some(f0) = far {
        if !limit(f0 - a.z + 1e-12, f0 - b.z + 1e-12, &mut u0, &mut u1) {
            return None;
        }
    }
    let at = |u: f64| a + (b - a) * u;
    Some((at(u0), at(u1)))
}

/// Sutherland-Hodgman on the plane t = `c`, keeping t >= c (`keep_greater`)
/// or t <= c. Points on the plane are snapped onto it.
fn clip_plane(poly: &[P3], c: f64, keep_greater: bool) -> Vec<P3> {
    let side = |p: &P3| if keep_greater { p.z - c } else { c - p.z };
    let mut out = Vec::with_capacity(poly.len() + 2);
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
        let (da, db) = (side(&a), side(&b));
        if da >= 0.0 {
            out.push(a);
        }
        // A point on the plane is kept as it is, not met again.
        if (da > 0.0 && db < 0.0) || (da < 0.0 && db > 0.0) {
            let u = da / (da - db);
            let mut q = a + (b - a) * u;
            q.z = c;
            out.push(q);
        }
    }
    out
}

/// Newell normal of a polygon (its area vector).
pub fn newell(pts: &[P3]) -> P3 {
    let mut n = p3(0.0, 0.0, 0.0);
    for i in 0..pts.len() {
        let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
        n.x += (a.y - b.y) * (a.z + b.z);
        n.y += (a.z - b.z) * (a.x + b.x);
        n.z += (a.x - b.x) * (a.y + b.y);
    }
    n
}

#[derive(Default)]
struct Weld {
    pts: Vec<P3>,
    map: HashMap<(i64, i64, i64), Vec<u32>>,
}

impl Weld {
    fn id(&mut self, p: P3) -> u32 {
        let k = |x: f64| (x / WELD_MM).floor() as i64;
        let (kx, ky, kz) = (k(p.x), k(p.y), k(p.z));
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    if let Some(list) = self.map.get(&(kx + dx, ky + dy, kz + dz)) {
                        for &i in list {
                            if (self.pts[i as usize] - p).len() <= WELD_MM {
                                return i;
                            }
                        }
                    }
                }
            }
        }
        let i = self.pts.len() as u32;
        self.pts.push(p);
        self.map.entry((kx, ky, kz)).or_default().push(i);
        i
    }
}

/// Faces by space, for "does this surface carry on past the edge".
struct FaceIndex {
    cell: f64,
    map: HashMap<(i64, i64, i64), Vec<u32>>,
}

impl FaceIndex {
    fn new(faces: &[VFace]) -> FaceIndex {
        let cell = 1000.0;
        let mut map: HashMap<(i64, i64, i64), Vec<u32>> = HashMap::new();
        let k = |x: f64| (x / cell).floor() as i64;
        for (i, f) in faces.iter().enumerate() {
            let mut lo = p3(f64::INFINITY, f64::INFINITY, f64::INFINITY);
            let mut hi = p3(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
            for p in &f.pts {
                lo = p3(lo.x.min(p.x), lo.y.min(p.y), lo.z.min(p.z));
                hi = p3(hi.x.max(p.x), hi.y.max(p.y), hi.z.max(p.z));
            }
            for x in k(lo.x - 1.0)..=k(hi.x + 1.0) {
                for y in k(lo.y - 1.0)..=k(hi.y + 1.0) {
                    for z in k(lo.z - 1.0)..=k(hi.z + 1.0) {
                        map.entry((x, y, z)).or_default().push(i as u32);
                    }
                }
            }
        }
        FaceIndex { cell, map }
    }

    /// The stretches (as parameters of a to b) of the segment a to b that
    /// lie on a face other than `fi`, coplanar with it and facing the same
    /// way: where `fi`'s surface carries on past its edge.
    fn continuation(&self, faces: &[VFace], fi: usize, a: P3, b: P3, out: &mut Vec<(f64, f64)>) {
        let k = |x: f64| (x / self.cell).floor() as i64;
        let f = &faces[fi];
        let mut seen: Vec<u32> = Vec::new();
        let (lo, hi) = (p3(a.x.min(b.x), a.y.min(b.y), a.z.min(b.z)), p3(a.x.max(b.x), a.y.max(b.y), a.z.max(b.z)));
        for x in k(lo.x)..=k(hi.x) {
            for y in k(lo.y)..=k(hi.y) {
                for z in k(lo.z)..=k(hi.z) {
                    let Some(list) = self.map.get(&(x, y, z)) else { continue };
                    for &gi in list {
                        if gi as usize == fi || seen.contains(&gi) {
                            continue;
                        }
                        seen.push(gi);
                        let g = &faces[gi as usize];
                        if g.normal.dot(f.normal) <= 0.9999
                            || (a - g.pts[0]).dot(g.normal).abs() >= 0.05
                            || (b - g.pts[0]).dot(g.normal).abs() >= 0.05
                        {
                            continue;
                        }
                        let (pick, poly) = flat(g);
                        let (a2, b2) = (pick(a), pick(b));
                        let d = b2 - a2;
                        let dd = d.dot(d);
                        if dd < 1e-12 {
                            continue;
                        }
                        for (p, q) in clip_segment(a2, b2, &poly) {
                            let (t0, t1) = ((p - a2).dot(d) / dd, (q - a2).dot(d) / dd);
                            out.push((t0.min(t1), t0.max(t1)));
                        }
                    }
                }
            }
        }
    }
}

/// A face's polygon in the plane of its two dominant axes, and the map
/// into it.
fn flat(f: &VFace) -> (impl Fn(P3) -> V, Vec<V>) {
    let n = f.normal;
    let (ax, ay) = if n.x.abs() >= n.y.abs() && n.x.abs() >= n.z.abs() {
        (1, 2)
    } else if n.y.abs() >= n.z.abs() {
        (0, 2)
    } else {
        (0, 1)
    };
    let pick = move |p: P3| {
        let c = [p.x, p.y, p.z];
        v(c[ax], c[ay])
    };
    let poly: Vec<V> = f.pts.iter().map(|p| pick(*p)).collect();
    (pick, poly)
}

/// Chain cut segments of each solid into loops (ends welded at 0.05 mm).
fn chain_loops(segs: &[(V, V, u32)]) -> Vec<CutLoop> {
    let mut by_solid: HashMap<u32, Vec<(u32, u32)>> = HashMap::new();
    let mut weld = Weld::default();
    for (a, b, s) in segs {
        let (ia, ib) = (weld.id(p3(a.x, a.y, 0.0)), weld.id(p3(b.x, b.y, 0.0)));
        if ia != ib {
            by_solid.entry(*s).or_default().push((ia, ib));
        }
    }
    let at = |i: u32| {
        let p = weld.pts[i as usize];
        v(p.x, p.y)
    };
    let mut solids: Vec<u32> = by_solid.keys().copied().collect();
    solids.sort_unstable();
    let mut out = Vec::new();
    for s in solids {
        let list = &by_solid[&s];
        let mut ends: HashMap<u32, Vec<usize>> = HashMap::new();
        for (i, (a, b)) in list.iter().enumerate() {
            ends.entry(*a).or_default().push(i);
            ends.entry(*b).or_default().push(i);
        }
        let mut used = vec![false; list.len()];
        for start in 0..list.len() {
            if used[start] {
                continue;
            }
            used[start] = true;
            let mut ids = vec![list[start].0, list[start].1];
            let mut closed = false;
            loop {
                let tail = *ids.last().unwrap();
                if tail == ids[0] && ids.len() > 2 {
                    ids.pop();
                    closed = true;
                    break;
                }
                let next = ends.get(&tail).and_then(|c| c.iter().copied().find(|&j| !used[j]));
                let Some(j) = next else { break };
                used[j] = true;
                let (a, b) = list[j];
                ids.push(if a == tail { b } else { a });
            }
            let pts = clean_polygon(&ids.iter().map(|i| at(*i)).collect::<Vec<V>>());
            if pts.len() >= 2 {
                let closed = closed && pts.len() >= 3 && polygon_area(&pts) > 1.0;
                out.push(CutLoop { closed, pts, solid: s });
            }
        }
    }
    out
}

/// Join collinear pieces that overlap or touch within 0.01 mm. Each piece
/// carries a weight; where pieces overlap the heaviest wins. Returns the
/// joined pieces with their weight.
pub fn merge_collinear<T: Copy + Ord>(pieces: &[(V, V, T)]) -> Vec<(V, V, T)> {
    const TOL: f64 = 0.01;
    // Line key: direction folded into [0, 180) degrees and the offset.
    struct Item<T> {
        ang: f64,
        off: f64,
        u: V,
        t0: f64,
        t1: f64,
        w: T,
    }
    let mut items: Vec<Item<T>> = Vec::with_capacity(pieces.len());
    for &(a, b, w) in pieces {
        let Some(mut u) = (b - a).unit() else { continue };
        if u.x < -1e-12 || (u.x.abs() <= 1e-12 && u.y < 0.0) {
            u = -u;
        }
        // Bucketed so pieces of one line sort together.
        let ang = (u.y.atan2(u.x) / 1e-7).round() * 1e-7;
        let off = u.x * a.y - u.y * a.x;
        let (ta, tb) = (a.dot(u), b.dot(u));
        items.push(Item { ang, off, u, t0: ta.min(tb), t1: ta.max(tb), w });
    }
    items.sort_by(|a, b| a.ang.partial_cmp(&b.ang).unwrap_or(std::cmp::Ordering::Equal).then(a.off.partial_cmp(&b.off).unwrap_or(std::cmp::Ordering::Equal)));
    let mut out = Vec::new();
    let mut i = 0;
    while i < items.len() {
        // A run of items on one line: the same angle bucket, offsets
        // within 0.01 mm of each other.
        let mut j = i + 1;
        while j < items.len() && items[j].ang == items[i].ang && (items[j].off - items[j - 1].off).abs() <= TOL {
            j += 1;
        }
        let group = &items[i..j];
        let u = group[0].u;
        let off = group[0].off;
        let base = v(-u.y, u.x) * off;
        // Sweep the intervals: at each stretch the heaviest weight covers it.
        let mut cuts: Vec<f64> = group.iter().flat_map(|g| [g.t0, g.t1]).collect();
        cuts.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        cuts.dedup_by(|a, b| (*a - *b).abs() <= 1e-9);
        let mut run: Option<(f64, f64, T)> = None;
        for k in 0..cuts.len().saturating_sub(1) {
            let (s0, s1) = (cuts[k], cuts[k + 1]);
            let m = (s0 + s1) / 2.0;
            let w = group.iter().filter(|g| g.t0 - TOL <= m && m <= g.t1 + TOL).map(|g| g.w).max();
            match (run.as_mut(), w) {
                (Some(r), Some(w)) if r.2 == w && s0 <= r.1 + TOL => r.1 = s1,
                (_, w) => {
                    if let Some(r) = run.take() {
                        out.push((base + u * r.0, base + u * r.1, r.2));
                    }
                    run = w.map(|w| (s0, s1, w));
                }
            }
        }
        if let Some(r) = run.take() {
            out.push((base + u * r.0, base + u * r.1, r.2));
        }
        i = j;
    }
    out.retain(|(a, b, _)| (*b - *a).len() > TOL);
    out
}
