//! Types, as Revit writes them: an IfcDoorType or IfcWindowType per door
//! and window type (DECISIONS D53), named and tagged with its mark ("D1"),
//! untyped openings grouped by kind, style and size; an IfcWallType per
//! thickness and material, site walls apart. Each type defines its
//! occurrences (IfcRelDefinesByType) and is declared to the project
//! (IfcRelDeclares).

use guhit_model::{Opening, OpeningType, Wall};

use super::*;

struct OpeningGroup {
    key: String,
    door: bool,
    name: String,
    /// The mark, for typed openings.
    tag: Option<String>,
    description: String,
    element_type: String,
    /// IfcDoorTypeOperationEnum or IfcWindowTypePartitioningEnum.
    operation: &'static str,
    occurrences: Vec<Ref>,
}

struct WallGroup {
    key: String,
    name: String,
    site: bool,
    thickness: f64,
    material: String,
    occurrences: Vec<Ref>,
}

/// The types collected while the occurrences are written.
#[derive(Default)]
pub(super) struct Types {
    openings: Vec<OpeningGroup>,
    walls: Vec<WallGroup>,
}

fn mm(x: f64) -> String {
    format!("{}", x.round() as i64)
}

impl Types {
    /// File an opening under its type. `mark` is the schedule's mark for it,
    /// when there is one.
    pub fn add_opening(&mut self, project: &Project, o: &Opening, mark: Option<&str>, filler: Ref) {
        let door = o.opening_type == OpeningType::Door;
        let kind = if door { "Door" } else { "Window" };
        let size = format!("{} x {}", mm(o.width_mm), mm(o.height_mm));
        let generic = format!("{kind} {} {size}", style_label(o.style));
        let spec = o.type_id.as_ref().and_then(|id| project.opening_type(id));
        let (key, tag, description) = match spec {
            Some(t) => {
                let tag = mark
                    .map(str::to_string)
                    .filter(|m| !m.is_empty())
                    .or_else(|| Some(t.mark.clone()).filter(|m| !m.is_empty()));
                (format!("type:{}", t.id), tag, crate::text::clean(&t.description))
            }
            None => (format!("untyped:{kind}:{:?}:{}:{}", o.style, mm(o.width_mm), mm(o.height_mm)), None, String::new()),
        };
        if let Some(g) = self.openings.iter_mut().find(|g| g.key == key) {
            g.occurrences.push(filler);
            return;
        }
        self.openings.push(OpeningGroup {
            key,
            door,
            name: tag.clone().unwrap_or_else(|| generic.clone()),
            tag,
            description,
            element_type: format!("{} {size}", style_label(o.style)),
            operation: if door { door_operation(o) } else { window_partitioning(o.style) },
            occurrences: vec![filler],
        });
    }

    /// File a wall under its thickness and material.
    pub fn add_wall(&mut self, wall: &Wall, material: &str, wall_ref: Ref) {
        let t = wall.thickness_mm.abs();
        let site = wall.is_site();
        let key = format!("{}:{}:{material}", if site { "site" } else { "wall" }, mm(t));
        if let Some(g) = self.walls.iter_mut().find(|g| g.key == key) {
            g.occurrences.push(wall_ref);
            return;
        }
        let name = if site { format!("Site wall {} mm {material}", mm(t)) } else { format!("{material} {} mm", mm(t)) };
        self.walls.push(WallGroup {
            key,
            name,
            site,
            thickness: t,
            material: material.to_string(),
            occurrences: vec![wall_ref],
        });
    }

    /// Write every type, what it defines, and its declaration to the
    /// project. Wall types share their walls' material.
    pub fn write(self, build: &mut Build, ifc_project: Ref, project_id: &str) {
        let mut declared: Vec<Ref> = Vec::new();
        for g in &self.openings {
            let common = if g.door { "Pset_DoorCommon" } else { "Pset_WindowCommon" };
            let reference = build.step.add(&format!(
                "IFCPROPERTYSINGLEVALUE('Reference',$,{},$)",
                Val::Text(g.tag.clone().unwrap_or_else(|| g.name.clone())).step()
            ));
            let pset = build.step.add(&format!(
                "IFCPROPERTYSET({},$,{},$,({reference}))",
                s(&guid_for(&format!("{common}-type"), &g.key)),
                s(common)
            ));
            let description = if g.description.is_empty() { "$".to_string() } else { s(&g.description) };
            let tag = g.tag.as_deref().map(s).unwrap_or_else(|| "$".into());
            let ty = if g.door {
                build.counts.door_types += 1;
                build.step.add(&format!(
                    "IFCDOORTYPE({},$,{},{description},$,({pset}),$,{tag},{},.DOOR.,{},$,$)",
                    s(&guid_for("door-type", &g.key)),
                    s(&g.name),
                    s(&g.element_type),
                    g.operation
                ))
            } else {
                build.counts.window_types += 1;
                build.step.add(&format!(
                    "IFCWINDOWTYPE({},$,{},{description},$,({pset}),$,{tag},{},.WINDOW.,{},$,$)",
                    s(&guid_for("window-type", &g.key)),
                    s(&g.name),
                    s(&g.element_type),
                    g.operation
                ))
            };
            build.step.add(&format!(
                "IFCRELDEFINESBYTYPE({},$,$,$,{},{ty})",
                s(&guid_for("opening-type-of", &g.key)),
                list(&g.occurrences)
            ));
            declared.push(ty);
        }
        for g in &self.walls {
            let thickness = build.step.add(&format!(
                "IFCPROPERTYSINGLEVALUE('Thickness',$,{},$)",
                Val::PositiveLength(g.thickness.max(1.0)).step()
            ));
            let pset = build.step.add(&format!(
                "IFCPROPERTYSET({},$,'Guhit_Pset_WallType',$,({thickness}))",
                s(&guid_for("wall-type-pset", &g.key))
            ));
            let (element_type, predefined) = if g.site { ("BOUNDARYWALL", ".USERDEFINED.") } else { ("SOLIDWALL", ".SOLIDWALL.") };
            let ty = build.step.add(&format!(
                "IFCWALLTYPE({},$,{},$,$,({pset}),$,$,{},{predefined})",
                s(&guid_for("wall-type", &g.key)),
                s(&g.name),
                s(element_type)
            ));
            build.counts.wall_types += 1;
            build.material_named(&g.material, &[ty]);
            build.step.add(&format!(
                "IFCRELDEFINESBYTYPE({},$,$,$,{},{ty})",
                s(&guid_for("wall-type-of", &g.key)),
                list(&g.occurrences)
            ));
            declared.push(ty);
        }
        if !declared.is_empty() {
            build.step.add(&format!(
                "IFCRELDECLARES({},$,$,$,{ifc_project},{})",
                s(&guid_for("declares-types", project_id)),
                list(&declared)
            ));
        }
    }
}
