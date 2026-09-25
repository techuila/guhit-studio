//! Text helpers: width estimates for layout without a font engine, and
//! cleanup of user text before it is drawn.

/// Replace characters that must never reach a sheet. House rule: no em
/// dashes or en dashes, a plain hyphen is used instead. Control characters
/// become spaces.
pub fn clean(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            '\u{2012}' | '\u{2013}' | '\u{2014}' | '\u{2015}' | '\u{2212}' => '-',
            '\u{00a0}' => ' ',
            c if c.is_control() => ' ',
            c => c,
        })
        .collect::<String>()
        .trim()
        .to_string()
}

/// Rough advance width of one character as a fraction of the font size,
/// modelled on Helvetica. Good enough to fit labels into boxes.
fn char_em(c: char) -> f64 {
    match c {
        'i' | 'j' | 'l' | '.' | ',' | ':' | ';' | '\'' | '|' | '!' | 'I' => 0.28,
        ' ' | 'f' | 't' | 'r' | '/' | '(' | ')' | '[' | ']' | '-' => 0.33,
        'm' | 'w' | 'M' | 'W' | '@' | '%' => 0.86,
        '0'..='9' => 0.56,
        'A'..='Z' => 0.69,
        '\u{00b2}' => 0.36,
        _ => 0.54,
    }
}

/// Estimated width of `text` at the given font size (same unit as the size).
pub fn est_width(text: &str, size: f64, bold: bool) -> f64 {
    let em: f64 = text.chars().map(char_em).sum();
    em * size * if bold { 1.06 } else { 1.0 }
}

/// Font size (not above `size`, not below `min_size`) and text that fit into
/// `max_width`. When even the minimum size overflows, the text is cut and
/// ends with "...".
pub fn fit(text: &str, size: f64, min_size: f64, max_width: f64, bold: bool) -> (String, f64) {
    let w = est_width(text, size, bold);
    if w <= max_width || text.is_empty() {
        return (text.to_string(), size);
    }
    let shrunk = size * max_width / w;
    if shrunk >= min_size {
        return (text.to_string(), shrunk);
    }
    let mut chars: Vec<char> = text.chars().collect();
    while chars.len() > 1 {
        chars.pop();
        let candidate: String = chars.iter().collect::<String>().trim_end().to_string() + "...";
        if est_width(&candidate, min_size, bold) <= max_width {
            return (candidate, min_size);
        }
    }
    (text.chars().take(1).collect(), min_size)
}

/// Format a model length for a dimension: whole millimeters, or meters with
/// two decimals.
pub fn format_length(mm: f64, meters: bool) -> String {
    if meters {
        format!("{:.2}", mm / 1000.0)
    } else {
        format!("{}", mm.round() as i64)
    }
}

/// Escape text for XML content and attribute values.
pub fn xml_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_replaces_dashes() {
        assert_eq!(clean("Living \u{2014} Dining \u{2013} A"), "Living - Dining - A");
        assert_eq!(clean("  a\tb\n"), "a b");
    }

    #[test]
    fn fit_shrinks_then_truncates() {
        let (t, s) = fit("Short", 3.0, 1.5, 100.0, false);
        assert_eq!((t.as_str(), s), ("Short", 3.0));
        let (t, s) = fit("A fairly long project name", 3.0, 1.5, 30.0, false);
        assert_eq!(t, "A fairly long project name");
        assert!((1.5..3.0).contains(&s));
        let (t, s) = fit("A fairly long project name that goes on", 3.0, 2.5, 20.0, false);
        assert!(t.ends_with("..."));
        assert_eq!(s, 2.5);
        assert!(est_width(&t, s, false) <= 20.0);
    }

    #[test]
    fn lengths() {
        assert_eq!(format_length(8000.0, false), "8000");
        assert_eq!(format_length(8000.0, true), "8.00");
        assert_eq!(format_length(1234.6, false), "1235");
    }
}
