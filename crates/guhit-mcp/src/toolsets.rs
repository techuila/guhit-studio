//! Toolsets and the v1 names (MCP v2, docs/MCP.md "Toolsets").
//!
//! `draft`, the core toolset, is always on: what an agent needs to draft and
//! check a house. The extended toolsets are opt in, by the MCP URL
//! (`/mcp?toolsets=services,visuals`) or by the `mcp_toolsets` setting the
//! Connect agent dialog writes. Toolsets only decide what `tools/list`
//! shows, so the list stays small for clients that cap it (Cursor warns
//! above 40); a call to a tool that is not listed still works.
//!
//! The v1 tool names keep working for one release as hidden aliases: never
//! listed, still answered, so an agent set up on v1 does not break.

/// The opt-in toolsets, in the order the docs list them.
pub const EXTENDED: [&str; 7] = ["services", "visuals", "live", "review", "interop", "documents", "tours"];

/// The core toolset, in the order `tools/list` shows it.
pub const DRAFT: [&str; 26] = [
    "project",
    "get_guide",
    "query",
    "look",
    "measure",
    "check",
    "build",
    "batch",
    "add_walls",
    "add_room",
    "add_opening",
    "add_stair",
    "add_column",
    "add_slab",
    "place_asset",
    "add_grid",
    "annotate",
    "add_roof",
    "types",
    "modify",
    "reshape",
    "delete",
    "levels",
    "set_project",
    "history",
    "export",
];

/// Extended tools by toolset.
pub const SERVICES: [&str; 3] = ["edit_pipe", "add_pipe", "link_devices"];
pub const VISUALS: [&str; 3] = ["render", "visualize_render", "set_look"];
pub const LIVE: [&str; 3] = ["get_session", "send_chat_message", "set_status"];
pub const REVIEW: [&str; 1] = ["set_review_mark"];
pub const INTEROP: [&str; 2] = ["import_underlay", "import_dxf"];
pub const DOCUMENTS: [&str; 4] = ["title_block", "sheets", "view_marks", "set_lot"];
pub const TOURS: [&str; 3] = ["make_tour", "edit_tour", "export_tour"];

/// The tools of one extended toolset.
pub fn tools_of(toolset: &str) -> &'static [&'static str] {
    match toolset {
        "services" => &SERVICES,
        "visuals" => &VISUALS,
        "live" => &LIVE,
        "review" => &REVIEW,
        "interop" => &INTEROP,
        "documents" => &DOCUMENTS,
        "tours" => &TOURS,
        "draft" => &DRAFT,
        _ => &[],
    }
}

/// The toolset a v2 tool belongs to.
pub fn toolset_of(tool: &str) -> Option<&'static str> {
    if DRAFT.contains(&tool) {
        return Some("draft");
    }
    EXTENDED.into_iter().find(|t| tools_of(t).contains(&tool))
}

/// Every v2 tool name, core first.
pub fn all_v2() -> Vec<&'static str> {
    let mut out: Vec<&'static str> = DRAFT.to_vec();
    for t in EXTENDED {
        out.extend_from_slice(tools_of(t));
    }
    out
}

/// Which extended toolsets are on. `draft` is always on.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Toolsets {
    on: Vec<&'static str>,
}

impl Toolsets {
    /// Only the core toolset.
    pub fn core() -> Self {
        Self::default()
    }

    /// Every toolset.
    pub fn all() -> Self {
        Self { on: EXTENDED.to_vec() }
    }

    /// "services,visuals", "all" or "". Unknown names are left out: a list
    /// request cannot fail.
    pub fn parse(list: &str) -> Self {
        let mut on = vec![];
        for name in list.split([',', ' ', '+']).map(str::trim).filter(|n| !n.is_empty()) {
            let name = name.to_ascii_lowercase();
            if name == "all" {
                return Self::all();
            }
            if let Some(t) = EXTENDED.into_iter().find(|t| *t == name) {
                if !on.contains(&t) {
                    on.push(t);
                }
            }
        }
        on.sort_by_key(|t| EXTENDED.iter().position(|e| e == t));
        Self { on }
    }

    /// The `toolsets` parameter of a request URL's query string, when it has one.
    pub fn from_query(query: &str) -> Option<Self> {
        query
            .split('&')
            .filter_map(|kv| kv.split_once('='))
            .find(|(k, _)| *k == "toolsets")
            .map(|(_, v)| Self::parse(&v.replace("%2C", ",").replace("%2c", ",")))
    }

    pub fn enabled(&self) -> &[&'static str] {
        &self.on
    }

    pub fn has(&self, toolset: &str) -> bool {
        toolset == "draft" || self.on.contains(&toolset)
    }

    /// The v2 tools listed with these toolsets on, in list order.
    pub fn tools(&self) -> Vec<&'static str> {
        let mut out: Vec<&'static str> = DRAFT.to_vec();
        for t in EXTENDED {
            if self.has(t) {
                out.extend_from_slice(tools_of(t));
            }
        }
        out
    }
}

/// v1 tool names and the v2 tool that replaces each. Answered, never listed.
pub const ALIASES: [(&str, &str); 52] = [
    ("list_projects", "project"),
    ("open_project", "project"),
    ("create_project", "project"),
    ("close_project", "project"),
    ("save_version", "project"),
    ("get_project_summary", "query"),
    ("list_rooms", "query"),
    ("list_elements", "query"),
    ("describe_elements", "query"),
    ("get_selection", "query"),
    ("get_schedule", "query"),
    ("get_pipe_takeoff", "query"),
    ("find_rooms_without_exterior_window", "check"),
    ("list_review_items", "check"),
    ("get_plan_image", "look"),
    ("capture_view", "look"),
    ("add_wall", "add_walls"),
    ("add_wall_chain", "add_walls"),
    ("add_rect_room", "add_room"),
    ("add_door", "add_opening"),
    ("add_window", "add_opening"),
    ("resize_room", "reshape"),
    ("set_wall_length", "reshape"),
    ("move_elements", "modify"),
    ("rename_room", "modify"),
    ("set_room_usage", "modify"),
    ("set_opening_size", "modify"),
    ("set_material", "modify"),
    ("delete_elements", "delete"),
    ("set_roof", "set_project"),
    ("add_asset", "place_asset"),
    ("add_level", "levels"),
    ("delete_level", "levels"),
    ("undo", "history"),
    ("redo", "history"),
    ("export_plan", "export"),
    ("add_camera", "build"),
    ("move_pipe_points", "edit_pipe"),
    ("set_pipe_segment_length", "edit_pipe"),
    ("set_pipe_slope", "edit_pipe"),
    ("insert_pipe_point", "edit_pipe"),
    ("remove_pipe_point", "edit_pipe"),
    ("render_view", "render"),
    ("get_render_job", "render"),
    ("get_render_image", "render"),
    ("list_renders", "render"),
    // Same name in v1 and v2: listed under v2, kept here so the full v1
    // set is on record.
    ("get_guide", "get_guide"),
    ("batch", "batch"),
    ("set_review_mark", "set_review_mark"),
    ("get_session", "get_session"),
    ("send_chat_message", "send_chat_message"),
    ("visualize_render", "visualize_render"),
];

/// The v2 tool a v1 name maps to, for names that changed.
pub fn alias_of(name: &str) -> Option<&'static str> {
    ALIASES.iter().find(|(old, new)| *old == name && old != new).map(|(_, new)| *new)
}

/// The nearest tool name to an unknown one: a v2 name, or the v2 tool a
/// close v1 name maps to.
pub fn nearest(name: &str) -> Option<&'static str> {
    let wanted = name.trim().to_ascii_lowercase();
    let mut best: Option<(usize, &'static str)> = None;
    let mut consider = |candidate: &'static str, target: &'static str| {
        let d = distance(&wanted, candidate);
        // Words in common count as close: "add_door" and "door".
        let shared = wanted.split('_').any(|w| w.len() > 2 && candidate.split('_').any(|c| c == w));
        let score = if shared { d.min(3) } else { d };
        if best.is_none_or(|(b, _)| score < b) {
            best = Some((score, target));
        }
    };
    for v2 in all_v2() {
        consider(v2, v2);
    }
    for (old, new) in ALIASES {
        consider(old, new);
    }
    best.filter(|(d, _)| *d <= wanted.len().max(4) / 2 + 2).map(|(_, n)| n)
}

/// Levenshtein distance, for "did you mean".
pub fn distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut cur = vec![i + 1; b.len() + 1];
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            cur[j + 1] = (prev[j] + cost).min(prev[j + 1] + 1).min(cur[j] + 1);
        }
        prev = cur;
    }
    prev[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toolsets_parse_and_list() {
        assert_eq!(Toolsets::core().tools().len(), 26);
        assert_eq!(Toolsets::all().tools().len(), 45);
        let t = Toolsets::parse("visuals, services,nope");
        assert_eq!(t.enabled(), &["services", "visuals"]);
        assert_eq!(t.tools().len(), 26 + 3 + 3);
        assert_eq!(Toolsets::parse("documents,tours").tools().len(), 26 + 4 + 3);
        assert_eq!(Toolsets::from_query("a=1&toolsets=live%2Creview"), Some(Toolsets::parse("live,review")));
        assert_eq!(Toolsets::from_query("a=1"), None);
        assert_eq!(Toolsets::parse("all"), Toolsets::all());
    }

    #[test]
    fn every_v2_tool_is_in_exactly_one_toolset() {
        let all = all_v2();
        let mut sorted = all.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), all.len(), "a tool is in two toolsets");
        for t in all {
            assert!(toolset_of(t).is_some());
        }
    }

    #[test]
    fn every_alias_points_at_a_v2_tool() {
        let v2 = all_v2();
        for (old, new) in ALIASES {
            assert!(v2.contains(&new), "{old} maps to {new}, which is not a v2 tool");
        }
        assert_eq!(alias_of("add_rect_room"), Some("add_room"));
        assert_eq!(alias_of("batch"), None);
    }

    #[test]
    fn unknown_names_get_the_nearest_tool() {
        assert_eq!(nearest("add_wal"), Some("add_walls"));
        assert_eq!(nearest("add_stairs"), Some("add_stair"));
        assert_eq!(nearest("list_room"), Some("query"));
        assert_eq!(nearest("undo_last"), Some("history"));
        assert_eq!(nearest("render_views"), Some("render"));
        assert_eq!(nearest("xyzzy_qqq_zzzz_wwww"), None);
    }
}
