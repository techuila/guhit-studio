//! Base quantities: the IFC4 `Qto_*BaseQuantities` sets for walls, slabs,
//! spaces, doors, windows, roofs and stair flights, as Revit and ArchiCAD
//! write them. Lengths are in the project unit (mm), areas in m² and
//! volumes in m³, as the unit assignment says. Every value is computed from
//! the same geometry the file draws.

use super::*;

/// One quantity of a set.
pub(super) enum Q {
    /// mm.
    Length(f64),
    /// m².
    Area(f64),
    /// m³.
    Volume(f64),
}

impl Q {
    fn step(&self, name: &str) -> String {
        // IFC4 refuses negative quantities.
        let k = |x: f64| r(if x.is_finite() { x.max(0.0) } else { 0.0 });
        match self {
            Q::Length(v) => format!("IFCQUANTITYLENGTH({},$,$,{},$)", s(name), k(*v)),
            Q::Area(v) => format!("IFCQUANTITYAREA({},$,$,{},$)", s(name), k(*v)),
            Q::Volume(v) => format!("IFCQUANTITYVOLUME({},$,$,{},$)", s(name), k(*v)),
        }
    }
}

/// mm² to m².
pub(super) fn m2(mm2: f64) -> f64 {
    mm2 / 1.0e6
}

/// An area in m² over a height in mm, in m³.
pub(super) fn m3(area_m2: f64, mm: f64) -> f64 {
    area_m2 * mm / 1000.0
}

/// Length round a closed polygon, mm.
pub(super) fn perimeter(poly: &[V]) -> f64 {
    if poly.len() < 2 {
        return 0.0;
    }
    (0..poly.len()).map(|i| (poly[(i + 1) % poly.len()] - poly[i]).len()).sum()
}

impl Build<'_> {
    /// An IfcElementQuantity named `name` on `elements`, with ids stable
    /// from `owner_id`.
    pub(super) fn qto(&mut self, name: &str, owner_id: &str, quantities: &[(&str, Q)], elements: &[Ref]) {
        if quantities.is_empty() || elements.is_empty() {
            return;
        }
        let refs: Vec<Ref> = quantities.iter().map(|(n, q)| self.step.add(&q.step(n))).collect();
        let set = self.step.add(&format!(
            "IFCELEMENTQUANTITY({},$,{},$,'BaseQuantities',{})",
            s(&guid_for(name, owner_id)),
            s(name),
            list(&refs)
        ));
        self.step.add(&format!(
            "IFCRELDEFINESBYPROPERTIES({},$,$,$,{},{set})",
            s(&guid_for(&format!("{name}-rel"), owner_id)),
            list(elements)
        ));
        self.counts.quantity_sets += 1;
    }

    /// Qto_SlabBaseQuantities of a slab `thickness` mm thick over `outline`,
    /// less `cut_m2` of voids.
    pub(super) fn slab_qto(&mut self, owner_id: &str, outline: &[V], thickness: f64, cut_m2: f64, slab: Ref) {
        let gross = m2(polygon_area(outline));
        let net = (gross - cut_m2).max(0.0);
        self.qto(
            "Qto_SlabBaseQuantities",
            owner_id,
            &[
                ("Width", Q::Length(thickness)),
                ("Perimeter", Q::Length(perimeter(outline))),
                ("GrossArea", Q::Area(gross)),
                ("NetArea", Q::Area(net)),
                ("GrossVolume", Q::Volume(m3(gross, thickness))),
                ("NetVolume", Q::Volume(m3(net, thickness))),
            ],
            &[slab],
        );
    }

    /// Qto_DoorBaseQuantities or Qto_WindowBaseQuantities.
    pub(super) fn opening_qto(&mut self, owner_id: &str, door: bool, width: f64, height: f64, filler: Ref) {
        let name = if door { "Qto_DoorBaseQuantities" } else { "Qto_WindowBaseQuantities" };
        self.qto(
            name,
            owner_id,
            &[
                ("Width", Q::Length(width)),
                ("Height", Q::Length(height)),
                ("Perimeter", Q::Length(2.0 * (width + height))),
                ("Area", Q::Area(m2(width * height))),
            ],
            &[filler],
        );
    }
}
