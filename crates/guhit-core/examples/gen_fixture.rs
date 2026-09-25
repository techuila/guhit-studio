//! Writes the golden fixtures used by frontend development, export tests and
//! the engine golden tests:
//! - fixtures/sample-bungalow.docstate.json: a small two-room house,
//! - fixtures/plumbing-demo.docstate.json: the bungalow with services
//!   (plumbing, storm, lighting, power and a split aircon).
//!
//! Run: cargo run -p guhit-core --example gen_fixture
//!
//! The projects come from `templates::sample_bungalow()` and
//! `templates::plumbing_demo()`, and every derived value comes from the real
//! engine. Nothing is injected by hand.

use guhit_core::{templates, Document};
use guhit_model::DocState;

fn write(name: &str, state: &DocState) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(name);
    let mut json = serde_json::to_string_pretty(state).unwrap();
    json.push('\n');
    std::fs::write(&path, json).unwrap();
    println!("wrote {}", path.display());
}

fn main() {
    let state = Document::new(templates::sample_bungalow()).state();
    write("sample-bungalow.docstate.json", &state);
    for room in &state.derived.rooms {
        println!("room {} net {:.3} m2", room.room_id, room.area_mm2 / 1.0e6);
    }

    let state = Document::new(templates::plumbing_demo()).state();
    write("plumbing-demo.docstate.json", &state);
    let pipes = &state.derived.pipes;
    for row in &pipes.takeoff {
        println!(
            "pipe {:?} {:?} {} mm: {:.3} m in {} runs",
            row.system, row.material, row.diameter_mm, row.length_m, row.run_count
        );
    }
    println!(
        "pipe total {:.3} m, {} elbows, {} tees, {} sleeves or flashings",
        pipes.total_length_m, pipes.elbow_count, pipes.tee_count, pipes.sleeve_count
    );
    let objects: u32 = state.derived.schedule.iter().map(|r| r.count).sum();
    println!(
        "schedule: {} objects counted in {} rows",
        objects,
        state.derived.schedule.len()
    );
    for issue in &state.derived.issues {
        println!("issue {}: {}", issue.code, issue.message);
    }
}
