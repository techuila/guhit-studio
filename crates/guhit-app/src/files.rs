//! File helpers shared by the store modules: atomic writes, names that are
//! safe to join onto a folder, image data URLs, timestamps.
//!
//! Everything here is `std::path` and `std::fs` only, so it behaves the same
//! on macOS and Windows.

use std::io::Write;
use std::path::{Path, PathBuf};

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use guhit_model::{defaults, IpcError};

/// Largest decoded image accepted over IPC.
pub const MAX_IMAGE_BYTES: usize = 25 * 1024 * 1024;

const MAX_NAME_CHARS: usize = 100;

pub fn io_err(what: &str, path: &Path, e: std::io::Error) -> IpcError {
    IpcError::new("io", format!("{what} {}: {e}", path.display()))
}

pub fn create_dir(path: &Path) -> Result<(), IpcError> {
    std::fs::create_dir_all(path).map_err(|e| io_err("cannot create folder", path, e))
}

/// Write `bytes` to `path` so that a crash never leaves a half-written file:
/// write a temp file next to it, flush to disk, then rename over the target.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), IpcError> {
    let dir = path
        .parent()
        .ok_or_else(|| IpcError::new("io", format!("{} has no parent folder", path.display())))?;
    create_dir(dir)?;
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| IpcError::new("io", format!("{} has no file name", path.display())))?;
    let tmp = dir.join(format!(".{file_name}.{}.tmp", defaults::new_id()));
    let result = (|| {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        drop(f);
        std::fs::rename(&tmp, path)
    })();
    if let Err(e) = result {
        let _ = std::fs::remove_file(&tmp);
        return Err(io_err("cannot write", path, e));
    }
    Ok(())
}

pub fn write_json_atomic<T: serde::Serialize>(path: &Path, value: &T) -> Result<(), IpcError> {
    let bytes = serde_json::to_vec_pretty(value)
        .map_err(|e| IpcError::new("io", format!("cannot serialize {}: {e}", path.display())))?;
    write_atomic(path, &bytes)
}

// ---------------------------------------------------------------- safe names

/// Ids that arrive over IPC become folder and file names. Accept only what a
/// UUID or slug can contain, so an id can never leave its parent folder.
pub fn check_id(id: &str, what: &str) -> Result<(), IpcError> {
    let ok = !id.is_empty()
        && id.len() <= 64
        && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if ok {
        Ok(())
    } else {
        Err(IpcError::new("invalid", format!("invalid {what} id")))
    }
}

const WINDOWS_RESERVED: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9", "LPT1",
    "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Turn a file name from the UI into one that is safe inside a folder we own.
/// Directories are stripped, `..` anywhere is rejected, and every character
/// outside `A-Z a-z 0-9 . _ - ( ) space` becomes `_`.
pub fn safe_file_name(raw: &str) -> Result<String, IpcError> {
    let bad = |why: &str| IpcError::new("invalid", format!("invalid file name: {why}"));
    if raw.contains('\0') {
        return Err(bad("contains a NUL character"));
    }
    let parts: Vec<&str> = raw.split(['/', '\\']).collect();
    if parts.iter().any(|p| p.trim() == "..") {
        return Err(bad("path traversal is not allowed"));
    }
    let last = parts.last().copied().unwrap_or("");
    let mut name: String = last
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '(' | ')' | ' ') {
                c
            } else {
                '_'
            }
        })
        .collect();
    // Leading dots make hidden files, trailing dots and spaces break on Windows.
    name = name.trim_matches(|c| c == '.' || c == ' ').to_string();
    while name.contains("..") {
        name = name.replace("..", ".");
    }
    if name.chars().count() > MAX_NAME_CHARS {
        let (stem, ext) = split_ext(&name);
        let keep = MAX_NAME_CHARS.saturating_sub(ext.len() + 1).max(1);
        let stem: String = stem.chars().take(keep).collect();
        name = if ext.is_empty() { stem } else { format!("{stem}.{ext}") };
    }
    if name.is_empty() {
        return Err(bad("the name is empty"));
    }
    let stem_upper = split_ext(&name).0.to_ascii_uppercase();
    if WINDOWS_RESERVED.contains(&stem_upper.as_str()) {
        name = format!("_{name}");
    }
    Ok(name)
}

/// For reads: the name must already be in safe form. Nothing is rewritten, so
/// a lookup can only ever hit a file that `safe_file_name` could have made.
pub fn check_file_name(raw: &str) -> Result<(), IpcError> {
    match safe_file_name(raw) {
        Ok(n) if n == raw => Ok(()),
        Ok(_) => Err(IpcError::new("invalid", "invalid file name")),
        Err(e) => Err(e),
    }
}

/// A short slug for default export file names.
pub fn slug(raw: &str, fallback: &str) -> String {
    let mut out = String::new();
    for c in raw.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    let out: String = out.trim_matches('-').chars().take(60).collect();
    let out = out.trim_matches('-').to_string();
    if out.is_empty() {
        fallback.to_string()
    } else {
        out
    }
}

/// `("plan", "png")` for `plan.png`. The extension is empty when there is none.
pub fn split_ext(name: &str) -> (&str, &str) {
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => (stem, ext),
        _ => (name, ""),
    }
}

/// First path of the form `stem.ext`, `stem-2.ext`, `stem-3.ext` ... that does
/// not exist yet in `dir`.
pub fn unique_path(dir: &Path, stem: &str, ext: &str) -> PathBuf {
    let make = |n: u32| {
        let base = if n <= 1 { stem.to_string() } else { format!("{stem}-{n}") };
        dir.join(if ext.is_empty() { base } else { format!("{base}.{ext}") })
    };
    let mut n = 1;
    loop {
        let p = make(n);
        if !p.exists() {
            return p;
        }
        n += 1;
    }
}

// ----------------------------------------------------------------- data URLs

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageKind {
    Png,
    Jpeg,
}

impl ImageKind {
    pub fn mime(self) -> &'static str {
        match self {
            ImageKind::Png => "image/png",
            ImageKind::Jpeg => "image/jpeg",
        }
    }

    pub fn ext(self) -> &'static str {
        match self {
            ImageKind::Png => "png",
            ImageKind::Jpeg => "jpg",
        }
    }

    pub fn from_ext(ext: &str) -> Option<Self> {
        match ext.to_ascii_lowercase().as_str() {
            "png" => Some(ImageKind::Png),
            "jpg" | "jpeg" => Some(ImageKind::Jpeg),
            _ => None,
        }
    }

    fn matches_magic(self, bytes: &[u8]) -> bool {
        match self {
            ImageKind::Png => bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]),
            ImageKind::Jpeg => bytes.starts_with(&[0xFF, 0xD8, 0xFF]),
        }
    }

    /// The type of image bytes that arrived without a media type (a live
    /// session guest's upload), from their first bytes.
    pub fn sniff(bytes: &[u8]) -> Option<Self> {
        [ImageKind::Png, ImageKind::Jpeg].into_iter().find(|k| k.matches_magic(bytes))
    }
}

/// Decode a `data:image/png;base64,...` or `data:image/jpeg;base64,...` URL.
/// Anything else is rejected, as is a payload over 25 MB or one whose bytes
/// are not actually that image type.
pub fn decode_image_data_url(url: &str) -> Result<(ImageKind, Vec<u8>), IpcError> {
    let bad = |why: &str| IpcError::new("invalid", format!("invalid image data: {why}"));
    let rest = url.strip_prefix("data:").ok_or_else(|| bad("expected a data URL"))?;
    let (header, payload) = rest.split_once(',').ok_or_else(|| bad("expected a data URL"))?;
    let mut params = header.split(';');
    let mime = params.next().unwrap_or("").trim().to_ascii_lowercase();
    let kind = match mime.as_str() {
        "image/png" => ImageKind::Png,
        "image/jpeg" | "image/jpg" => ImageKind::Jpeg,
        _ => return Err(bad("only image/png and image/jpeg are accepted")),
    };
    if !params.any(|p| p.trim().eq_ignore_ascii_case("base64")) {
        return Err(bad("the data URL must be base64 encoded"));
    }
    // Check the size before decoding: 4 base64 chars carry 3 bytes.
    if payload.len() / 4 * 3 > MAX_IMAGE_BYTES + 3 {
        return Err(bad("the image is larger than 25 MB"));
    }
    let bytes = B64
        .decode(payload.trim().as_bytes())
        .map_err(|e| bad(&format!("base64: {e}")))?;
    if bytes.len() > MAX_IMAGE_BYTES {
        return Err(bad("the image is larger than 25 MB"));
    }
    if !kind.matches_magic(&bytes) {
        return Err(bad(&format!("the bytes are not a {} file", kind.ext())));
    }
    Ok((kind, bytes))
}

/// Decode any base64 data URL, whatever its media type, up to `max_bytes`.
/// Used by the interchange commands, where the payload is a DXF, a DWG or a
/// 3D model and the extension, not the media type, decides what it is.
pub fn decode_any_data_url(url: &str, max_bytes: usize) -> Result<Vec<u8>, IpcError> {
    let bad = |why: &str| IpcError::new("invalid", format!("invalid file data: {why}"));
    let rest = url.strip_prefix("data:").ok_or_else(|| bad("expected a data URL"))?;
    let (header, payload) = rest.split_once(',').ok_or_else(|| bad("expected a data URL"))?;
    if !header.split(';').any(|p| p.trim().eq_ignore_ascii_case("base64")) {
        return Err(bad("the data URL must be base64 encoded"));
    }
    let too_big = || bad(&format!("the file is larger than {} MB", max_bytes / (1024 * 1024)));
    // Check the size before decoding: 4 base64 chars carry 3 bytes.
    if payload.len() / 4 * 3 > max_bytes + 3 {
        return Err(too_big());
    }
    let bytes = B64
        .decode(payload.trim().as_bytes())
        .map_err(|e| bad(&format!("base64: {e}")))?;
    if bytes.len() > max_bytes {
        return Err(too_big());
    }
    Ok(bytes)
}

pub fn encode_any_data_url(mime: &str, bytes: &[u8]) -> String {
    format!("data:{mime};base64,{}", B64.encode(bytes))
}

/// PNG only, for thumbnails, renders and image exports.
pub fn decode_png_data_url(url: &str) -> Result<Vec<u8>, IpcError> {
    match decode_image_data_url(url)? {
        (ImageKind::Png, bytes) => Ok(bytes),
        _ => Err(IpcError::new("invalid", "invalid image data: a PNG is required")),
    }
}

pub fn encode_data_url(kind: ImageKind, bytes: &[u8]) -> String {
    format!("data:{};base64,{}", kind.mime(), B64.encode(bytes))
}

/// Read an image file and return it as a data URL.
pub fn read_data_url(path: &Path, kind: ImageKind) -> Result<String, IpcError> {
    let bytes = std::fs::read(path).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => IpcError::new("not_found", format!("file not found: {}", path.display())),
        _ => io_err("cannot read", path, e),
    })?;
    Ok(encode_data_url(kind, &bytes))
}

// ---------------------------------------------------------------------- time

/// `20260922-143005` from the current UTC time. Used in file names.
pub fn file_timestamp() -> String {
    let t = defaults::now_rfc3339();
    let digits: String = t.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.len() >= 14 {
        format!("{}-{}", &digits[..8], &digits[8..14])
    } else {
        digits
    }
}

/// Seconds since the Unix epoch for a `YYYY-MM-DDTHH:MM:SSZ` timestamp, the
/// only form `defaults::now_rfc3339` writes. None for anything else.
pub fn parse_rfc3339_secs(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.len() < 20 || b[4] != b'-' || b[7] != b'-' || (b[10] != b'T' && b[10] != b't') || b[13] != b':' || b[16] != b':' {
        return None;
    }
    let num = |r: std::ops::Range<usize>| s.get(r)?.parse::<i64>().ok();
    let (y, m, d) = (num(0..4)?, num(5..7)?, num(8..10)?);
    let (hh, mm, ss) = (num(11..13)?, num(14..16)?, num(17..19)?);
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) || hh > 23 || mm > 59 || ss > 60 {
        return None;
    }
    // Days from civil date (Howard Hinnant's algorithm).
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(days * 86_400 + hh * 3_600 + mm * 60 + ss)
}

pub fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_names_are_made_safe() {
        assert_eq!(safe_file_name("plan.png").unwrap(), "plan.png");
        assert_eq!(safe_file_name("C:\\Users\\me\\site plan (1).JPG").unwrap(), "site plan (1).JPG");
        assert_eq!(safe_file_name("/etc/passwd").unwrap(), "passwd");
        assert_eq!(safe_file_name("a:b*c?.png").unwrap(), "a_b_c_.png");
        assert_eq!(safe_file_name(".hidden.png").unwrap(), "hidden.png");
        assert_eq!(safe_file_name("con.png").unwrap(), "_con.png");
        assert!(safe_file_name("../../secret.png").is_err());
        assert!(safe_file_name("a/../b.png").is_err());
        assert!(safe_file_name("..\\..\\b.png").is_err());
        assert!(safe_file_name("..").is_err());
        assert!(safe_file_name("").is_err());
        assert!(safe_file_name("dir/").is_err());
        assert!(safe_file_name(&"x".repeat(500)).unwrap().chars().count() <= MAX_NAME_CHARS);
    }

    #[test]
    fn read_names_must_already_be_safe() {
        assert!(check_file_name("plan.png").is_ok());
        assert!(check_file_name("sub/plan.png").is_err());
        assert!(check_file_name("../plan.png").is_err());
        assert!(check_file_name(".plan.png").is_err());
    }

    #[test]
    fn ids_are_checked() {
        assert!(check_id("0b9d2c1e-6f0a-4f0e-9d8e-2b1c3d4e5f60", "project").is_ok());
        assert!(check_id("..", "project").is_err());
        assert!(check_id("a/b", "project").is_err());
        assert!(check_id("a\\b", "project").is_err());
        assert!(check_id("", "project").is_err());
    }

    #[test]
    fn data_urls() {
        let png = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 1, 2, 3];
        let url = encode_data_url(ImageKind::Png, &png);
        assert_eq!(decode_image_data_url(&url).unwrap(), (ImageKind::Png, png.to_vec()));
        assert!(decode_image_data_url("data:text/html;base64,PGI+").is_err());
        assert!(decode_image_data_url("data:image/svg+xml;base64,PGI+").is_err());
        assert!(decode_image_data_url("data:image/png;base64,PGI+").is_err(), "magic bytes are checked");
        assert!(decode_image_data_url("data:image/png,rawtext").is_err());
        assert!(decode_image_data_url("http://example.com/a.png").is_err());
    }

    #[test]
    fn timestamps() {
        assert_eq!(parse_rfc3339_secs("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_rfc3339_secs("2026-09-22T00:00:00Z"), Some(1_790_035_200));
        assert_eq!(parse_rfc3339_secs("garbage"), None);
        let now = parse_rfc3339_secs(&defaults::now_rfc3339()).unwrap();
        assert!((now - now_secs()).abs() <= 2);
        assert_eq!(file_timestamp().len(), 15);
    }

    #[test]
    fn slugs() {
        assert_eq!(slug("Reyes Residence / Rev. 2", "project"), "reyes-residence-rev-2");
        assert_eq!(slug("///", "project"), "project");
    }
}
