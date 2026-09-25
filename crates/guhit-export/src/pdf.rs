//! SVG sheet to vector PDF through usvg and svg2pdf. Fonts come from the
//! operating system through fontdb and are embedded (subset) in the PDF, so
//! the file reads the same on any machine.

use std::sync::{Arc, OnceLock};

use svg2pdf::usvg::{self, fontdb};

use crate::ExportError;

/// Sans-serif families to look for, in order. All are common on macOS,
/// Windows or Linux.
const PREFERRED: [&str; 10] = [
    "Helvetica",
    "Arial",
    "Helvetica Neue",
    "Segoe UI",
    "Liberation Sans",
    "DejaVu Sans",
    "Noto Sans",
    "Tahoma",
    "Verdana",
    "Roboto",
];

/// System font database, loaded once per process. Scanning the font folders
/// takes a noticeable fraction of a second.
fn system_fonts() -> Arc<fontdb::Database> {
    static DB: OnceLock<Arc<fontdb::Database>> = OnceLock::new();
    DB.get_or_init(|| {
        let mut db = fontdb::Database::new();
        db.load_system_fonts();
        if let Some(family) = pick_family(&db) {
            // Every generic family resolves to a face that really exists.
            db.set_sans_serif_family(family.clone());
            db.set_serif_family(family);
        }
        Arc::new(db)
    })
    .clone()
}

/// First preferred family that is installed, else the family of any face.
fn pick_family(db: &fontdb::Database) -> Option<String> {
    for name in PREFERRED {
        let query = fontdb::Query {
            families: &[fontdb::Family::Name(name)],
            ..fontdb::Query::default()
        };
        if db.query(&query).is_some() {
            return Some(name.to_string());
        }
    }
    db.faces()
        .find_map(|f| f.families.first().map(|(name, _)| name.clone()))
}

fn count_text_nodes(group: &usvg::Group) -> usize {
    group
        .children()
        .iter()
        .map(|node| match node {
            usvg::Node::Text(_) => 1,
            usvg::Node::Group(g) => count_text_nodes(g),
            _ => 0,
        })
        .sum()
}

/// Convert a sheet SVG (sized in mm) to a single page PDF of the same size.
pub fn svg_to_pdf(svg: &str) -> Result<Vec<u8>, ExportError> {
    svg_to_pdf_with(svg, system_fonts())
}

/// Same as `svg_to_pdf` with an explicit font database.
pub fn svg_to_pdf_with(svg: &str, db: Arc<fontdb::Database>) -> Result<Vec<u8>, ExportError> {
    if db.is_empty() {
        return Err(ExportError::Failed(
            "no fonts were found on this system, so the sheet text cannot be written to PDF. Install a sans-serif font such as Arial or DejaVu Sans, or export SVG instead".into(),
        ));
    }
    let options = usvg::Options {
        fontdb: db,
        ..usvg::Options::default()
    };
    let tree = usvg::Tree::from_str(svg, &options)
        .map_err(|e| ExportError::Failed(format!("could not read the sheet SVG: {e}")))?;

    // usvg drops a text element when no font can draw it. Never let that
    // pass silently: a sheet without its labels is a wrong sheet.
    let expected = svg.matches("<text ").count();
    let found = count_text_nodes(tree.root());
    if found < expected {
        return Err(ExportError::Failed(format!(
            "{} of {expected} text labels could not be drawn with the fonts on this system. Install a sans-serif font such as Arial or DejaVu Sans, or export SVG instead",
            expected - found
        )));
    }

    // usvg sizes are CSS pixels at 96 per inch. At dpi 96 svg2pdf maps them
    // to points so the page is the true paper size.
    let pdf = svg2pdf::to_pdf(
        &tree,
        svg2pdf::ConversionOptions::default(),
        svg2pdf::PageOptions { dpi: 96.0 },
    )
    .map_err(|e| ExportError::Failed(format!("PDF conversion failed: {e}")))?;
    Ok(pdf)
}

/// Several sheet SVGs as one PDF, one page each at its true paper size.
/// Each page is converted on its own and placed as a form XObject.
pub fn svgs_to_pdf(svgs: &[String]) -> Result<Vec<u8>, ExportError> {
    use pdf_writer::{Content, Finish, Name, Pdf, Rect, Ref, TextStr};
    let db = system_fonts();
    if db.is_empty() {
        return Err(ExportError::Failed(
            "no fonts were found on this system, so the sheet text cannot be written to PDF. Install a sans-serif font such as Arial or DejaVu Sans, or export SVG instead".into(),
        ));
    }
    let options = usvg::Options {
        fontdb: db,
        ..usvg::Options::default()
    };
    let mut alloc = Ref::new(1);
    let catalog = alloc.bump();
    let tree_id = alloc.bump();
    let mut pdf = Pdf::new();
    let mut page_ids = Vec::new();
    for svg in svgs {
        let tree = usvg::Tree::from_str(svg, &options)
            .map_err(|e| ExportError::Failed(format!("could not read the sheet SVG: {e}")))?;
        let expected = svg.matches("<text ").count();
        let found = count_text_nodes(tree.root());
        if found < expected {
            return Err(ExportError::Failed(format!(
                "{} of {expected} text labels could not be drawn with the fonts on this system. Install a sans-serif font such as Arial or DejaVu Sans, or export SVG instead",
                expected - found
            )));
        }
        let (chunk, xobject) = svg2pdf::to_chunk(&tree, svg2pdf::ConversionOptions::default())
            .map_err(|e| ExportError::Failed(format!("PDF conversion failed: {e}")))?;
        let mut map = std::collections::HashMap::new();
        let chunk = chunk.renumber(|old| *map.entry(old).or_insert_with(|| alloc.bump()));
        let xobject = map
            .get(&xobject)
            .copied()
            .ok_or_else(|| ExportError::Failed("PDF conversion lost the page content".into()))?;
        // usvg sizes are CSS pixels at 96 per inch; PDF points are 72 per inch.
        let (w, h) = (tree.size().width() * 0.75, tree.size().height() * 0.75);
        let page_id = alloc.bump();
        let content_id = alloc.bump();
        page_ids.push(page_id);
        {
            let mut page = pdf.page(page_id);
            page.media_box(Rect::new(0.0, 0.0, w, h));
            page.parent(tree_id);
            page.contents(content_id);
            page.resources().x_objects().pair(Name(b"S1"), xobject);
            page.finish();
        }
        let mut content = Content::new();
        content.transform([w, 0.0, 0.0, h, 0.0, 0.0]);
        content.x_object(Name(b"S1"));
        pdf.stream(content_id, &content.finish());
        pdf.extend(&chunk);
    }
    pdf.catalog(catalog).pages(tree_id);
    pdf.pages(tree_id).kids(page_ids.iter().copied()).count(page_ids.len() as i32);
    pdf.document_info(alloc.bump()).producer(TextStr("Guhit Studio"));
    Ok(pdf.finish())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_fonts_is_a_clear_error() {
        let svg = "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"10mm\" height=\"10mm\" viewBox=\"0 0 10 10\"><text x=\"1\" y=\"5\" font-size=\"3\">A</text></svg>";
        let err = svg_to_pdf_with(svg, Arc::new(fontdb::Database::new())).unwrap_err();
        match err {
            ExportError::Failed(m) => assert!(m.contains("no fonts"), "{m}"),
            other => panic!("unexpected {other:?}"),
        }
    }
}
