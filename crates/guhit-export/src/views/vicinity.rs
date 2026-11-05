//! The vicinity map frame (DECISIONS D53): an image the user or an agent
//! placed in the project's `files/`, framed, with a north arrow, the site
//! marked at its center and the radius note. No map service. The image is
//! taken to be north up and centered on the site; the drawing says so.

use guhit_model::Project;

use crate::geom::*;
use crate::plan::{Cat, Fill, Item, Pen, Prim};
use crate::text::clean;

use super::annot::{self, paper};
use super::{ViewImage, ViewItems};

/// The frame's side on paper, mm.
pub const FRAME_PAPER_MM: f64 = 110.0;

pub fn items(_p: &Project, file_name: Option<&str>, radius_m: f64, n: f64) -> ViewItems {
    let side = FRAME_PAPER_MM * n;
    let (min, max) = (v(0.0, 0.0), v(side, side));
    let c = v(side / 2.0, side / 2.0);
    let mut items = Vec::new();
    let mut images = Vec::new();
    // No image yet: the empty frame, ready to fill by hand. How to place
    // one is the Sheets panel's business, never a drawing's.
    if let Some(f) = file_name.map(str::trim).filter(|f| !f.is_empty()) {
        images.push(ViewImage { file_name: f.to_string(), min, max });
    }
    items.push(Item { cat: Cat::Text, pen: Pen::Medium, prim: Prim::Poly { pts: vec![min, v(side, 0.0), max, v(0.0, side)], closed: true, fill: Fill::None } });
    // The radius, dashed, and the site at the center.
    let r = side * 0.42;
    let ring = annot::circle_pts(c, r, 72);
    items.extend(annot::dashed_path(Cat::Symb, Pen::Light, &ring, true, &annot::scaled(&paper::HIDDEN, n)));
    items.push(annot::circle(Cat::Symb, Pen::Medium, c, 1.8 * n, Fill::Ink));
    items.push(annot::circle(Cat::Symb, Pen::Light, c, 3.6 * n, Fill::None));
    items.push(annot::centered(Cat::Text, c + v(0.0, 6.5 * n), paper::LABEL * n, 0.0, "SITE", true));
    let radius = if radius_m.is_finite() && radius_m > 0.0 { radius_m } else { 500.0 };
    let note = format!("{} m RADIUS", clean(&format!("{radius:.0}")));
    items.push(annot::centered(Cat::Text, v(c.x, c.y - r + 3.5 * n), paper::SMALL * n, 0.0, &note, false));
    // North up, top right inside the frame.
    let nr = paper::NORTH * n / 2.0;
    items.extend(annot::north_arrow(Cat::Symb, v(side - nr - 4.0 * n, side - nr - 7.0 * n), 0.0, n));
    let bounds = crate::plan::items_bounds(&items);
    ViewItems { items, bounds, title: "VICINITY MAP".into(), scale_denominator: n as u32, images }
}
