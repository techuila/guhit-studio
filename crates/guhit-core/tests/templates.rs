//! Starter projects are stable: building one twice gives the same project,
//! down to the id of the roof that schema 6 makes from the project roof.
//!
//! That id is seeded (`ids.rs`) from the project: its id, every element,
//! material, type, sheet and shot id, and the roof itself. So it changes
//! only when the template's content does, for example when a built-in
//! material is added to `defaults::default_materials`.

use guhit_core::{migrate, templates, Document};
use guhit_model::*;

fn roof_id(p: &Project) -> Id {
    p.top_roof_area().expect("the template has a roof over the top floor").id.clone()
}

#[test]
fn building_a_template_twice_gives_the_same_roof_id() {
    for build in [templates::sample_bungalow, templates::plumbing_demo] {
        let (a, b) = (build(), build());
        assert_eq!(roof_id(&a), roof_id(&b));
        assert_eq!(a, b, "the whole project is the same");
        // Opening it migrates again, which leaves the roof alone.
        assert_eq!(roof_id(Document::new(build()).project()), roof_id(&a));
    }
}

/// A schema 5 project: the old project roof, no roof element yet.
fn before_schema_6() -> Project {
    let mut p = templates::sample_bungalow();
    p.elements.retain(|e| !matches!(e, Element::Roof(_)));
    p.roof.kind = RoofKind::Gable;
    p.schema_version = 5;
    p
}

#[test]
fn migration_seeds_the_roof_id_from_the_project_alone() {
    let (mut a, mut b) = (before_schema_6(), before_schema_6());
    migrate(&mut a);
    migrate(&mut b);
    assert_eq!(roof_id(&a), roof_id(&b), "same project, same id");
    assert_eq!(roof_id(&a), roof_id(&templates::sample_bungalow()), "the template's own migration");

    // Another project gets another id.
    let mut other = before_schema_6();
    other.id = "00000000-0000-4000-8000-0000000000ff".into();
    migrate(&mut other);
    assert_ne!(roof_id(&other), roof_id(&a));

    // The materials are part of the seed: `mat-paving-stone` (Wave 2 lane
    // L5) is what moved the fixture's roof id from fb150be7-... to
    // 16050954-... between two regenerations.
    let mut fewer = before_schema_6();
    fewer.materials.retain(|m| m.id != "mat-paving-stone");
    migrate(&mut fewer);
    assert_ne!(roof_id(&fewer), roof_id(&a));
}
