//! Document API: undo and redo, revisions, rename, and the golden fixture.

mod common;

use common::*;
use guhit_core::{compute_derived, templates, CoreError, Document};
use guhit_model::*;

#[test]
fn undo_restores_the_exact_project_and_redo_the_exact_result() {
    let mut doc = Document::new(templates::sample_bungalow());
    let original = doc.project().clone();
    let mut snapshots = vec![];
    let cmds = vec![
        add_wall(p(0.0, 3500.0), p(5000.0, 3500.0)),
        Command::ResizeRoom {
            room_id: "00000000-0000-4000-8000-000000000302".into(),
            side: Side::East,
            delta_mm: 700.0,
        },
        Command::DeleteElements {
            ids: vec!["00000000-0000-4000-8000-000000000105".into()],
        },
    ];
    for c in cmds {
        let r = apply_checked(&mut doc, c);
        snapshots.push(r.state.project.clone());
    }
    assert_eq!(doc.revision(), 3);
    assert_eq!(doc.state().undo_label.as_deref(), Some("Delete"));
    for i in (0..3).rev() {
        let s = doc.undo().unwrap();
        let expect = if i == 0 { &original } else { &snapshots[i - 1] };
        assert_eq!(&s.project, expect, "undo {i} is exact, timestamps included");
        assert_eq!(s.derived, compute_derived(expect));
    }
    assert!(matches!(doc.undo(), Err(CoreError::NothingToUndo)));
    assert_eq!(doc.state().redo_label.as_deref(), Some("Add wall"));
    for snap in &snapshots {
        assert_eq!(&doc.redo().unwrap().project, snap);
    }
    assert!(matches!(doc.redo(), Err(CoreError::NothingToRedo)));
    assert_eq!(
        doc.revision(),
        9,
        "every apply, undo and redo bumps the revision"
    );

    // A new command clears the redo stack.
    doc.undo().unwrap();
    apply_checked(&mut doc, add_wall(p(0.0, -2000.0), p(3000.0, -2000.0)));
    assert!(!doc.state().can_redo);
}

#[test]
fn with_revision_starts_the_counter_and_has_no_history() {
    let mut doc = Document::with_revision(templates::sample_bungalow(), 41);
    assert_eq!(doc.revision(), 41);
    let s = doc.state();
    assert_eq!(s.revision, 41);
    assert!(!s.can_undo && !s.can_redo);
    assert_eq!(
        s.derived,
        Document::new(templates::sample_bungalow()).state().derived
    );
    assert!(matches!(doc.undo(), Err(CoreError::NothingToUndo)));
    assert_eq!(
        doc.preview(&add_wall(p(0.0, -2000.0), p(3000.0, -2000.0)))
            .unwrap()
            .state
            .revision,
        42
    );
    let r = doc
        .apply(add_wall(p(0.0, -2000.0), p(3000.0, -2000.0)), Origin::User)
        .unwrap();
    assert_eq!(r.state.revision, 42);
    assert_eq!(doc.undo().unwrap().revision, 43);
    assert_eq!(Document::new(templates::sample_bungalow()).revision(), 0);
}

#[test]
fn rename_is_not_an_undo_step_and_survives_undo_and_redo() {
    let mut doc = Document::new(templates::sample_bungalow());
    apply_checked(&mut doc, add_wall(p(0.0, -2000.0), p(3000.0, -2000.0)));
    apply_checked(&mut doc, add_wall(p(0.0, -4000.0), p(3000.0, -4000.0)));
    doc.undo().unwrap();
    // One entry on each stack now.
    let revision = doc.revision();
    let labels = (doc.state().undo_label, doc.state().redo_label);

    doc.rename("  Casa Reyes  ").unwrap();
    assert_eq!(doc.project().name, "Casa Reyes");
    assert_eq!(doc.state().project.name, "Casa Reyes");
    assert_eq!(
        doc.revision(),
        revision,
        "rename does not bump the revision"
    );
    assert_eq!(
        (doc.state().undo_label, doc.state().redo_label),
        labels,
        "rename does not touch the stacks"
    );
    assert_ne!(doc.project().updated_at, "2026-09-22T00:00:00Z");

    assert_eq!(
        doc.undo().unwrap().project.name,
        "Casa Reyes",
        "undo does not bring the old name back"
    );
    assert_eq!(doc.redo().unwrap().project.name, "Casa Reyes");
    assert_eq!(doc.redo().unwrap().project.name, "Casa Reyes");
    assert_eq!(walls(doc.project()).len(), 7);

    for bad in ["", "   ", "\t\n"] {
        let before = doc.project().clone();
        let e = doc.rename(bad).unwrap_err();
        assert_eq!(code(&e), "bad_name");
        assert_eq!(doc.project(), &before);
    }
}

#[test]
fn core_error_converts_to_ipc_error() {
    let mut doc = Document::new(templates::sample_bungalow());
    let e = doc
        .apply(
            Command::SetWallLength {
                wall_id: "00000000-0000-4000-8000-000000000105".into(),
                length_mm: 3000.0,
                anchor: WallAnchor::Start,
            },
            Origin::Ai,
        )
        .unwrap_err();
    let ipc: IpcError = e.into();
    assert_eq!(ipc.code, "invalid");
    assert_eq!(ipc.element_ids[0], "00000000-0000-4000-8000-000000000202");
    assert!(ipc.message.contains("does not fit"));
    let ipc: IpcError = CoreError::NotFound("x".into()).into();
    assert_eq!(
        (ipc.code.as_str(), ipc.element_ids),
        ("not_found", vec!["x".to_string()])
    );
}

// ------------------------------------------------------------------- golden

fn fixture() -> DocState {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/sample-bungalow.docstate.json");
    serde_json::from_str(&std::fs::read_to_string(path).expect("fixture exists"))
        .expect("fixture parses")
}

#[test]
fn golden_fixture_matches_the_engine() {
    let golden = fixture();
    let live = Document::new(templates::sample_bungalow()).state();
    assert_eq!(
        golden.project, live.project,
        "fixture project differs from templates::sample_bungalow()"
    );
    assert_eq!(
        golden.derived, live.derived,
        "regenerate: cargo run -p guhit-core --example gen_fixture"
    );
    assert_eq!(golden.revision, 0);
    assert!(!golden.can_undo && !golden.can_redo);
}

#[test]
fn golden_fixture_values() {
    let golden = fixture();
    assert_invariants(&golden);
    let d = &golden.derived;
    assert_eq!(walls(&golden.project).len(), 5);
    assert_eq!(rooms(&golden.project).len(), 2);
    assert_eq!(d.rooms.len(), 2);
    let living = room_geo(&golden, "00000000-0000-4000-8000-000000000301");
    let bedroom = room_geo(&golden, "00000000-0000-4000-8000-000000000302");
    assert_eq!(bbox(&living.polygon), (75.0, 75.0, 4950.0, 5925.0));
    assert_eq!(living.area_mm2, 4875.0 * 5850.0);
    assert_eq!(bbox(&bedroom.polygon), (5050.0, 75.0, 7925.0, 5925.0));
    assert_eq!(bedroom.area_mm2, 2875.0 * 5850.0);
    assert_eq!(
        living.centerline_polygon,
        vec![
            p(0.0, 0.0),
            p(5000.0, 0.0),
            p(5000.0, 6000.0),
            p(0.0, 6000.0)
        ]
    );
    assert_eq!(living.label_point, p(2512.5, 3000.0));
    assert_eq!(d.footprints.len(), 1);
    assert_eq!(
        d.footprints[0].polygon,
        vec![
            p(-75.0, -75.0),
            p(8075.0, -75.0),
            p(8075.0, 6075.0),
            p(-75.0, 6075.0)
        ]
    );
    assert_eq!(d.footprints[0].area_mm2, 8150.0 * 6150.0);
    assert!((d.totals.floor_area_m2 - 45.3375).abs() < 1e-9);
    assert!((d.totals.gross_area_m2 - 50.1225).abs() < 1e-9);
    assert_eq!(d.totals.wall_length_m, 34.0);
    assert_eq!(
        (
            d.totals.room_count,
            d.totals.door_count,
            d.totals.window_count
        ),
        (2, 2, 3)
    );
    let exterior: Vec<bool> = d.walls.iter().map(|w| w.exterior).collect();
    assert_eq!(exterior, vec![true, true, true, true, false]);
    assert!(d.issues.is_empty());
}
