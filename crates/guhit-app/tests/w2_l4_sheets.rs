//! Lane L4, sheets and documents (DECISIONS D53, D54): sheet previews and
//! the one PDF set, the PH title block, files and the professional profile.

use std::time::Instant;

use guhit_app::AppService;
use guhit_export::{sheetset, SheetInputs};
use guhit_model::*;
use serde_json::{json, Value};

fn app() -> (AppService, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("temp dir");
    (AppService::new_sandboxed(dir.path().to_path_buf()), dir)
}

async fn call<T: serde::de::DeserializeOwned>(app: &AppService, cmd: &str, args: Value) -> T {
    let v = app.handle(cmd, args).await.unwrap_or_else(|e| panic!("{cmd} failed: {e}"));
    serde_json::from_value(v).unwrap_or_else(|e| panic!("{cmd}: {e}"))
}

async fn fail(app: &AppService, cmd: &str, args: Value) -> IpcError {
    match app.handle(cmd, args).await {
        Ok(v) => panic!("{cmd} should have failed, got {v}"),
        Err(e) => e,
    }
}

async fn bungalow(app: &AppService) -> DocState {
    call(app, "hub_create", json!({ "name": "Bahay", "template": "sample-bungalow" })).await
}

async fn apply(app: &AppService, command: Command) -> ApplyResult {
    call(app, "doc_apply", json!({ "command": command })).await
}

async fn state(app: &AppService) -> DocState {
    call(app, "doc_state", json!({})).await
}

fn vp(view: ViewRef) -> Viewport {
    Viewport { id: String::new(), view, scale_denominator: None, position: None, title: None }
}

fn sheet(title: &str, paper: PaperSize, orientation: Orientation, viewports: Vec<Viewport>) -> Sheet {
    Sheet { id: String::new(), number: String::new(), title: title.into(), paper, orientation, viewports, revision_ids: vec![] }
}

async fn add_sheet(app: &AppService, s: Sheet) -> Sheet {
    apply(app, Command::UpsertSheet { sheet: Box::new(s), index: None }).await;
    state(app).await.project.sheet_set.sheets.last().cloned().expect("a sheet")
}

fn signatory() -> Signatory {
    Signatory {
        name: "Arch. Maria Santos-Reyes".into(),
        prc_no: "0012345".into(),
        prc_valid_until: "2027-08-15".into(),
        ptr_no: "PTR-2026 / 7654321".into(),
        ptr_date: "2026-01-06".into(),
        ptr_place: "Quezon City".into(),
        iapoa_no: "IAPOA 18-0456".into(),
        iapoa_or_no: "OR 990011".into(),
        iapoa_date: "2026-01-10".into(),
        tin: "123-456-789-000".into(),
    }
}

/// The text of the title block field with class `tb-<id>`, if printed.
fn field(svg: &str, id: &str) -> Option<String> {
    let tag = format!("class=\"tb-{id}\"");
    let at = svg.find(&tag)?;
    let rest = &svg[at..];
    let open = rest.find('>')? + 1;
    let close = rest[open..].find("</text>")?;
    Some(rest[open..open + close].to_string())
}

fn xml(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

// --------------------------------------------------------------- a PNG

fn crc32(bytes: &[u8]) -> u32 {
    let mut c = 0xffff_ffffu32;
    for b in bytes {
        c ^= *b as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xedb8_8320 ^ (c >> 1) } else { c >> 1 };
        }
    }
    !c
}

/// An RGB PNG with stored (uncompressed) deflate blocks: big and honest, the
/// size of a full HD render.
fn png(w: u32, h: u32) -> Vec<u8> {
    let mut raw = Vec::with_capacity(((w * 3 + 1) * h) as usize);
    for y in 0..h {
        raw.push(0);
        for x in 0..w {
            let n = (x.wrapping_mul(2654435761) ^ y.wrapping_mul(40503)) >> 13;
            raw.extend_from_slice(&[(x * 255 / w) as u8, (y * 255 / h) as u8, (n & 0x3f) as u8 + 120]);
        }
    }
    let mut z = vec![0x78, 0x01];
    for (i, chunk) in raw.chunks(65535).enumerate() {
        let last = (i + 1) * 65535 >= raw.len();
        z.push(last as u8);
        z.extend_from_slice(&(chunk.len() as u16).to_le_bytes());
        z.extend_from_slice(&(!(chunk.len() as u16)).to_le_bytes());
        z.extend_from_slice(chunk);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for x in &raw {
        a = (a + *x as u32) % 65521;
        b = (b + a) % 65521;
    }
    z.extend_from_slice(&((b << 16) | a).to_be_bytes());
    let mut out = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    let mut chunk = |kind: &[u8], data: &[u8]| {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let mut body = kind.to_vec();
        body.extend_from_slice(data);
        out.extend_from_slice(&body);
        out.extend_from_slice(&crc32(&body).to_be_bytes());
    };
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&w.to_be_bytes());
    ihdr.extend_from_slice(&h.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    chunk(b"IHDR", &ihdr);
    chunk(b"IDAT", &z);
    chunk(b"IEND", &[]);
    out
}

fn data_url(bytes: &[u8]) -> String {
    format!("data:image/png;base64,{}", sheetset::base64(bytes))
}

fn pages(pdf: &[u8]) -> usize {
    let text = String::from_utf8_lossy(pdf);
    text.matches("/Type /Page\n").count().max(text.matches("/Type /Page ").count()).max(text.matches("/Type/Page/").count())
}

// ------------------------------------------------------------------ tests

#[tokio::test]
async fn sheets_and_documents_are_wired() {
    let (app, dir) = app();
    bungalow(&app).await;
    // Unknown sheet.
    let e = fail(&app, "sheet_preview", json!({ "sheet_id": "x" })).await;
    assert_eq!(e.code, "not_found", "{e}");
    // A project with no sheets still exports a set: the default one.
    let opts = SheetExportOptions { format: PlanFormat::Pdf, sheet_ids: vec![], path: None, date: None };
    let r: ExportResult = call(&app, "export_sheets", serde_json::to_value(&opts).unwrap()).await;
    assert!(r.path.ends_with(".pdf"), "{}", r.path);
    let bytes = std::fs::read(&r.path).unwrap();
    assert!(bytes.starts_with(b"%PDF"));
    // The bungalow: its ground floor plan, the roof plan and the schedules.
    let set = sheetset::default_set(&state(&app).await.project, PaperSize::A3);
    assert!(set.len() >= 3, "{set:?}");
    // The profile is this computer's, in settings.json.
    let profile: Signatory = call(&app, "professional_get", json!({})).await;
    assert!(profile.is_empty());
    let saved: Signatory = call(&app, "professional_set", json!({ "signatory": Signatory { name: "  Ana Cruz ".into(), ..signatory() } })).await;
    assert_eq!(saved.name, "Ana Cruz");
    let again: Signatory = call(&app, "professional_get", json!({})).await;
    assert_eq!(again, saved);
    let settings = std::fs::read_to_string(dir.path().join("settings.json")).unwrap();
    assert!(settings.contains("\"professional\""));
    // Clearing it removes the key.
    let _: Signatory = call(&app, "professional_set", json!({ "signatory": Signatory::default() })).await;
    let cleared: Signatory = call(&app, "professional_get", json!({})).await;
    assert!(cleared.is_empty());
}

#[tokio::test]
async fn previews_are_fresh_after_reopening_an_edited_project() {
    // GR1 #5: every open starts the revision at 0 again, so a picture cached
    // by revision came back from before the edit.
    let (app, _dir) = app();
    let st = bungalow(&app).await;
    let id = st.project.id.clone();
    let level = st.project.levels[0].id.clone();
    let s = add_sheet(&app, sheet("Ground floor plan", PaperSize::A3, Orientation::Landscape, vec![vp(ViewRef::Plan { level_id: level.clone(), sheet: SheetKind::Plan })])).await;
    let plan = json!({ "view": ViewRef::Plan { level_id: level, sheet: SheetKind::Plan } });

    let _: Value = call(&app, "hub_close", json!({})).await;
    let opened: DocState = call(&app, "hub_open", json!({ "id": id })).await;
    let revision = opened.revision;
    let before: SvgDrawing = call(&app, "view_svg", plan.clone()).await;
    let sheet_before: SvgDrawing = call(&app, "sheet_preview", json!({ "sheet_id": s.id })).await;

    let wall = opened.project.elements.iter().find(|e| matches!(e, Element::Wall(_))).unwrap().id().clone();
    apply(&app, Command::DeleteElements { ids: vec![wall] }).await;
    let edited: SvgDrawing = call(&app, "view_svg", plan.clone()).await;
    let sheet_edited: SvgDrawing = call(&app, "sheet_preview", json!({ "sheet_id": s.id })).await;
    assert_ne!(edited.svg, before.svg, "the wall is gone");

    // Closed and opened again: the same revision number as the first open.
    let _: Value = call(&app, "hub_close", json!({})).await;
    let again: DocState = call(&app, "hub_open", json!({ "id": id })).await;
    assert_eq!(again.revision, revision);
    let after: SvgDrawing = call(&app, "view_svg", plan.clone()).await;
    let sheet_after: SvgDrawing = call(&app, "sheet_preview", json!({ "sheet_id": s.id })).await;
    assert_eq!(after.svg, edited.svg, "the view is the edited plan, not the picture from before");
    assert_eq!(sheet_after.svg, sheet_edited.svg, "the sheet is the edited plan too");
    assert_ne!(sheet_after.svg, sheet_before.svg);

    // A rename keeps the revision but changes the sheet's title block.
    let _: Value = call(&app, "hub_rename", json!({ "id": id, "name": "Bahay ni Juan" })).await;
    let renamed: SvgDrawing = call(&app, "sheet_preview", json!({ "sheet_id": s.id })).await;
    assert_eq!(state(&app).await.revision, again.revision);
    assert!(renamed.svg.contains("BAHAY NI JUAN") || renamed.svg.contains("Bahay ni Juan"), "the new name prints");
}

#[tokio::test]
async fn signatory_fields_print_exactly_and_blank_when_unset() {
    let (app, _dir) = app();
    let st = bungalow(&app).await;
    let level = st.project.levels[0].id.clone();
    let s = add_sheet(&app, sheet("Ground floor plan", PaperSize::A3, Orientation::Landscape, vec![vp(ViewRef::Plan { level_id: level, sheet: SheetKind::Plan })])).await;
    assert_eq!(s.number, "A-1");

    let blank: SvgDrawing = call(&app, "sheet_preview", json!({ "sheet_id": s.id })).await;
    for id in ["signatory-name", "prc-no", "prc-valid-until", "ptr-no", "ptr-date", "ptr-place", "iapoa-no", "iapoa-or-no", "iapoa-date", "tin", "printed-name"] {
        assert_eq!(field(&blank.svg, id), None, "{id} prints although unset");
    }
    assert_eq!(field(&blank.svg, "sheet-number").as_deref(), Some("A-1"));
    assert!(blank.svg.contains("SIGNATURE AND DRY SEAL"));
    // Never a compliance claim.
    let lower = blank.svg.to_lowercase();
    for word in ["complies", "compliant", "approved", "certified"] {
        assert!(!lower.contains(word), "{word}");
    }

    let sig = Signatory { tin: String::new(), ..signatory() };
    apply(&app, Command::SetSignatory { signatory: Box::new(sig.clone()) }).await;
    let full: SvgDrawing = call(&app, "sheet_preview", json!({ "sheet_id": s.id })).await;
    let expect = [
        ("signatory-name", &sig.name),
        ("prc-no", &sig.prc_no),
        ("prc-valid-until", &sig.prc_valid_until),
        ("ptr-no", &sig.ptr_no),
        ("ptr-date", &sig.ptr_date),
        ("ptr-place", &sig.ptr_place),
        ("iapoa-no", &sig.iapoa_no),
        ("iapoa-or-no", &sig.iapoa_or_no),
        ("iapoa-date", &sig.iapoa_date),
    ];
    for (id, value) in expect {
        assert_eq!(field(&full.svg, id), Some(xml(value)), "{id}");
    }
    assert_eq!(field(&full.svg, "tin"), None, "an unset TIN stays blank");
    assert_eq!(field(&full.svg, "printed-name"), Some(xml(&sig.name.to_uppercase())));
}

#[tokio::test]
async fn masked_pictures_never_show_the_license_numbers() {
    let (app, _dir) = app();
    let st = bungalow(&app).await;
    let level = st.project.levels[0].id.clone();
    let s = add_sheet(&app, sheet("Plan", PaperSize::A1, Orientation::Landscape, vec![vp(ViewRef::Plan { level_id: level, sheet: SheetKind::Plan })])).await;
    let sig = signatory();
    apply(&app, Command::SetSignatory { signatory: Box::new(sig.clone()) }).await;
    let st = state(&app).await;
    let masked = SheetInputs { mask_signatory: true, ..Default::default() };
    let svg = guhit_export::sheet_svg(&st.project, &st.derived, &s.id, &masked).unwrap();
    for secret in [&sig.prc_no, &sig.ptr_no, &sig.iapoa_no, &sig.iapoa_or_no, &sig.tin] {
        assert!(!svg.contains(&xml(secret)), "{secret} shows on a masked sheet");
        // Not even its digits run together.
        let digits: String = secret.chars().filter(|c| c.is_ascii_digit()).collect();
        assert!(!svg.contains(&digits), "{digits} shows on a masked sheet");
    }
    assert_eq!(field(&svg, "prc-no").as_deref(), Some("*****45"));
    assert_eq!(field(&svg, "tin").as_deref(), Some("***-***-***-*00"));
    assert_eq!(field(&svg, "signatory-name"), Some(xml(&sig.name)), "the name is kept");
    // The PDF on disk and the window's preview print in full.
    let full: SvgDrawing = call(&app, "sheet_preview", json!({ "sheet_id": s.id })).await;
    assert_eq!(field(&full.svg, "tin"), Some(sig.tin.clone()));
}

#[tokio::test]
async fn no_viewport_overlaps_the_title_block_at_any_paper_size() {
    let (app, _dir) = app();
    let st = bungalow(&app).await;
    let level = st.project.levels[0].id.clone();
    let plan = || ViewRef::Plan { level_id: level.clone(), sheet: SheetKind::Plan };
    let at = |x: f64, y: f64| Some(Point { x, y });
    for paper in [PaperSize::A0, PaperSize::A1, PaperSize::A2, PaperSize::A3] {
        for orientation in [Orientation::Landscape, Orientation::Portrait] {
            let (w, h) = guhit_export::paper_size_mm(paper, orientation);
            let viewports = vec![
                // Automatic: laid out in order at the largest common scale.
                vp(plan()),
                vp(ViewRef::Schedule { schedule: ScheduleKind::DoorsWindows }),
                vp(ViewRef::Schedule { schedule: ScheduleKind::Areas }),
                vp(ViewRef::Notes { text: "All dimensions in millimeters.\nVerify on site.".into() }),
                // Placed on the title block, off the paper, and at a scale
                // far too large for the paper.
                Viewport { position: at(w - 30.0, h / 2.0), ..vp(plan()) },
                Viewport { position: at(-500.0, 5000.0), ..vp(ViewRef::Perspective { render_id: "none".into() }) },
                Viewport { position: at(w, 0.0), scale_denominator: Some(20), ..vp(plan()) },
            ];
            let s = add_sheet(&app, sheet("Test", paper, orientation, viewports)).await;
            let st = state(&app).await;
            let sh = st.project.sheet_set.sheets.iter().find(|x| x.id == s.id).unwrap();
            let (page, boxes) = sheetset::viewport_boxes(&st.project, &st.derived, sh, &Default::default());
            assert_eq!(boxes.len(), 7);
            let (tx, ty, tw, th) = page.title_block;
            let (ax, ay, aw, ah) = page.area;
            assert!(ax + aw <= tx, "{paper:?} {orientation:?}: the area reaches the title block");
            for b in &boxes {
                let (x, y, bw, bh) = b.frame;
                assert!(x >= ax - 1e-6 && y >= ay - 1e-6 && x + bw <= ax + aw + 1e-6 && y + bh <= ay + ah + 1e-6, "{paper:?} {orientation:?}: {b:?} leaves the area {:?}", page.area);
                let apart = x + bw <= tx || tx + tw <= x || y + bh <= ty || ty + th <= y;
                assert!(apart, "{paper:?} {orientation:?}: {b:?} overlaps the title block");
            }
            // And on paper: everything drawn is clipped to the area.
            let svg = guhit_export::sheet_svg(&st.project, &st.derived, &s.id, &Default::default()).unwrap();
            assert!(svg.contains("clip-path=\"url(#area-clip)\""));
            apply(&app, Command::DeleteSheets { ids: vec![s.id.clone()] }).await;
        }
    }
}

#[tokio::test]
async fn unplaced_viewports_share_the_largest_common_scale_that_fits() {
    let (app, _dir) = app();
    let st = bungalow(&app).await;
    let level = st.project.levels[0].id.clone();
    let plan = ViewRef::Plan { level_id: level.clone(), sheet: SheetKind::Plan };
    let a1 = add_sheet(&app, sheet("A1", PaperSize::A1, Orientation::Landscape, vec![vp(plan.clone()), vp(plan.clone())])).await;
    let a3 = add_sheet(&app, sheet("A3", PaperSize::A3, Orientation::Landscape, vec![vp(plan.clone()), vp(plan.clone())])).await;
    let fixed = add_sheet(&app, sheet("Fixed", PaperSize::A3, Orientation::Landscape, vec![Viewport { scale_denominator: Some(200), ..vp(plan.clone()) }])).await;
    let st = state(&app).await;
    let find = |id: &str| st.project.sheet_set.sheets.iter().find(|s| s.id == id).unwrap().clone();
    let (_, b1) = sheetset::viewport_boxes(&st.project, &st.derived, &find(&a1.id), &Default::default());
    let (_, b3) = sheetset::viewport_boxes(&st.project, &st.derived, &find(&a3.id), &Default::default());
    let (_, bf) = sheetset::viewport_boxes(&st.project, &st.derived, &find(&fixed.id), &Default::default());
    assert_eq!(b1[0].scale, b1[1].scale, "one common scale");
    assert_eq!(b3[0].scale, b3[1].scale);
    assert!(b1[0].scale.unwrap() < b3[0].scale.unwrap(), "A1 draws larger than A3: {:?} {:?}", b1[0].scale, b3[0].scale);
    assert_eq!(bf[0].scale, Some(200), "a stated scale is kept");
    let svg: SvgDrawing = call(&app, "sheet_preview", json!({ "sheet_id": a1.id })).await;
    assert!(svg.svg.contains(&format!("SCALE 1:{}", b1[0].scale.unwrap())));
    assert_eq!(field(&svg.svg, "scale"), Some(format!("1:{}", b1[0].scale.unwrap())));
}

#[tokio::test]
async fn files_are_stored_with_the_project_and_the_logo_prints() {
    let (app, _dir) = app();
    let st = bungalow(&app).await;
    let logo = png(64, 32);
    let stored: Value = call(&app, "file_store", json!({ "file_name": "firm logo.png", "data": data_url(&logo) })).await;
    let name = stored["file_name"].as_str().unwrap().to_string();
    assert!(name.ends_with(".png"), "{name}");
    let back: String = call(&app, "file_data", json!({ "file_name": name })).await;
    assert_eq!(back, data_url(&logo));
    assert_eq!(fail(&app, "file_data", json!({ "file_name": "../x.png" })).await.code, "invalid");
    let tb = TitleBlock { firm_name: "Santos Reyes Architects".into(), firm_logo: Some(name), lot_no: "12".into(), block_no: "4".into(), tct_no: "T-123456".into(), ..(*st.project.title_block).clone() };
    apply(&app, Command::SetTitleBlock { title_block: Box::new(tb) }).await;
    let level = st.project.levels[0].id.clone();
    let s = add_sheet(&app, sheet("Plan", PaperSize::A3, Orientation::Landscape, vec![vp(ViewRef::Plan { level_id: level, sheet: SheetKind::Plan })])).await;
    let svg: SvgDrawing = call(&app, "sheet_preview", json!({ "sheet_id": s.id })).await;
    assert!(svg.svg.contains("id=\"firm-logo\""));
    assert_eq!(field(&svg.svg, "firm-name").as_deref(), Some("Santos Reyes Architects"));
    assert_eq!(field(&svg.svg, "tct-no").as_deref(), Some("T-123456"));
}

#[tokio::test]
async fn the_set_exports_in_one_pdf_fast_and_small_with_a_perspective() {
    let (app, _dir) = app();
    let st = bungalow(&app).await;
    let level = st.project.levels[0].id.clone();
    // A full HD model render in Visuals.
    let camera = st.project.elements.iter().find_map(|e| match e {
        Element::Camera(c) => Some(c.clone()),
        _ => None,
    });
    let camera = camera.expect("the bungalow has a camera");
    let render: RenderRecord = call(&app, "render_capture", json!({ "camera": camera, "png": data_url(&png(1920, 1080)) })).await;
    let plan = ViewRef::Plan { level_id: level.clone(), sheet: SheetKind::Plan };
    add_sheet(&app, sheet("Perspective and plan", PaperSize::A1, Orientation::Landscape, vec![vp(ViewRef::Perspective { render_id: render.id.clone() }), vp(plan.clone())])).await;
    add_sheet(&app, sheet("Plan", PaperSize::A1, Orientation::Landscape, vec![vp(plan.clone())])).await;
    add_sheet(
        &app,
        sheet(
            "Schedules",
            PaperSize::A1,
            Orientation::Landscape,
            [ScheduleKind::DoorsWindows, ScheduleKind::Rooms, ScheduleKind::Finishes, ScheduleKind::Areas].into_iter().map(|k| vp(ViewRef::Schedule { schedule: k })).collect(),
        ),
    )
    .await;
    let preview: SvgDrawing = call(&app, "sheet_preview", json!({ "sheet_id": state(&app).await.project.sheet_set.sheets[0].id })).await;
    assert!(preview.svg.contains("id=\"perspective-image\""), "the render is on the sheet");

    let t = Instant::now();
    let opts = SheetExportOptions { format: PlanFormat::Pdf, sheet_ids: vec![], path: None, date: Some("2026-11-04".into()) };
    let r: ExportResult = call(&app, "export_sheets", serde_json::to_value(&opts).unwrap()).await;
    let took = t.elapsed();
    let size = std::fs::metadata(&r.path).unwrap().len();
    eprintln!("permit set: {} sheets in {:?}, {} bytes", 3, took, size);
    assert!(took.as_secs_f64() < 10.0, "took {took:?}");
    assert!(size < 20 * 1024 * 1024, "{size} bytes");
    let pdf = std::fs::read(&r.path).unwrap();
    assert_eq!(pages(&pdf), 3, "one PDF, one page per sheet");
    assert!(String::from_utf8_lossy(&pdf).contains("/Subtype /Image"), "the render is embedded");

    // SVG: one file per sheet, named by sheet number.
    let opts = SheetExportOptions { format: PlanFormat::Svg, sheet_ids: vec![], path: None, date: None };
    let r: ExportResult = call(&app, "export_sheets", serde_json::to_value(&opts).unwrap()).await;
    assert!(r.path.ends_with("-a-1.svg"), "{}", r.path);
    let folder = std::path::Path::new(&r.path).parent().unwrap();
    let svgs = std::fs::read_dir(folder).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().ends_with(".svg")).count();
    assert_eq!(svgs, 3);
}

#[tokio::test]
async fn a_services_plan_alone_on_a_sheet_is_the_service_sheet_with_the_ph_block() {
    let (app, _dir) = app();
    let st: DocState = call(&app, "hub_create", json!({ "name": "Bahay", "template": "plumbing-demo" })).await;
    let level = st.project.levels[0].id.clone();
    let s = add_sheet(&app, sheet("Plumbing layout", PaperSize::A1, Orientation::Landscape, vec![vp(ViewRef::Plan { level_id: level.clone(), sheet: SheetKind::Plumbing })])).await;
    let svg: SvgDrawing = call(&app, "sheet_preview", json!({ "sheet_id": s.id })).await;
    assert!(svg.svg.contains("id=\"service-sheet\""), "drawn by the service sheet");
    assert!(svg.svg.contains("id=\"title-block\""), "inside the PH title block");
    assert!(svg.svg.contains("MASTER PLUMBER") || svg.svg.contains("Master Plumber"), "with its blank professional block");
    let iso = add_sheet(&app, sheet("Isometric", PaperSize::A1, Orientation::Landscape, vec![vp(ViewRef::Plan { level_id: level, sheet: SheetKind::PlumbingIsometric })])).await;
    let svg: SvgDrawing = call(&app, "sheet_preview", json!({ "sheet_id": iso.id })).await;
    assert_eq!(field(&svg.svg, "scale").as_deref(), Some("NTS"));
    // The PDF of both pages still reads.
    let st = state(&app).await;
    let pdf = guhit_export::sheets_pdf(&st.project, &st.derived, &[], &SheetInputs::default()).unwrap();
    assert_eq!(pages(&pdf), 2);
}

// ------------------------------------------------- sample permit sets

/// Stub schedules from the model while lane L1's `Derived` schedules are
/// empty: types by kind, style, size and sill, rooms from their polygons,
/// levels from footprints. Samples only; the sheets read whatever `Derived`
/// holds.
fn stub_schedules(p: &Project, d: &mut Derived) {
    use std::collections::BTreeMap;
    if !d.opening_schedule.rows.is_empty() {
        return;
    }
    let wall_level: BTreeMap<&str, &str> = p
        .elements
        .iter()
        .filter_map(|e| match e {
            Element::Wall(w) => Some((w.id.as_str(), w.level_id.as_str())),
            _ => None,
        })
        .collect();
    type Key = (u8, String, i64, i64, i64);
    let mut groups: BTreeMap<Key, (OpeningType, OpeningStyle, Vec<String>)> = BTreeMap::new();
    for e in &p.elements {
        if let Element::Opening(o) = e {
            let kind = if o.opening_type == OpeningType::Door { 0 } else { 1 };
            let key = (kind, format!("{:?}", o.style), -(o.width_mm.round() as i64), o.height_mm.round() as i64, o.sill_mm.round() as i64);
            let level = wall_level.get(o.wall_id.as_str()).map(|l| l.to_string()).unwrap_or_default();
            groups.entry(key).or_insert((o.opening_type, o.style, vec![])).2.push(level);
        }
    }
    let (mut dn, mut wn) = (0, 0);
    for ((kind, _, w, h, sill), (t, style, levels)) in groups {
        let mark = if kind == 0 {
            dn += 1;
            format!("D{dn}")
        } else {
            wn += 1;
            format!("W{wn}")
        };
        let mut per: BTreeMap<String, u32> = BTreeMap::new();
        for l in &levels {
            *per.entry(l.clone()).or_default() += 1;
        }
        let description = match (t, style) {
            (OpeningType::Door, OpeningStyle::Sliding) => "Aluminum framed sliding glass door",
            (OpeningType::Door, _) => "Solid wood panel door, painted",
            (_, OpeningStyle::Fixed) => "Fixed glass, aluminum frame",
            (_, OpeningStyle::Jalousie) => "Glass jalousie, aluminum frame",
            _ => "Aluminum framed glass window",
        };
        d.opening_schedule.rows.push(OpeningRow {
            mark,
            type_id: None,
            opening_type: t,
            style,
            width_mm: -w as f64,
            height_mm: h as f64,
            sill_mm: Some(sill as f64),
            material_id: None,
            description: description.into(),
            per_level: p.levels.iter().filter_map(|l| per.get(&l.id).map(|c| LevelCount { level_id: l.id.clone(), count: *c })).collect(),
            total: levels.len() as u32,
        });
    }
    for e in &p.elements {
        if let Element::Room(r) = e {
            let Some(g) = d.rooms.iter().find(|g| g.room_id == r.id) else { continue };
            let level_h = p.levels.iter().find(|l| l.id == r.level_id).map(|l| l.height_mm).unwrap_or(2700.0);
            d.room_schedule.push(RoomRow {
                room_id: r.id.clone(),
                level_id: r.level_id.clone(),
                name: r.name.clone(),
                usage: r.usage,
                net_area_m2: g.area_mm2 / 1e6,
                floor_area_m2: g.area_mm2 / 1e6,
                perimeter_m: g.perimeter_mm / 1000.0,
                ceiling_height_mm: r.ceiling_height_mm.unwrap_or(level_h - 300.0),
                floor_material_id: r.floor_material_id.clone(),
                finishes: r.finishes.clone(),
            });
        }
    }
    for l in &p.levels {
        let net: f64 = d.room_schedule.iter().filter(|r| r.level_id == l.id).map(|r| r.floor_area_m2).sum();
        let gross: f64 = d.footprints.iter().filter(|f| f.level_id == l.id).map(|f| f.area_mm2 / 1e6).sum();
        let slab = |k: SlabKind| -> f64 {
            p.elements
                .iter()
                .filter_map(|e| match e {
                    Element::Slab(s) if s.level_id == l.id && s.kind == k => {
                        let a: f64 = s.polygon.iter().zip(s.polygon.iter().cycle().skip(1)).map(|(a, b)| a.x * b.y - b.x * a.y).sum();
                        Some(a.abs() / 2.0 / 1e6)
                    }
                    _ => None,
                })
                .sum()
        };
        if net > 0.0 || gross > 0.0 {
            d.level_areas.push(LevelAreas {
                level_id: l.id.clone(),
                net_m2: net,
                gross_m2: gross,
                open_m2: slab(SlabKind::Balcony) + slab(SlabKind::Deck),
                void_m2: slab(SlabKind::Void),
            });
        }
    }
}

fn sample_sheets(p: &Project, paper: PaperSize, perspective: Option<&str>) -> Vec<Sheet> {
    let marks = |k: MarkKind| -> Vec<Id> {
        p.elements
            .iter()
            .filter_map(|e| match e {
                Element::ViewMark(m) if m.mark_kind == k => Some(m.id.clone()),
                _ => None,
            })
            .collect()
    };
    let plan = |l: &Level, sheet: SheetKind| vp(ViewRef::Plan { level_id: l.id.clone(), sheet });
    let mut out = Vec::new();
    let mut a1 = vec![];
    if let Some(r) = perspective {
        a1.push(vp(ViewRef::Perspective { render_id: r.into() }));
    }
    a1.push(vp(ViewRef::SitePlan));
    a1.push(vp(ViewRef::VicinityMap { file_name: None, radius_m: 500.0 }));
    a1.push(vp(ViewRef::Notes {
        text: "All dimensions are in millimeters unless noted.\nVerify every dimension on site before work starts.\nThis is a sample set made from a study model; the license fields show sample values.".into(),
    }));
    let big = matches!(paper, PaperSize::A0 | PaperSize::A1);
    if big {
        out.push(sheet("Perspective, site development plan and vicinity map", paper, Orientation::Landscape, a1));
    } else {
        let notes = a1.pop().unwrap();
        let vicinity = a1.pop().unwrap();
        out.push(sheet("Perspective and site development plan", paper, Orientation::Landscape, a1));
        out.push(sheet("Vicinity map and general notes", paper, Orientation::Landscape, vec![vicinity, notes]));
    }
    let per = if big { 2 } else { 1 };
    for pair in p.levels.chunks(per) {
        let title = pair.iter().map(|l| l.name.clone()).collect::<Vec<_>>().join(" and ");
        out.push(sheet(&format!("{title} plan"), paper, Orientation::Landscape, pair.iter().map(|l| plan(l, SheetKind::Plan)).collect()));
    }
    let per = if big { 4 } else { 2 };
    for group in marks(MarkKind::Elevation).chunks(per) {
        out.push(sheet("Elevations", paper, Orientation::Landscape, group.iter().map(|id| vp(ViewRef::Elevation { mark_id: id.clone() })).collect()));
    }
    for group in marks(MarkKind::Section).chunks(2) {
        out.push(sheet("Sections", paper, Orientation::Landscape, group.iter().map(|id| vp(ViewRef::Section { mark_id: id.clone() })).collect()));
    }
    out.push(sheet("Roof plan", paper, Orientation::Landscape, vec![vp(ViewRef::RoofPlan)]));
    let ceilings: Vec<&Level> = p.levels.iter().take(3).collect();
    for pair in ceilings.chunks(if big { 2 } else { 1 }) {
        out.push(sheet("Reflected ceiling plan", paper, Orientation::Landscape, pair.iter().map(|l| plan(l, SheetKind::ReflectedCeiling)).collect()));
    }
    let schedules = [ScheduleKind::DoorsWindows, ScheduleKind::Areas, ScheduleKind::Rooms, ScheduleKind::Finishes];
    if big {
        out.push(sheet("Schedules", paper, Orientation::Landscape, schedules.iter().map(|k| vp(ViewRef::Schedule { schedule: *k })).collect()));
    } else {
        out.push(sheet("Door and window schedule, floor areas", paper, Orientation::Landscape, schedules[..2].iter().map(|k| vp(ViewRef::Schedule { schedule: *k })).collect()));
        out.push(sheet("Room and finish schedules", paper, Orientation::Landscape, schedules[2..].iter().map(|k| vp(ViewRef::Schedule { schedule: *k })).collect()));
    }
    out
}

/// Writes `permit-set-a1.pdf` and `permit-set-a3.pdf` into `L4_SAMPLE_OUT`
/// from the project folder in `L4_SAMPLE_PROJECT` (project.json and its
/// renders). Run with `--ignored`.
#[tokio::test]
#[ignore]
async fn sample_permit_sets() {
    let (Ok(src), Ok(out)) = (std::env::var("L4_SAMPLE_PROJECT"), std::env::var("L4_SAMPLE_OUT")) else {
        eprintln!("set L4_SAMPLE_PROJECT and L4_SAMPLE_OUT");
        return;
    };
    let src = std::path::PathBuf::from(src);
    let out = std::path::PathBuf::from(out);
    std::fs::create_dir_all(&out).unwrap();
    let (app, dir) = app();
    let project: Project = serde_json::from_str(&std::fs::read_to_string(src.join("project.json")).unwrap()).unwrap();
    let pdir = dir.path().join("projects").join(&project.id);
    std::fs::create_dir_all(pdir.join("renders")).unwrap();
    std::fs::copy(src.join("project.json"), pdir.join("project.json")).unwrap();
    for e in std::fs::read_dir(src.join("renders")).unwrap().flatten() {
        std::fs::copy(e.path(), pdir.join("renders").join(e.file_name())).unwrap();
    }
    let st: DocState = call(&app, "hub_open", json!({ "id": project.id })).await;
    let ground = st.project.levels[0].id.clone();

    // View marks around the buildings, and a lot 4 m out.
    let mut b = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for f in &st.derived.footprints {
        for q in &f.polygon {
            b = (b.0.min(q.x), b.1.min(q.y), b.2.max(q.x), b.3.max(q.y));
        }
    }
    let (cx, cy) = ((b.0 + b.2) / 2.0, (b.1 + b.3) / 2.0);
    let pt = |x: f64, y: f64| Point { x, y };
    let mark = |kind: MarkKind, label: &str, a: Point, z: Point| Command::AddElement {
        element: Element::ViewMark(ViewMark { id: String::new(), level_id: ground.clone(), mark_kind: kind, label: label.into(), start: a, end: z, flip: false, depth_mm: None }),
    };
    let mut cmds = vec![
        mark(MarkKind::Elevation, "North", pt(b.2 + 2000.0, b.3 + 3000.0), pt(b.0 - 2000.0, b.3 + 3000.0)),
        mark(MarkKind::Elevation, "South", pt(b.0 - 2000.0, b.1 - 3000.0), pt(b.2 + 2000.0, b.1 - 3000.0)),
        mark(MarkKind::Elevation, "East", pt(b.2 + 3000.0, b.1 - 2000.0), pt(b.2 + 3000.0, b.3 + 2000.0)),
        mark(MarkKind::Elevation, "West", pt(b.0 - 3000.0, b.3 + 2000.0), pt(b.0 - 3000.0, b.1 - 2000.0)),
        mark(MarkKind::Section, "A", pt(b.0 - 1000.0, cy), pt(b.2 + 1000.0, cy)),
        mark(MarkKind::Section, "B", pt(cx, b.3 + 1000.0), pt(cx, b.1 - 1000.0)),
    ];
    let side = |role: LotSideRole, setback: f64, road: &str| LotSide { role, setback_mm: setback, road: road.into(), road_width_mm: if road.is_empty() { None } else { Some(10000.0) } };
    cmds.push(Command::AddElement {
        element: Element::Lot(Lot {
            id: String::new(),
            corners: vec![pt(b.0 - 4000.0, b.1 - 4500.0), pt(b.2 + 4000.0, b.1 - 4500.0), pt(b.2 + 4000.0, b.3 + 4000.0), pt(b.0 - 4000.0, b.3 + 4000.0)],
            sides: vec![side(LotSideRole::Road, 4500.0, "Sample Street"), side(LotSideRole::Side, 2000.0, ""), side(LotSideRole::Rear, 2000.0, ""), side(LotSideRole::Side, 2000.0, "")],
            grade_mm: 150.0,
        }),
    });
    // Door and window types and marks, where lane L1 has built them.
    cmds.push(Command::AssignOpeningTypes { level_id: None });
    for c in cmds {
        if let Err(e) = app.handle("doc_apply", json!({ "command": c })).await {
            eprintln!("skipped: {e}");
        }
    }
    apply(
        &app,
        Command::SetTitleBlock {
            title_block: Box::new(TitleBlock {
                project_title: "Lovell Beach House (study model)".into(),
                lot_no: "12".into(),
                block_no: "4".into(),
                tct_no: "T-000123 (sample)".into(),
                firm_name: "Sample Design Studio".into(),
                firm_address: "Unit 5, 123 Sample Avenue, Quezon City".into(),
                firm_logo: None,
                signatory: Signatory::default(),
            }),
        },
    )
    .await;
    apply(&app, Command::SetSignatory { signatory: Box::new(Signatory { name: "Juan Dela Cruz (sample)".into(), ..signatory() }) }).await;

    let renders: Vec<RenderRecord> = call(&app, "render_list", json!({})).await;
    let traced = |r: &&RenderRecord| r.source == RenderSource::ModelView && r.info.as_ref().is_some_and(|i| format!("{:?}", i.kind).contains("Path"));
    let perspective = renders.iter().find(traced).or_else(|| renders.iter().find(|r| r.source == RenderSource::ModelView)).map(|r| r.id.clone());

    for (paper, name) in [(PaperSize::A1, "permit-set-a1"), (PaperSize::A3, "permit-set-a3")] {
        let st = state(&app).await;
        let old: Vec<Id> = st.project.sheet_set.sheets.iter().map(|s| s.id.clone()).collect();
        if !old.is_empty() {
            apply(&app, Command::DeleteSheets { ids: old }).await;
        }
        for s in sample_sheets(&st.project, paper, perspective.as_deref()) {
            apply(&app, Command::UpsertSheet { sheet: Box::new(s), index: None }).await;
        }
        let st = state(&app).await;
        let ids: Vec<Id> = st.project.sheet_set.sheets.iter().map(|s| s.id.clone()).collect();
        let revision = SheetRevision { id: String::new(), mark: "1".into(), date: "2026-11-04".into(), description: "Issued for building permit".into() };
        let _ = app.handle("doc_apply", json!({ "command": Command::UpsertRevision { revision, sheet_ids: ids } })).await;
        let st = state(&app).await;
        let mut derived = st.derived.clone();
        stub_schedules(&st.project, &mut derived);
        let images = guhit_app::documents::sheet_images(&pdir, &st.project);
        let inputs = SheetInputs { date: Some("2026-11-04".into()), images, mask_signatory: false };
        let t = Instant::now();
        let pdf = guhit_export::sheets_pdf(&st.project, &derived, &[], &inputs).unwrap();
        let took = t.elapsed();
        std::fs::write(out.join(format!("{name}.pdf")), &pdf).unwrap();
        for (number, svg) in sheetset::sheets_svg(&st.project, &derived, &[], &inputs).unwrap() {
            std::fs::write(out.join(format!("{name}-{number}.svg")), svg).unwrap();
        }
        eprintln!("{name}: {} sheets, {} bytes, {took:?}", st.project.sheet_set.sheets.len(), pdf.len());
    }
}
