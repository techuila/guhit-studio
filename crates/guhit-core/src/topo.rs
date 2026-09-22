//! Wall topology: planar graph of wall centerlines per level, faces (rooms),
//! footprints, exterior flags and mitred wall outlines.
//!
//! Rules:
//! - Wall ends within 1 mm of each other are one joint.
//! - A wall end within 1 mm of another wall's body is a T junction.
//! - Walls that cross are split at the crossing.
//! - Overlapping collinear walls share graph edges.
//!
//! Everything is recomputed from scratch and is deterministic: only vectors
//! and ordered maps, never hash iteration order.

use std::collections::BTreeMap;

use guhit_model::*;

use crate::geom::*;

/// Faces with a net area below this are not rooms (0.01 m2).
const MIN_ROOM_AREA_MM2: f64 = 10_000.0;

/// Wall outline mitres longer than this many thicknesses fall back to a
/// square end. Same cut-off as the footprint bevel in `geom::offset_corner`
/// (4 half thicknesses), which is a corner sharper than about 29 degrees.
const OUTLINE_MITRE_LIMIT: f64 = 2.0;

#[derive(Debug, Clone)]
pub struct Edge {
    pub a: usize,
    pub b: usize,
    pub wall_ids: Vec<Id>,
    /// Largest thickness of the walls on this edge.
    pub thickness: f64,
    /// True for edges that bound no closed face (dangling walls, bridges).
    pub bridge: bool,
    /// True when one side of the edge is outside the building.
    pub exterior: bool,
}

#[derive(Debug, Clone)]
pub struct BoundaryEdge {
    pub from: Point,
    pub to: Point,
    pub wall_ids: Vec<Id>,
    pub exterior: bool,
}

/// One closed room face.
#[derive(Debug, Clone)]
pub struct Face {
    pub level_id: Id,
    /// Counter-clockwise on wall centerlines, collinear vertices removed.
    pub centerline: Vec<Point>,
    /// Counter-clockwise on the inner wall faces.
    pub net: Vec<Point>,
    /// Clockwise outlines of wall islands standing inside the room.
    pub holes: Vec<Vec<Point>>,
    pub area_mm2: f64,
    pub centerline_area_mm2: f64,
    pub perimeter_mm: f64,
    pub label: Point,
    /// Walls on the outer ring, in walk order, no repeats.
    pub wall_ids: Vec<Id>,
    pub boundary: Vec<BoundaryEdge>,
}

impl Face {
    /// True when `p` belongs to this face (by centerlines).
    pub fn contains(&self, p: Point) -> bool {
        point_in_polygon(p, &self.centerline)
    }

    /// True when `p` is clear of the walls, inside the net polygon and
    /// outside every island.
    pub fn contains_net(&self, p: Point) -> bool {
        strictly_inside(p, &self.net, 0.5) && !self.holes.iter().any(|h| point_in_polygon(p, h))
    }
}

#[derive(Debug, Clone)]
pub struct WallTopo {
    pub wall_id: Id,
    pub length_mm: f64,
    pub outline: Vec<Point>,
    pub exterior: bool,
    pub joined_at_start: Vec<Id>,
    pub joined_at_end: Vec<Id>,
    /// Distances from the wall start where other walls meet its body.
    pub junctions_mm: Vec<(f64, Id)>,
}

#[derive(Debug, Clone)]
pub struct LevelTopo {
    pub level_id: Id,
    pub edges: Vec<Edge>,
    pub faces: Vec<Face>,
    /// Counter-clockwise outer boundaries on the outer wall faces, largest first.
    pub footprints: Vec<(Vec<Point>, f64)>,
}

#[derive(Debug, Clone, Default)]
pub struct Analysis {
    pub levels: Vec<LevelTopo>,
    /// In project element order.
    pub walls: Vec<WallTopo>,
}

impl Analysis {
    pub fn level(&self, level_id: &str) -> Option<&LevelTopo> {
        self.levels.iter().find(|l| l.level_id == level_id)
    }

    pub fn wall(&self, wall_id: &str) -> Option<&WallTopo> {
        self.walls.iter().find(|w| w.wall_id == wall_id)
    }

    /// The smallest face on `level_id` that contains `p`.
    pub fn face_at(&self, level_id: &str, p: Point) -> Option<usize> {
        let level = self.level(level_id)?;
        let mut best: Option<(usize, f64)> = None;
        for (i, f) in level.faces.iter().enumerate() {
            if f.contains(p) && best.map(|(_, a)| f.centerline_area_mm2 < a).unwrap_or(true) {
                best = Some((i, f.centerline_area_mm2));
            }
        }
        best.map(|(i, _)| i)
    }
}

#[derive(Clone, Copy, PartialEq)]
enum EndKind {
    Start,
    End,
    Through,
}

/// A wall leaving a node in direction `dir`.
#[derive(Clone)]
struct Incidence {
    wall: usize,
    dir: Point,
    thickness: f64,
    kind: EndKind,
}

struct Graph {
    nodes: Vec<Point>,
}

impl Graph {
    fn node_at(&mut self, p: Point) -> usize {
        for (i, n) in self.nodes.iter().enumerate() {
            if dist(*n, p) <= JOIN_EPS {
                return i;
            }
        }
        self.nodes.push(p);
        self.nodes.len() - 1
    }
}

fn usable(w: &Wall) -> bool {
    is_finite(w.start)
        && is_finite(w.end)
        && w.thickness_mm.is_finite()
        && dist(w.start, w.end) > JOIN_EPS
}

/// Analyze every level of the project.
pub fn analyze(project: &Project) -> Analysis {
    let mut analysis = Analysis::default();
    let mut wall_topos: BTreeMap<Id, WallTopo> = BTreeMap::new();
    let mut level_ids: Vec<Id> = project.levels.iter().map(|l| l.id.clone()).collect();
    for el in &project.elements {
        if let Element::Wall(w) = el {
            if !level_ids.contains(&w.level_id) {
                level_ids.push(w.level_id.clone());
            }
        }
    }
    for level_id in level_ids {
        let walls: Vec<&Wall> = project
            .elements
            .iter()
            .filter_map(|e| match e {
                Element::Wall(w) if w.level_id == level_id && usable(w) => Some(w),
                _ => None,
            })
            .collect();
        let (topo, wt) = analyze_level(&level_id, &walls);
        for w in wt {
            wall_topos.insert(w.wall_id.clone(), w);
        }
        analysis.levels.push(topo);
    }
    for el in &project.elements {
        if let Element::Wall(w) = el {
            let topo = wall_topos.remove(&w.id).unwrap_or_else(|| WallTopo {
                wall_id: w.id.clone(),
                length_mm: if is_finite(w.start) && is_finite(w.end) {
                    dist(w.start, w.end)
                } else {
                    0.0
                },
                outline: vec![],
                exterior: false,
                joined_at_start: vec![],
                joined_at_end: vec![],
                junctions_mm: vec![],
            });
            analysis.walls.push(topo);
        }
    }
    analysis
}

/// Half-edge `edge * 2 + dir`; dir 0 runs a to b.
fn he_from(edges: &[Edge], he: usize) -> usize {
    let e = &edges[he / 2];
    if he.is_multiple_of(2) {
        e.a
    } else {
        e.b
    }
}

fn he_to(edges: &[Edge], he: usize) -> usize {
    let e = &edges[he / 2];
    if he.is_multiple_of(2) {
        e.b
    } else {
        e.a
    }
}

/// Trace all faces of the graph made of the `active` edges. Each face is a
/// list of half-edges with the face on the left. Bounded faces come out
/// counter-clockwise, outer boundaries clockwise.
fn trace_faces(nodes: &[Point], edges: &[Edge], active: &[bool]) -> Vec<Vec<usize>> {
    let mut out_of: Vec<Vec<usize>> = vec![vec![]; nodes.len()];
    for (i, e) in edges.iter().enumerate() {
        if active[i] {
            out_of[e.a].push(i * 2);
            out_of[e.b].push(i * 2 + 1);
        }
    }
    for (n, list) in out_of.iter_mut().enumerate() {
        list.sort_by(|x, y| {
            let dx = sub(nodes[he_to(edges, *x)], nodes[n]);
            let dy = sub(nodes[he_to(edges, *y)], nodes[n]);
            dx.y.atan2(dx.x).total_cmp(&dy.y.atan2(dy.x)).then(x.cmp(y))
        });
    }
    let mut visited = vec![false; edges.len() * 2];
    let mut faces = vec![];
    for start in 0..edges.len() * 2 {
        if !active[start / 2] || visited[start] {
            continue;
        }
        let mut walk = vec![];
        let mut he = start;
        loop {
            visited[he] = true;
            walk.push(he);
            let v = he_to(edges, he);
            let twin = he ^ 1;
            let list = &out_of[v];
            let pos = list.iter().position(|h| *h == twin).unwrap_or(0);
            he = list[(pos + list.len() - 1) % list.len()];
            if he == start || walk.len() > edges.len() * 2 + 2 {
                break;
            }
        }
        faces.push(walk);
    }
    faces
}

fn walk_points(nodes: &[Point], edges: &[Edge], walk: &[usize]) -> Vec<Point> {
    walk.iter().map(|he| nodes[he_from(edges, *he)]).collect()
}

fn find(parent: &mut [usize], i: usize) -> usize {
    let mut r = i;
    while parent[r] != r {
        r = parent[r];
    }
    let mut c = i;
    while parent[c] != r {
        let next = parent[c];
        parent[c] = r;
        c = next;
    }
    r
}

struct RawFace {
    comp: usize,
    raw: Vec<Point>,
    area: f64,
    walk: Vec<usize>,
}

fn analyze_level(level_id: &str, walls: &[&Wall]) -> (LevelTopo, Vec<WallTopo>) {
    let mut g = Graph { nodes: vec![] };
    let ends: Vec<(usize, usize)> = walls
        .iter()
        .map(|w| (g.node_at(w.start), g.node_at(w.end)))
        .collect();

    // Crossings and T junctions.
    for i in 0..walls.len() {
        for j in (i + 1)..walls.len() {
            let (a, b) = (walls[i], walls[j]);
            let (Some(da), Some(db)) = (unit(sub(a.end, a.start)), unit(sub(b.end, b.start)))
            else {
                continue;
            };
            if cross(da, db).abs() < 1e-9 {
                continue;
            }
            if let Some(x) = line_intersect(a.start, da, b.start, db) {
                if dist_point_segment(x, a.start, a.end) <= JOIN_EPS
                    && dist_point_segment(x, b.start, b.end) <= JOIN_EPS
                {
                    g.node_at(x);
                }
            }
        }
    }

    // Split every wall at all nodes on its body.
    let mut edges: Vec<Edge> = vec![];
    let mut edge_index: BTreeMap<(usize, usize), usize> = BTreeMap::new();
    let mut wall_nodes: Vec<Vec<(f64, usize)>> = vec![];
    for (wi, w) in walls.iter().enumerate() {
        let mut on: Vec<(f64, usize)> = vec![];
        for (ni, n) in g.nodes.iter().enumerate() {
            if ni == ends[wi].0 || ni == ends[wi].1 {
                continue;
            }
            let (along, d) = project_on_segment(*n, w.start, w.end);
            if d <= JOIN_EPS {
                on.push((along, ni));
            }
        }
        on.push((0.0, ends[wi].0));
        on.push((dist(w.start, w.end), ends[wi].1));
        on.sort_by(|x, y| x.0.total_cmp(&y.0).then(x.1.cmp(&y.1)));
        // The wall's own ends must stay first and last.
        on.retain(|(_, n)| *n != ends[wi].0 && *n != ends[wi].1);
        on.insert(0, (0.0, ends[wi].0));
        on.push((dist(w.start, w.end), ends[wi].1));
        for pair in on.windows(2) {
            let (a, b) = (pair[0].1, pair[1].1);
            if a == b {
                continue;
            }
            let key = (a.min(b), a.max(b));
            let idx = *edge_index.entry(key).or_insert_with(|| {
                edges.push(Edge {
                    a: key.0,
                    b: key.1,
                    wall_ids: vec![],
                    thickness: 0.0,
                    bridge: false,
                    exterior: false,
                });
                edges.len() - 1
            });
            if !edges[idx].wall_ids.contains(&w.id) {
                edges[idx].wall_ids.push(w.id.clone());
            }
            edges[idx].thickness = edges[idx].thickness.max(w.thickness_mm);
        }
        wall_nodes.push(on);
    }

    // Bridges: both sides of the edge see the same face.
    let all_active = vec![true; edges.len()];
    let first = trace_faces(&g.nodes, &edges, &all_active);
    let mut face_of = vec![usize::MAX; edges.len() * 2];
    for (fi, walk) in first.iter().enumerate() {
        for he in walk {
            face_of[*he] = fi;
        }
    }
    for (i, e) in edges.iter_mut().enumerate() {
        e.bridge = face_of[i * 2] == face_of[i * 2 + 1];
    }
    let active: Vec<bool> = edges.iter().map(|e| !e.bridge).collect();

    // Components of the bridge-free graph.
    let mut parent: Vec<usize> = (0..g.nodes.len()).collect();
    for (i, e) in edges.iter().enumerate() {
        if active[i] {
            let (ra, rb) = (find(&mut parent, e.a), find(&mut parent, e.b));
            if ra != rb {
                parent[ra.max(rb)] = ra.min(rb);
            }
        }
    }

    let walks = trace_faces(&g.nodes, &edges, &active);
    let raw_faces: Vec<RawFace> = walks
        .into_iter()
        .map(|walk| {
            let raw = walk_points(&g.nodes, &edges, &walk);
            RawFace {
                comp: find(&mut parent, he_from(&edges, walk[0])),
                area: signed_area(&raw),
                raw,
                walk,
            }
        })
        .collect();

    // The outer boundary of a component is its face with the smallest area.
    let mut outer_of: BTreeMap<usize, usize> = BTreeMap::new();
    for (i, f) in raw_faces.iter().enumerate() {
        match outer_of.get(&f.comp) {
            Some(j) if raw_faces[*j].area <= f.area => {}
            _ => {
                outer_of.insert(f.comp, i);
            }
        }
    }
    let is_outer = |i: usize| outer_of.get(&raw_faces[i].comp) == Some(&i);

    // Parent face of every component: the smallest bounded face of another
    // component that contains it.
    let mut comp_parent: BTreeMap<usize, Option<usize>> = BTreeMap::new();
    for (comp, outer) in &outer_of {
        let rep = raw_faces[*outer].raw[0];
        let mut best: Option<(usize, f64)> = None;
        for (i, f) in raw_faces.iter().enumerate() {
            if f.comp == *comp || is_outer(i) || f.area <= 0.0 {
                continue;
            }
            if point_in_polygon(rep, &f.raw) && best.map(|(_, a)| f.area < a).unwrap_or(true) {
                best = Some((i, f.area));
            }
        }
        comp_parent.insert(*comp, best.map(|(i, _)| i));
    }

    // Exterior edges: on the outer boundary of a top-level component, or a
    // bridge that lies outside every bounded face.
    for (comp, outer) in &outer_of {
        if comp_parent[comp].is_none() {
            for he in &raw_faces[*outer].walk {
                edges[he / 2].exterior = true;
            }
        }
    }
    for e in edges.iter_mut().filter(|e| e.bridge) {
        let mid = lerp(g.nodes[e.a], g.nodes[e.b], 0.5);
        let inside = raw_faces
            .iter()
            .enumerate()
            .any(|(i, f)| !is_outer(i) && f.area > 0.0 && point_in_polygon(mid, &f.raw));
        e.exterior = !inside;
    }

    let offsets = |walk: &[usize]| -> Vec<f64> {
        walk.iter()
            .map(|he| edges[he / 2].thickness / 2.0)
            .collect()
    };

    // Room faces.
    let mut faces: Vec<Face> = vec![];
    for (i, f) in raw_faces.iter().enumerate() {
        if is_outer(i) || f.area <= 0.0 {
            continue;
        }
        let net = remove_collinear(&offset_walk_left(&f.raw, &offsets(&f.walk)), 1e-6);
        let net_area = signed_area(&net);
        if net.len() < 3
            || !is_simple(&net)
            || net_area < MIN_ROOM_AREA_MM2
            || net_area > f.area + 1e-6
        {
            continue;
        }
        let mut holes: Vec<Vec<Point>> = vec![];
        let mut hole_area = 0.0;
        for (comp, outer) in &outer_of {
            if comp_parent[comp] != Some(i) {
                continue;
            }
            let o = &raw_faces[*outer];
            let ring = remove_collinear(&offset_walk_left(&o.raw, &offsets(&o.walk)), 1e-6);
            let a = signed_area(&ring);
            if ring.len() >= 3 && a < 0.0 {
                hole_area += -a;
                holes.push(ring);
            }
        }
        let area = net_area - hole_area;
        if area < MIN_ROOM_AREA_MM2 {
            continue;
        }
        let Some(label) = label_point(&net, &holes, 10.0) else {
            continue;
        };
        let mut wall_ids: Vec<Id> = vec![];
        let mut boundary = vec![];
        for he in &f.walk {
            let e = &edges[he / 2];
            for id in &e.wall_ids {
                if !wall_ids.contains(id) {
                    wall_ids.push(id.clone());
                }
            }
            boundary.push(BoundaryEdge {
                from: g.nodes[he_from(&edges, *he)],
                to: g.nodes[he_to(&edges, *he)],
                wall_ids: e.wall_ids.clone(),
                exterior: e.exterior,
            });
        }
        let face = Face {
            level_id: level_id.to_string(),
            centerline: remove_collinear(&f.raw, 1e-6),
            perimeter_mm: perimeter(&net),
            net,
            holes,
            area_mm2: area,
            centerline_area_mm2: f.area,
            label,
            wall_ids,
            boundary,
        };
        if face.contains_net(face.label) && face.contains(face.label) {
            faces.push(face);
        }
    }
    faces.sort_by(|a, b| {
        a.label
            .x
            .total_cmp(&b.label.x)
            .then(a.label.y.total_cmp(&b.label.y))
    });

    // Footprints of top-level components.
    let mut footprints: Vec<(Vec<Point>, f64)> = vec![];
    for (comp, outer) in &outer_of {
        if comp_parent[comp].is_some() {
            continue;
        }
        let o = &raw_faces[*outer];
        if o.area >= 0.0 {
            continue;
        }
        let mut ring = remove_collinear(&offset_walk_left(&o.raw, &offsets(&o.walk)), 1e-6);
        ring.reverse();
        let a = signed_area(&ring);
        if ring.len() >= 3 && a > 0.0 {
            // Start at the lowest, then leftmost vertex so the output is stable.
            let first = (0..ring.len())
                .min_by(|i, j| {
                    ring[*i]
                        .y
                        .total_cmp(&ring[*j].y)
                        .then(ring[*i].x.total_cmp(&ring[*j].x))
                })
                .unwrap_or(0);
            ring.rotate_left(first);
            footprints.push((ring, a));
        }
    }
    footprints.sort_by(|a, b| b.1.total_cmp(&a.1));

    let wall_topos = wall_outlines(walls, &g.nodes, &ends, &wall_nodes, &edges);

    (
        LevelTopo {
            level_id: level_id.to_string(),
            edges,
            faces,
            footprints,
        },
        wall_topos,
    )
}

/// Corner where my side line meets a neighbor's side line. Falls back to the
/// square corner for parallel walls and for very sharp angles.
fn outline_corner(
    p: Point,
    my_dir: Point,
    my_off: Point,
    n_dir: Point,
    n_off: Point,
    limit: f64,
) -> Point {
    let square = add(p, my_off);
    if cross(my_dir, n_dir).abs() < 0.0175 {
        return square;
    }
    match line_intersect(square, my_dir, add(p, n_off), n_dir) {
        Some(x) if dist(x, p) <= limit => x,
        _ => square,
    }
}

fn wall_outlines(
    walls: &[&Wall],
    nodes: &[Point],
    ends: &[(usize, usize)],
    wall_nodes: &[Vec<(f64, usize)>],
    edges: &[Edge],
) -> Vec<WallTopo> {
    // Everything that leaves each node.
    let mut at_node: Vec<Vec<Incidence>> = vec![vec![]; nodes.len()];
    for (wi, w) in walls.iter().enumerate() {
        let Some(d) = unit(sub(w.end, w.start)) else {
            continue;
        };
        let back = scale(d, -1.0);
        let count = wall_nodes[wi].len();
        for (k, (_, n)) in wall_nodes[wi].iter().enumerate() {
            let t = w.thickness_mm;
            if k == 0 {
                at_node[*n].push(Incidence {
                    wall: wi,
                    dir: d,
                    thickness: t,
                    kind: EndKind::Start,
                });
            } else if k == count - 1 {
                at_node[*n].push(Incidence {
                    wall: wi,
                    dir: back,
                    thickness: t,
                    kind: EndKind::End,
                });
            } else {
                at_node[*n].push(Incidence {
                    wall: wi,
                    dir: d,
                    thickness: t,
                    kind: EndKind::Through,
                });
                at_node[*n].push(Incidence {
                    wall: wi,
                    dir: back,
                    thickness: t,
                    kind: EndKind::Through,
                });
            }
        }
    }

    let mut out = vec![];
    for (wi, w) in walls.iter().enumerate() {
        let len = dist(w.start, w.end);
        let Some(d) = unit(sub(w.end, w.start)) else {
            continue;
        };
        let half = w.thickness_mm / 2.0;

        // Returns (corner on the left of `u`, corner on the right of `u`, center point if needed).
        let end_corners = |node: usize, p: Point, u: Point| -> (Point, Point, Option<Point>) {
            let left = scale(perp_left(u), half);
            let right = scale(left, -1.0);
            let mut others: Vec<(f64, &Incidence)> = at_node[node]
                .iter()
                .filter(|i| i.wall != wi)
                .map(|i| {
                    let mut ang = cross(u, i.dir).atan2(dot(u, i.dir));
                    if ang < 0.0 {
                        ang += std::f64::consts::TAU;
                    }
                    (ang, i)
                })
                .collect();
            if others.is_empty() {
                return (add(p, left), add(p, right), None);
            }
            others.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.wall.cmp(&b.1.wall)));
            let ccw = others[0].1;
            let cw = others[others.len() - 1].1;
            let limit = |o: &Incidence| OUTLINE_MITRE_LIMIT * w.thickness_mm.max(o.thickness) + 1.0;
            let l = outline_corner(
                p,
                u,
                left,
                ccw.dir,
                scale(perp_left(ccw.dir), -ccw.thickness / 2.0),
                limit(ccw),
            );
            let r = outline_corner(
                p,
                u,
                right,
                cw.dir,
                scale(perp_left(cw.dir), cw.thickness / 2.0),
                limit(cw),
            );
            let has_through = others.iter().any(|(_, i)| i.kind == EndKind::Through);
            let center = if others.len() >= 2 && !has_through && ccw.wall != cw.wall {
                Some(p)
            } else {
                None
            };
            (l, r, center)
        };

        let (start_node, end_node) = ends[wi];
        let (s_left, s_right, s_center) = end_corners(start_node, w.start, d);
        let (e_right, e_left, e_center) = end_corners(end_node, w.end, scale(d, -1.0));
        let mut outline = vec![s_right, e_right];
        if let Some(c) = e_center {
            outline.push(c);
        }
        outline.push(e_left);
        outline.push(s_left);
        if let Some(c) = s_center {
            outline.push(c);
        }
        if !outline.iter().all(|p| is_finite(*p)) || signed_area(&outline) <= 0.0 {
            let l = scale(perp_left(d), half);
            outline = vec![
                sub(w.start, l),
                sub(w.end, l),
                add(w.end, l),
                add(w.start, l),
            ];
        }

        let joined = |node: usize| -> Vec<Id> {
            let mut ids: Vec<Id> = vec![];
            for i in &at_node[node] {
                let id = &walls[i.wall].id;
                if i.wall != wi && !ids.contains(id) {
                    ids.push(id.clone());
                }
            }
            ids
        };
        let mut junctions = vec![];
        for (along, n) in wall_nodes[wi]
            .iter()
            .skip(1)
            .take(wall_nodes[wi].len().saturating_sub(2))
        {
            for i in &at_node[*n] {
                if i.wall != wi && !junctions.iter().any(|(_, id)| id == &walls[i.wall].id) {
                    junctions.push((*along, walls[i.wall].id.clone()));
                }
            }
        }
        let exterior = edges
            .iter()
            .any(|e| e.exterior && e.wall_ids.contains(&w.id));
        out.push(WallTopo {
            wall_id: w.id.clone(),
            length_mm: len,
            outline,
            exterior,
            joined_at_start: joined(start_node),
            joined_at_end: joined(end_node),
            junctions_mm: junctions,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use guhit_model::defaults;

    pub fn project_with_walls(walls: &[(f64, f64, f64, f64, f64)]) -> Project {
        let mut p = defaults::new_project("t");
        let level = p.levels[0].id.clone();
        for (i, (x0, y0, x1, y1, t)) in walls.iter().enumerate() {
            p.elements.push(Element::Wall(Wall {
                id: format!("w{}", i + 1),
                level_id: level.clone(),
                start: pt(*x0, *y0),
                end: pt(*x1, *y1),
                thickness_mm: *t,
                height_mm: None,
                material_id: None,
            }));
        }
        p
    }

    fn bungalow() -> Project {
        project_with_walls(&[
            (0.0, 0.0, 8000.0, 0.0, 150.0),
            (8000.0, 0.0, 8000.0, 6000.0, 150.0),
            (8000.0, 6000.0, 0.0, 6000.0, 150.0),
            (0.0, 6000.0, 0.0, 0.0, 150.0),
            (5000.0, 0.0, 5000.0, 6000.0, 100.0),
        ])
    }

    #[test]
    fn two_rooms_with_t_junction_partition() {
        let a = analyze(&bungalow());
        let level = &a.levels[0];
        assert_eq!(level.faces.len(), 2);
        let living = &level.faces[0];
        assert_eq!(
            living.net,
            vec![
                pt(75.0, 75.0),
                pt(4950.0, 75.0),
                pt(4950.0, 5925.0),
                pt(75.0, 5925.0)
            ]
        );
        assert_eq!(living.area_mm2, 4875.0 * 5850.0);
        assert_eq!(living.wall_ids.len(), 4);
        let bed = &level.faces[1];
        assert_eq!(bed.area_mm2, 2875.0 * 5850.0);
        assert_eq!(level.footprints.len(), 1);
        assert_eq!(level.footprints[0].1, 8150.0 * 6150.0);
        assert_eq!(level.footprints[0].0.len(), 4);
        assert!(signed_area(&level.footprints[0].0) > 0.0);
    }

    #[test]
    fn exterior_flags_and_joins() {
        let a = analyze(&bungalow());
        let ext: Vec<bool> = a.walls.iter().map(|w| w.exterior).collect();
        assert_eq!(ext, vec![true, true, true, true, false]);
        assert_eq!(a.walls[0].joined_at_start, vec!["w4".to_string()]);
        assert_eq!(a.walls[0].joined_at_end, vec!["w2".to_string()]);
        assert_eq!(a.walls[4].joined_at_start, vec!["w1".to_string()]);
        assert_eq!(a.walls[4].joined_at_end, vec!["w3".to_string()]);
        assert_eq!(a.walls[0].junctions_mm, vec![(5000.0, "w5".to_string())]);
    }

    #[test]
    fn l_corner_mitres_and_t_junction_butts() {
        let a = analyze(&bungalow());
        let south = &a.walls[0].outline;
        assert_eq!(
            south,
            &vec![
                pt(-75.0, -75.0),
                pt(8075.0, -75.0),
                pt(7925.0, 75.0),
                pt(75.0, 75.0)
            ]
        );
        let partition = &a.walls[4].outline;
        assert_eq!(
            partition,
            &vec![
                pt(5050.0, 75.0),
                pt(5050.0, 5925.0),
                pt(4950.0, 5925.0),
                pt(4950.0, 75.0)
            ]
        );
        for w in &a.walls {
            assert!(signed_area(&w.outline) > 0.0);
        }
    }

    #[test]
    fn free_wall_has_square_ends_and_no_room() {
        let a = analyze(&project_with_walls(&[(0.0, 0.0, 3000.0, 0.0, 200.0)]));
        assert_eq!(
            a.walls[0].outline,
            vec![
                pt(0.0, -100.0),
                pt(3000.0, -100.0),
                pt(3000.0, 100.0),
                pt(0.0, 100.0)
            ]
        );
        assert!(a.levels[0].faces.is_empty());
        assert!(a.levels[0].footprints.is_empty());
    }

    #[test]
    fn crossing_walls_make_four_rooms() {
        let a = analyze(&project_with_walls(&[
            (0.0, 0.0, 6000.0, 0.0, 150.0),
            (6000.0, 0.0, 6000.0, 6000.0, 150.0),
            (6000.0, 6000.0, 0.0, 6000.0, 150.0),
            (0.0, 6000.0, 0.0, 0.0, 150.0),
            (3000.0, 0.0, 3000.0, 6000.0, 100.0),
            (0.0, 3000.0, 6000.0, 3000.0, 100.0),
        ]));
        assert_eq!(a.levels[0].faces.len(), 4);
        for f in &a.levels[0].faces {
            assert!((f.area_mm2 - 2875.0 * 2875.0).abs() < 1e-6);
        }
    }

    #[test]
    fn endpoints_within_one_mm_join() {
        let a = analyze(&project_with_walls(&[
            (0.0, 0.0, 4000.0, 0.0, 150.0),
            (4000.4, 0.3, 4000.0, 3000.0, 150.0),
            (4000.0, 3000.0, 0.0, 3000.0, 150.0),
            (0.0, 3000.0, 0.5, -0.5, 150.0),
        ]));
        assert_eq!(a.levels[0].faces.len(), 1);
    }

    #[test]
    fn dangling_wall_and_bridge_do_not_break_the_room() {
        let a = analyze(&project_with_walls(&[
            (0.0, 0.0, 6000.0, 0.0, 150.0),
            (6000.0, 0.0, 6000.0, 6000.0, 150.0),
            (6000.0, 6000.0, 0.0, 6000.0, 150.0),
            (0.0, 6000.0, 0.0, 0.0, 150.0),
            // Stub wall inside the room.
            (0.0, 3000.0, 1500.0, 3000.0, 100.0),
            // Fence wall outside.
            (6000.0, 0.0, 9000.0, 0.0, 100.0),
        ]));
        let level = &a.levels[0];
        assert_eq!(level.faces.len(), 1);
        assert_eq!(level.faces[0].net.len(), 4);
        assert!(!a.walls[4].exterior);
        assert!(a.walls[5].exterior);
    }

    #[test]
    fn island_inside_a_room_is_a_hole() {
        let a = analyze(&project_with_walls(&[
            (0.0, 0.0, 8000.0, 0.0, 200.0),
            (8000.0, 0.0, 8000.0, 8000.0, 200.0),
            (8000.0, 8000.0, 0.0, 8000.0, 200.0),
            (0.0, 8000.0, 0.0, 0.0, 200.0),
            (3000.0, 3000.0, 5000.0, 3000.0, 100.0),
            (5000.0, 3000.0, 5000.0, 5000.0, 100.0),
            (5000.0, 5000.0, 3000.0, 5000.0, 100.0),
            (3000.0, 5000.0, 3000.0, 3000.0, 100.0),
        ]));
        let level = &a.levels[0];
        assert_eq!(level.faces.len(), 2);
        let big = level
            .faces
            .iter()
            .find(|f| f.centerline_area_mm2 > 5e7)
            .unwrap();
        assert_eq!(big.holes.len(), 1);
        assert!((big.area_mm2 - (7800.0 * 7800.0 - 2100.0 * 2100.0)).abs() < 1e-6);
        assert!(big.contains_net(big.label));
        // Island walls are not exterior, and there is one footprint.
        assert!(!a.walls[4].exterior);
        assert_eq!(level.footprints.len(), 1);
    }

    #[test]
    fn overlapping_walls_share_edges() {
        let a = analyze(&project_with_walls(&[
            (0.0, 0.0, 4000.0, 0.0, 150.0),
            (4000.0, 0.0, 4000.0, 3000.0, 150.0),
            (4000.0, 3000.0, 0.0, 3000.0, 150.0),
            (0.0, 3000.0, 0.0, 0.0, 150.0),
            (1000.0, 0.0, 3000.0, 0.0, 150.0),
        ]));
        let level = &a.levels[0];
        assert_eq!(level.faces.len(), 1);
        assert!(level.edges.iter().any(|e| e.wall_ids.len() == 2));
        assert!((level.faces[0].area_mm2 - 3850.0 * 2850.0).abs() < 1e-6);
    }

    #[test]
    fn room_too_thin_for_its_walls_is_not_a_room() {
        let a = analyze(&project_with_walls(&[
            (0.0, 0.0, 4000.0, 0.0, 150.0),
            (4000.0, 0.0, 4000.0, 100.0, 150.0),
            (4000.0, 100.0, 0.0, 100.0, 150.0),
            (0.0, 100.0, 0.0, 0.0, 150.0),
        ]));
        assert!(a.levels[0].faces.is_empty());
    }
}
