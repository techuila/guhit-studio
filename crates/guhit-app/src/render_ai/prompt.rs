//! The text sent with the capture.
//!
//! Geometry is authoritative and AI imagery is derivative (AGENTS.md), so the
//! prompt's whole job is to stop the image model from redesigning the
//! building. The preamble comes first and says what the input is and what may
//! change; the style fragment and the architect's own words follow.

use guhit_model::{defaults, IpcError};

/// Always first. Names the input for what it is and fences off the geometry.
pub const PREAMBLE: &str = "This image is a rendering of an architectural 3D model. Produce a photorealistic architectural photograph of exactly this building. Keep the camera, composition, every wall, opening, roof line and proportion exactly as shown. Do not add or remove doors, windows or rooms. Change only materials, lighting, landscaping and atmosphere.";

/// Added last when `keep_geometry` is set, which is the default.
pub const GEOMETRY_LINE: &str = "Geometry must match the input exactly.";

/// Longest free text accepted from the user. Well under any model limit; the
/// point is to keep one runaway paste out of the log and the record.
pub const MAX_USER_TEXT: usize = 2000;

/// What one AI render's text is made of (AI render v2, DECISIONS D53).
#[derive(Debug, Clone, Default)]
pub struct Parts<'a> {
    pub style_key: Option<&'a str>,
    pub user_text: &'a str,
    pub keep_geometry: bool,
    /// Model facts from Rust (`facts::describe`), already worded.
    pub facts: Option<&'a str>,
    /// The images sent after the capture, in order: "lines", "depth",
    /// "normal", "style". The text names each one so the model knows what
    /// it is looking at.
    pub images: &'a [&'a str],
}

/// What each extra image is, for the model.
fn image_line(n: usize, role: &str) -> Option<String> {
    let text = match role {
        "lines" => "is the model's line drawing from the same camera. Every line is a real edge of the building: keep each one where it is",
        "depth" => "is the model's depth from the same camera: lighter is nearer",
        "normal" => "shows which way each surface of the model faces, from the same camera",
        "style" => "is the project look, an image the architect approved: match its materials, colors, planting, light and mood. Never copy its shapes, layout or camera",
        _ => return None,
    };
    Some(format!("Image {n} {text}."))
}

/// The image guide: which image is which. Empty with no extra images.
fn image_guide(images: &[&str]) -> String {
    if images.is_empty() {
        return String::new();
    }
    let mut lines = vec!["Image 1 is the model view to render.".to_string()];
    lines.extend(images.iter().enumerate().filter_map(|(i, role)| image_line(i + 2, role)));
    lines.join(" ")
}

/// Assemble the full text. The result is what the provider receives and what
/// the `RenderRecord` stores, so the architect can always read back exactly
/// what produced an image.
///
/// Order: preamble, style prompt, user text, geometry line. Blank parts are
/// left out. An unknown `style_key` is an error rather than a silent skip; a
/// missing style would change the image without anything saying so.
pub fn build(style_key: Option<&str>, user_text: &str, keep_geometry: bool) -> Result<String, IpcError> {
    build_with(&Parts { style_key, user_text, keep_geometry, facts: None, images: &[] })
}

/// `build` with the image guide and the model facts: preamble, image guide,
/// model facts, style prompt, user text, geometry line.
pub fn build_with(p: &Parts) -> Result<String, IpcError> {
    let (style_key, user_text, keep_geometry) = (p.style_key, p.user_text, p.keep_geometry);
    let mut parts: Vec<String> = vec![PREAMBLE.to_string()];
    let guide = image_guide(p.images);
    if !guide.is_empty() {
        parts.push(guide);
    }
    if let Some(facts) = p.facts.map(str::trim).filter(|f| !f.is_empty()) {
        parts.push(facts.to_string());
    }

    if let Some(key) = style_key.map(str::trim).filter(|k| !k.is_empty()) {
        let style = defaults::render_styles()
            .into_iter()
            .find(|s| s.key == key)
            .ok_or_else(|| IpcError::new("invalid", format!("unknown render style `{key}`")))?;
        parts.push(style.prompt);
    }

    let user_text = user_text.trim();
    if user_text.chars().count() > MAX_USER_TEXT {
        return Err(IpcError::new(
            "invalid",
            format!("the prompt is longer than {MAX_USER_TEXT} characters"),
        ));
    }
    if !user_text.is_empty() {
        parts.push(user_text.to_string());
    }

    if keep_geometry {
        parts.push(GEOMETRY_LINE.to_string());
    }

    Ok(parts.join("\n\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preamble_is_always_first() {
        let text = build(None, "", false).expect("builds");
        assert_eq!(text, PREAMBLE);
        assert!(text.starts_with("This image is a rendering of an architectural 3D model."));
    }

    #[test]
    fn style_user_text_and_geometry_line_come_in_order() {
        let text = build(Some("tropical-modern"), "afternoon light, red roof", true).expect("builds");
        let preamble_at = text.find(PREAMBLE).expect("preamble");
        let style_at = text.find("tropical modern Philippine residence").expect("style");
        let user_at = text.find("afternoon light, red roof").expect("user text");
        let geometry_at = text.find(GEOMETRY_LINE).expect("geometry line");
        assert!(preamble_at < style_at, "preamble before style");
        assert!(style_at < user_at, "style before user text");
        assert!(user_at < geometry_at, "user text before the geometry line");
        assert!(text.ends_with(GEOMETRY_LINE));
    }

    #[test]
    fn geometry_line_is_left_out_when_not_asked_for() {
        let text = build(Some("modern-minimal"), "", false).expect("builds");
        assert!(!text.contains(GEOMETRY_LINE));
        assert!(text.contains("minimalist modern house"));
    }

    #[test]
    fn every_built_in_style_resolves() {
        for style in defaults::render_styles() {
            let text = build(Some(&style.key), "", true).expect("builds");
            assert!(text.contains(&style.prompt), "style {} is in the prompt", style.key);
        }
    }

    #[test]
    fn an_unknown_style_is_rejected() {
        let e = build(Some("brutalist-moon-base"), "", true).expect_err("unknown style");
        assert_eq!(e.code, "invalid");
        assert!(e.message.contains("brutalist-moon-base"));
    }

    #[test]
    fn user_text_is_trimmed_and_bounded() {
        let text = build(None, "   spaced out   ", false).expect("builds");
        assert!(text.ends_with("spaced out"));
        // Whitespace-only text adds nothing.
        assert_eq!(build(None, "   \n  ", false).expect("builds"), PREAMBLE);
        let e = build(None, &"x".repeat(MAX_USER_TEXT + 1), false).expect_err("too long");
        assert_eq!(e.code, "invalid");
    }

    #[test]
    fn the_image_guide_and_facts_come_before_the_style() {
        let facts = "Model facts:\n- Do not move, add or remove walls, doors or windows.";
        let text = build_with(&Parts {
            style_key: Some("modern-minimal"),
            user_text: "warm light",
            keep_geometry: true,
            facts: Some(facts),
            images: &["lines", "depth", "style"],
        })
        .expect("builds");
        let guide_at = text.find("Image 1 is the model view").expect("guide");
        assert!(text.contains("Image 2 is the model's line drawing"));
        assert!(text.contains("Image 3 is the model's depth"));
        assert!(text.contains("Image 4 is the project look"));
        let facts_at = text.find("Model facts").expect("facts");
        let style_at = text.find("minimalist modern house").expect("style");
        assert!(text.find(PREAMBLE) < Some(guide_at));
        assert!(guide_at < facts_at && facts_at < style_at);
        assert!(text.ends_with(GEOMETRY_LINE));
    }

    #[test]
    fn without_extra_images_there_is_no_guide() {
        let text = build_with(&Parts { keep_geometry: false, ..Default::default() }).expect("builds");
        assert_eq!(text, PREAMBLE);
    }

    #[test]
    fn parts_are_separated_by_blank_lines() {
        let text = build(Some("dusk-exterior"), "warm lights", true).expect("builds");
        assert_eq!(text.split("\n\n").count(), 4);
    }
}
