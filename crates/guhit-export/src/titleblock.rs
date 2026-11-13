//! The PH title block on every sheet of the set (IRR of PD 1096, Figure
//! III.2): owner (`ProjectSettings::client_name`), project, location
//! (`ProjectSettings::location`), lot, block and TCT; the signing architect
//! with PRC, PTR, IAPOA and TIN; firm and logo; the signature and dry seal
//! box, left blank; the revision table; the sheet number. It prints what was
//! typed and never says a set complies. LANE L4 owns this module.
//!
//! It is a strip down the right edge of the sheet, inside the border, the
//! same on every paper size and orientation, scaled with the paper's type
//! size `k`. From the top: firm, project, revisions (which take the height
//! that is left), architect, signature and dry seal, sheet contents, sheet
//! number. A field that was never typed prints blank, ready to fill by hand.
//! License numbers print exactly as typed (they shrink to fit, never cut).

use guhit_model::{Orientation, PaperSize, Project, SheetRevision, TitleBlock};

use crate::sheet::{num, paper_name, Svg, INK, MUTED};
use crate::text::{clean, est_width, fit, xml_escape};

/// Width of the title block strip, paper mm.
pub fn strip_width(k: f64) -> f64 {
    66.0 * k
}

/// What one sheet's block says besides the project's `TitleBlock`.
pub struct SheetInfo<'a> {
    /// "A-1".
    pub number: &'a str,
    pub title: &'a str,
    /// 1-based place in the set, and the number of sheets in it.
    pub index: usize,
    pub total: usize,
    /// "1:100", "AS SHOWN" or "NTS".
    pub scale: String,
    /// "YYYY-MM-DD", or empty.
    pub date: String,
    pub paper: PaperSize,
    pub orientation: Orientation,
    /// The revisions marked on this sheet, in set order.
    pub revisions: Vec<&'a SheetRevision>,
    /// A services sheet's signing professional ("MASTER PLUMBER"): the
    /// block is theirs and blank, never the architect's license numbers.
    pub discipline: Option<&'a str>,
}

/// A logo to draw: its MIME type and the bytes as base64.
pub struct Logo<'a> {
    pub mime: &'a str,
    pub base64: &'a str,
}

/// Light rules inside the block.
const RULE: &str = "#b9c2cd";

/// Draw the block in `rect` (x, y, width, height; SVG y down). `tb` is the
/// project's title block, already masked when the picture goes to an agent.
pub(crate) fn draw(
    svg: &mut Svg,
    project: &Project,
    tb: &TitleBlock,
    info: &SheetInfo,
    rect: (f64, f64, f64, f64),
    k: f64,
    logo: Option<Logo>,
) {
    let (x0, y0, w, h) = rect;
    let pad = 2.0 * k;
    let inner = w - 2.0 * pad;
    svg.s.push_str("<g id=\"title-block\">\n");
    svg.rect(x0, y0, w, h, 0.35, "#ffffff");

    let rule = |svg: &mut Svg, y: f64| svg.line((x0, y), (x0 + w, y), 0.25);

    // ------------------------------------------------------------ firm
    let mut y = y0;
    let firm_h = 24.0 * k;
    {
        let mut tx = x0 + pad;
        let mut tw = inner;
        if let Some(logo) = logo {
            let s = firm_h - 2.0 * pad;
            svg.s.push_str(&format!(
                "<image id=\"firm-logo\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" preserveAspectRatio=\"xMidYMid meet\" href=\"data:{};base64,{}\"/>\n",
                num(x0 + pad),
                num(y + pad),
                num(s),
                num(s),
                logo.mime,
                logo.base64
            ));
            tx += s + pad;
            tw -= s + pad;
        }
        caption(svg, tx, y + 3.0 * k, k, "FIRM");
        let name = clean(&tb.firm_name);
        let mut ty = y + 3.0 * k;
        for line in lines(&name, 2.6 * k, tw, 2, true) {
            ty += 3.1 * k;
            text(svg, tx, ty, &line.0, line.1, true, "firm-name");
        }
        let addr = clean(&tb.firm_address);
        for line in lines(&addr, 1.7 * k, tw, 3, false) {
            ty += 2.3 * k;
            text(svg, tx, ty, &line.0, line.1, false, "firm-address");
        }
    }
    y += firm_h;
    rule(svg, y);

    // --------------------------------------------------------- project
    {
        let title = if tb.project_title.trim().is_empty() { &project.name } else { &tb.project_title };
        caption(svg, x0 + pad, y + 3.0 * k, k, "PROJECT");
        let mut ty = y + 3.0 * k;
        for line in lines(&clean(title), 2.8 * k, inner, 2, true) {
            ty += 3.3 * k;
            text(svg, x0 + pad, ty, &line.0, line.1, true, "project-title");
        }
        y = ty + 1.6 * k;
        y += cells(svg, x0, y, w, k, &[("OWNER", &project.settings.client_name, "owner")], 2.0 * k);
        let location = clean(&project.settings.location);
        caption(svg, x0 + pad, y + 2.4 * k, k, "LOCATION");
        let mut ty = y + 2.4 * k;
        let loc_lines = lines(&location, 2.0 * k, inner, 2, false);
        if loc_lines.is_empty() {
            ty += 3.0 * k;
        }
        for line in loc_lines {
            ty += 2.6 * k;
            text(svg, x0 + pad, ty, &line.0, line.1, false, "location");
        }
        y = ty + 1.2 * k;
        svg.line((x0, y), (x0 + w, y), 0.13);
        y += cells(
            svg,
            x0,
            y,
            w,
            k,
            &[("LOT NO.", &tb.lot_no, "lot-no"), ("BLOCK NO.", &tb.block_no, "block-no"), ("TCT NO.", &tb.tct_no, "tct-no")],
            2.0 * k,
        );
    }
    rule(svg, y);
    let revisions_top = y;

    // Everything below the revisions, from the bottom up.
    let number_h = 24.0 * k;
    let contents_h = 23.0 * k;
    let architect_h = 39.0 * k;
    let below = number_h + contents_h + architect_h;
    let min_revisions = 20.0 * k;
    let seal_h = (y0 + h - revisions_top - below - min_revisions).clamp(20.0 * k, 36.0 * k);
    let architect_top = y0 + h - below - seal_h;
    // The revision table takes up to twelve rows (more when the sheet has
    // more revisions); what is left on tall paper is a blank remarks box.
    let rows = info.revisions.len().max(12) as f64;
    let revisions_h = (architect_top - revisions_top).min(4.6 * k + (rows + 1.0) * 4.2 * k + 1.0 * k);

    // ------------------------------------------------------- revisions
    revisions(svg, x0, revisions_top, w, revisions_h, k, &info.revisions);
    if architect_top - revisions_top - revisions_h > 12.0 * k {
        let top = revisions_top + revisions_h;
        rule(svg, top);
        caption(svg, x0 + pad, top + 3.0 * k, k, "REMARKS");
    }
    rule(svg, architect_top);

    // ------------------------------------------------------- architect
    // A services sheet (D19) carries its own trade's block, blank.
    let blank = guhit_model::Signatory::default();
    let sig = if info.discipline.is_some() { &blank } else { &tb.signatory };
    let mut y = architect_top;
    caption(svg, x0 + pad, y + 3.0 * k, k, info.discipline.unwrap_or("ARCHITECT"));
    let (name, size) = shrink(&clean(&sig.name), 2.6 * k, inner, true);
    text(svg, x0 + pad, y + 7.0 * k, &name, size, true, "signatory-name");
    y += 9.0 * k;
    svg.line((x0, y), (x0 + w, y), 0.13);
    let rows: [&[(&str, &String, &str)]; 4] = [
        &[("PRC REG. NO.", &sig.prc_no, "prc-no"), ("VALID UNTIL", &sig.prc_valid_until, "prc-valid-until")],
        &[("PTR NO.", &sig.ptr_no, "ptr-no"), ("DATE ISSUED", &sig.ptr_date, "ptr-date"), ("PLACE ISSUED", &sig.ptr_place, "ptr-place")],
        &[(if info.discipline.is_some() { "ACCREDITED ORG. NO." } else { "IAPOA NO." }, &sig.iapoa_no, "iapoa-no"), ("O.R. NO.", &sig.iapoa_or_no, "iapoa-or-no"), ("DATE ISSUED", &sig.iapoa_date, "iapoa-date")],
        &[("TIN", &sig.tin, "tin")],
    ];
    for row in rows {
        y += cells(svg, x0, y, w, k, row, 1.9 * k);
        svg.line((x0, y), (x0 + w, y), 0.13);
    }
    let seal_top = architect_top + architect_h;
    rule(svg, seal_top);

    // ------------------------------------------- signature and dry seal
    caption(svg, x0 + pad, seal_top + 3.0 * k, k, "SIGNATURE AND DRY SEAL");
    let sign_y = seal_top + seal_h - 5.5 * k;
    svg.s.push_str(&format!(
        "<line id=\"signature-line\" x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"{MUTED}\" stroke-width=\"0.13\"/>\n",
        num(x0 + 8.0 * k),
        num(sign_y),
        num(x0 + w - 8.0 * k),
        num(sign_y)
    ));
    // The printed name the architect signs over (the IRR's "signed and
    // sealed over the printed names").
    let (printed, ps) = shrink(&clean(&sig.name).to_uppercase(), 2.0 * k, w - 16.0 * k, true);
    if !printed.is_empty() {
        svg.s.push_str(&format!(
            "<text class=\"tb-printed-name\" x=\"{}\" y=\"{}\" font-size=\"{}\" text-anchor=\"middle\" font-weight=\"bold\" fill=\"{INK}\">{}</text>\n",
            num(x0 + w / 2.0),
            num(sign_y - 0.8 * k),
            num(ps),
            xml_escape(&printed)
        ));
    }
    svg.text(x0 + w / 2.0, sign_y + 2.6 * k, 1.35 * k, "middle", false, MUTED, "SIGNATURE OVER PRINTED NAME");
    let contents_top = seal_top + seal_h;
    rule(svg, contents_top);

    // -------------------------------------------------- sheet contents
    caption(svg, x0 + pad, contents_top + 3.0 * k, k, "SHEET CONTENTS");
    let mut ty = contents_top + 3.0 * k;
    for line in lines(&clean(info.title).to_uppercase(), 2.4 * k, inner, 2, true) {
        ty += 2.9 * k;
        text(svg, x0 + pad, ty, &line.0, line.1, true, "sheet-title");
    }
    let row_y = contents_top + contents_h - 9.0 * k;
    svg.line((x0, row_y), (x0 + w, row_y), 0.13);
    let paper = format!(
        "{} {}",
        paper_name(info.paper),
        match info.orientation {
            Orientation::Landscape => "LANDSCAPE",
            Orientation::Portrait => "PORTRAIT",
        }
    );
    cells(
        svg,
        x0,
        row_y,
        w,
        k,
        &[("SCALE", &info.scale, "scale"), ("DATE", &info.date, "date"), ("PAPER", &paper, "paper")],
        1.9 * k,
    );
    let number_top = contents_top + contents_h;
    rule(svg, number_top);

    // ----------------------------------------------------- sheet number
    caption(svg, x0 + pad, number_top + 3.0 * k, k, "SHEET NO.");
    let (number, size) = shrink(&clean(info.number), 9.0 * k, inner * 0.62, true);
    text(svg, x0 + pad, number_top + 14.5 * k, &number, size, true, "sheet-number");
    let of = format!("{} OF {}", info.index, info.total);
    svg.s.push_str(&format!(
        "<text id=\"sheet-of\" x=\"{}\" y=\"{}\" font-size=\"{}\" text-anchor=\"end\" font-weight=\"bold\" fill=\"{INK}\">{}</text>\n",
        num(x0 + w - pad),
        num(number_top + 14.5 * k),
        num(2.6 * k),
        xml_escape(&of)
    ));
    svg.text(x0 + w - pad, number_top + 18.0 * k, 1.35 * k, "end", false, MUTED, "SHEETS");
    let (brand, bs) = fit("Drawn in Guhit Studio", 1.3 * k, 1.0 * k, inner, false);
    svg.text(x0 + pad, y0 + h - 2.0 * k, bs, "start", false, MUTED, &brand);
    svg.s.push_str("</g>\n");
}

/// A small caption over a field.
fn caption(svg: &mut Svg, x: f64, y: f64, k: f64, label: &str) {
    svg.text(x, y, 1.35 * k, "start", false, MUTED, label);
}

/// A value with an id, so tests and readers find each field.
fn text(svg: &mut Svg, x: f64, y: f64, value: &str, size: f64, bold: bool, id: &str) {
    if value.is_empty() {
        return;
    }
    svg.s.push_str(&format!(
        "<text class=\"tb-{id}\" x=\"{}\" y=\"{}\" font-size=\"{}\" text-anchor=\"start\"{} fill=\"{INK}\">{}</text>\n",
        num(x),
        num(y),
        num(size),
        if bold { " font-weight=\"bold\"" } else { "" },
        xml_escape(value)
    ));
}

/// `value` at `size`, smaller when it is wider than `width`, never cut:
/// license numbers print exactly as typed.
fn shrink(value: &str, size: f64, width: f64, bold: bool) -> (String, f64) {
    let w = est_width(value, size, bold);
    if w <= width || w <= 0.0 {
        (value.to_string(), size)
    } else {
        (value.to_string(), size * width / w)
    }
}

/// Up to `max` lines of `value` wrapped to `width`; the last line shrinks to
/// take the rest.
fn lines(value: &str, size: f64, width: f64, max: usize, bold: bool) -> Vec<(String, f64)> {
    let words: Vec<String> = crate::panel::wrap(value, size * if bold { 1.06 } else { 1.0 }, width);
    let mut out: Vec<(String, f64)> = Vec::new();
    for (i, l) in words.iter().enumerate() {
        if i + 1 == max {
            let rest = words[i..].join(" ");
            out.push(shrink(&rest, size, width, bold));
            break;
        }
        out.push((l.clone(), size));
    }
    out
}

/// One row of captioned cells across the block, widths by share. Returns its
/// height.
fn cells(svg: &mut Svg, x0: f64, y: f64, w: f64, k: f64, row: &[(&str, &String, &str)], size: f64) -> f64 {
    let pad = 2.0 * k;
    let h = 2.4 * k + 1.0 * k + size + 1.3 * k;
    let cw = w / row.len() as f64;
    for (i, (label, value, id)) in row.iter().enumerate() {
        let x = x0 + cw * i as f64;
        if i > 0 {
            svg.line((x, y), (x, y + h), 0.13);
        }
        caption(svg, x + pad, y + 2.4 * k, k, label);
        let (v, s) = shrink(&clean(value), size, cw - 2.0 * pad, false);
        text(svg, x + pad, y + 2.4 * k + 1.0 * k + size, &v, s, false, id);
    }
    h
}

/// The revision table: REV., DATE, DESCRIPTION, with ruled rows down to the
/// architect's block so later revisions can be written in.
fn revisions(svg: &mut Svg, x0: f64, y0: f64, w: f64, h: f64, k: f64, list: &[&SheetRevision]) {
    let pad = 2.0 * k;
    caption(svg, x0 + pad, y0 + 3.0 * k, k, "REVISIONS");
    let head_y = y0 + 4.6 * k;
    let row_h = 4.2 * k;
    let cols = [0.0, 0.16 * w, 0.4 * w];
    let heads = ["REV.", "DATE", "DESCRIPTION"];
    for (i, head) in heads.iter().enumerate() {
        svg.text(x0 + cols[i] + pad, head_y + 2.9 * k, 1.3 * k, "start", true, INK, head);
    }
    let mut y = head_y + row_h;
    svg.line((x0, y), (x0 + w, y), 0.18);
    let bottom = y0 + h - 1.0 * k;
    let mut i = 0;
    while y + row_h <= bottom + 1e-6 {
        if let Some(r) = list.get(i) {
            let size = 1.7 * k;
            let cells = [clean(&r.mark), clean(&r.date), clean(&r.description)];
            for (c, value) in cells.iter().enumerate() {
                let right = if c + 1 < cols.len() { x0 + cols[c + 1] } else { x0 + w };
                let (v, s) = fit(value, size, 1.1 * k, right - x0 - cols[c] - 2.0 * pad, false);
                svg.s.push_str(&format!(
                    "<text class=\"tb-revision\" x=\"{}\" y=\"{}\" font-size=\"{}\" fill=\"{INK}\">{}</text>\n",
                    num(x0 + cols[c] + pad),
                    num(y + row_h / 2.0 + 0.36 * s),
                    num(s),
                    xml_escape(&v)
                ));
            }
        }
        y += row_h;
        svg.s.push_str(&format!(
            "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"{RULE}\" stroke-width=\"0.1\"/>\n",
            num(x0),
            num(y),
            num(x0 + w),
            num(y)
        ));
        i += 1;
    }
    for c in &cols[1..] {
        svg.s.push_str(&format!(
            "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"{RULE}\" stroke-width=\"0.1\"/>\n",
            num(x0 + c),
            num(head_y),
            num(x0 + c),
            num(y)
        ));
    }
    if list.len() > i {
        // More revisions than rows: the rest are counted, never dropped silently.
        svg.text(x0 + w - pad, y0 + 3.0 * k, 1.35 * k, "end", false, MUTED, &format!("+ {} MORE", list.len() - i));
    }
}
