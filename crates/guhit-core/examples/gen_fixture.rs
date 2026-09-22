//! Writes fixtures/sample-bungalow.docstate.json: a small two-room house used
//! by frontend development, export tests and the engine golden test.
//!
//! Run: cargo run -p guhit-core --example gen_fixture
//!
//! The project comes from `templates::sample_bungalow()` and every derived
//! value comes from the real engine. Nothing is injected by hand.

use guhit_core::{templates, Document};

fn main() {
    let state = Document::new(templates::sample_bungalow()).state();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/sample-bungalow.docstate.json");
    let mut json = serde_json::to_string_pretty(&state).unwrap();
    json.push('\n');
    std::fs::write(&path, json).unwrap();
    println!("wrote {}", path.display());
    for room in &state.derived.rooms {
        println!("room {} net {:.3} m2", room.room_id, room.area_mm2 / 1.0e6);
    }
}
