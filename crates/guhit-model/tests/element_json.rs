//! Every element kind survives a JSON round trip. `Element` is tagged by
//! `kind`, so a variant with its own `kind` field would write the key twice
//! and fail to read back: this catches that for new element kinds.

use guhit_model::*;

fn pt(x: f64, y: f64) -> Point {
    Point { x, y }
}

fn round_trip(el: Element) {
    let json = serde_json::to_string(&el).unwrap();
    let back: Element = serde_json::from_str(&json).unwrap_or_else(|e| panic!("{json} does not read back: {e}"));
    assert_eq!(back, el, "{json}");
}

#[test]
fn a_slab_keeps_its_own_kind_apart_from_the_element_tag() {
    let slab = Element::Slab(Slab {
        id: "s1".into(),
        level_id: "l2".into(),
        kind: SlabKind::Void,
        polygon: vec![pt(0.0, 0.0), pt(4000.0, 0.0), pt(4000.0, 3000.0), pt(0.0, 3000.0)],
        thickness_mm: 200.0,
        top_offset_mm: 0.0,
        material_id: None,
        railing: Some(Railing { height_mm: 1000.0, style: RailingStyle::Glass }),
    });
    let json = serde_json::to_value(&slab).unwrap();
    assert_eq!(json["kind"], "slab");
    assert_eq!(json["slab_kind"], "void");
    round_trip(slab);
}

#[test]
fn a_stair_reads_without_a_railing_from_older_files() {
    let json = r#"{"kind":"stair","id":"st","level_id":"l1","origin":{"x":0,"y":0},"rotation_deg":0,"width_mm":900,"run_mm":3900,"riser_count":17}"#;
    let el: Element = serde_json::from_str(json).unwrap();
    let Element::Stair(s) = &el else { panic!("not a stair") };
    assert_eq!(s.railing, None);
    round_trip(el);
}
