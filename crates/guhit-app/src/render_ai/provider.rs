//! The seam between `render_ai_generate` and a hosted image model.
//!
//! Everything above this trait is provider neutral: the source capture, the
//! assembled prompt, the record that gets written. Everything a provider
//! needs to know arrives in `RenderInput`, and nothing else about the app
//! reaches it. `GeminiProvider` (gemini.rs) is the only real implementation;
//! `FakeProvider` stands in for it in tests so no test ever needs a key.

use std::future::Future;
use std::pin::Pin;
use std::sync::Mutex;

use guhit_model::{IpcError, RenderQuality};

use crate::files::ImageKind;

pub type BoxFut<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// One image request, minus transport details.
#[derive(Debug, Clone)]
pub struct RenderInput {
    /// The Tier 1 capture, as PNG bytes.
    pub png: Vec<u8>,
    /// The full text sent to the model: preamble, style, user text.
    pub prompt: String,
    /// What the image must not change. Providers with a real negative-prompt
    /// field use it there; Gemini has none, so it goes in the text.
    pub constraint: String,
    /// Derived from the capture, so the answer has the same framing.
    pub aspect_ratio: String,
    pub quality: RenderQuality,
}

/// One generated image.
#[derive(Debug, Clone)]
pub struct RenderOutput {
    pub bytes: Vec<u8>,
    pub kind: ImageKind,
    /// Exact model id the provider used, for the record and the log.
    pub model: String,
    pub seconds: f64,
}

/// Why a provider could not produce an image. Maps onto the two IPC codes
/// the contract reserves for AI failures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderError {
    /// No API key, or the provider is not set up. IPC code `ai_not_configured`.
    NotConfigured(String),
    /// Anything else: HTTP status, timeout, unparseable answer, refusal.
    /// IPC code `ai_failed`. Never holds the API key.
    Failed(String),
}

impl ProviderError {
    pub fn failed(message: impl Into<String>) -> Self {
        ProviderError::Failed(message.into())
    }
}

impl From<ProviderError> for IpcError {
    fn from(e: ProviderError) -> Self {
        match e {
            ProviderError::NotConfigured(m) => IpcError::new("ai_not_configured", m),
            ProviderError::Failed(m) => IpcError::new("ai_failed", m),
        }
    }
}

pub trait ImageProvider: Send + Sync {
    /// Turn a model view into a photorealistic image. Takes at most two
    /// minutes; callers must not hold a lock across it.
    fn render<'a>(&'a self, input: RenderInput) -> BoxFut<'a, Result<RenderOutput, ProviderError>>;
}

// ------------------------------------------------------------ image geometry

/// Width and height of a PNG, from the IHDR chunk. None when the bytes are
/// not a PNG with a readable header.
pub fn png_size(bytes: &[u8]) -> Option<(u32, u32)> {
    const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    if bytes.len() < 24 || !bytes.starts_with(&SIGNATURE) || &bytes[12..16] != b"IHDR" {
        return None;
    }
    let read = |at: usize| u32::from_be_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]);
    let (w, h) = (read(16), read(20));
    if w == 0 || h == 0 {
        None
    } else {
        Some((w, h))
    }
}

/// Aspect ratios the image models accept, as documented.
const RATIOS: [(&str, f64); 10] = [
    ("21:9", 21.0 / 9.0),
    ("16:9", 16.0 / 9.0),
    ("3:2", 3.0 / 2.0),
    ("4:3", 4.0 / 3.0),
    ("5:4", 5.0 / 4.0),
    ("1:1", 1.0),
    ("4:5", 4.0 / 5.0),
    ("3:4", 3.0 / 4.0),
    ("2:3", 2.0 / 3.0),
    ("9:16", 9.0 / 16.0),
];

/// The accepted ratio closest to the capture, so the generated image keeps
/// the framing the architect set up in the 3D view. A capture whose header
/// cannot be read falls back to 16:9, the viewport default.
pub fn aspect_ratio_for(png: &[u8]) -> String {
    let Some((w, h)) = png_size(png) else {
        return "16:9".to_string();
    };
    let want = w as f64 / h as f64;
    RATIOS
        .iter()
        // Compare in log space so 21:9 is not favoured by its larger spread.
        .min_by(|a, b| {
            let d = |r: f64| (r.ln() - want.ln()).abs();
            d(a.1).partial_cmp(&d(b.1)).unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|(name, _)| name.to_string())
        .unwrap_or_else(|| "1:1".to_string())
}

// ----------------------------------------------------------------------- fake

/// Test provider. Answers with a generated image, or with a scripted error,
/// and records every input so a test can assert on what was sent.
pub struct FakeProvider {
    pub inputs: Mutex<Vec<RenderInput>>,
    model: String,
    kind: ImageKind,
    error: Option<ProviderError>,
}

impl Default for FakeProvider {
    fn default() -> Self {
        Self {
            inputs: Mutex::new(vec![]),
            model: "fake-image-model".to_string(),
            kind: ImageKind::Png,
            error: None,
        }
    }
}

impl FakeProvider {
    pub fn new() -> Self {
        Self::default()
    }

    /// A provider that always fails the way a live one would.
    pub fn failing(message: &str) -> Self {
        Self { error: Some(ProviderError::failed(message)), ..Self::default() }
    }

    /// A provider that answers with JPEG bytes instead of PNG.
    pub fn jpeg() -> Self {
        Self { kind: ImageKind::Jpeg, ..Self::default() }
    }

    pub fn calls(&self) -> usize {
        self.inputs.lock().map(|i| i.len()).unwrap_or(0)
    }

    pub fn last_prompt(&self) -> String {
        self.inputs
            .lock()
            .ok()
            .and_then(|i| i.last().map(|input| input.prompt.clone()))
            .unwrap_or_default()
    }

    pub fn last_aspect_ratio(&self) -> String {
        self.inputs
            .lock()
            .ok()
            .and_then(|i| i.last().map(|input| input.aspect_ratio.clone()))
            .unwrap_or_default()
    }
}

/// A 1x1 PNG, built by hand so the tests need no image crate. Real enough to
/// pass the magic-byte check in `files::decode_image_data_url`.
pub fn tiny_png() -> Vec<u8> {
    const BYTES: [u8; 69] = [
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52, 0x00,
        0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90, 0x77, 0x53, 0xDE, 0x00,
        0x00, 0x00, 0x0C, 0x49, 0x44, 0x41, 0x54, 0x08, 0xD7, 0x63, 0xF8, 0xCF, 0xC0, 0x00, 0x00, 0x03, 0x01,
        0x01, 0x00, 0x18, 0xDD, 0x8D, 0xB0, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60,
        0x82,
    ];
    BYTES.to_vec()
}

/// A PNG with the given size in its header. Only the header is meaningful,
/// which is all `aspect_ratio_for` and the magic-byte check look at.
pub fn png_of_size(width: u32, height: u32) -> Vec<u8> {
    let mut bytes = tiny_png();
    bytes[16..20].copy_from_slice(&width.to_be_bytes());
    bytes[20..24].copy_from_slice(&height.to_be_bytes());
    bytes
}

/// The smallest thing that passes the JPEG magic-byte check.
pub fn tiny_jpeg() -> Vec<u8> {
    let mut bytes = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, b'J', b'F', b'I', b'F', 0x00];
    bytes.extend_from_slice(&[0xFF, 0xD9]);
    bytes
}

impl ImageProvider for FakeProvider {
    fn render<'a>(&'a self, input: RenderInput) -> BoxFut<'a, Result<RenderOutput, ProviderError>> {
        Box::pin(async move {
            if let Ok(mut seen) = self.inputs.lock() {
                seen.push(input);
            }
            if let Some(e) = &self.error {
                return Err(e.clone());
            }
            let bytes = match self.kind {
                ImageKind::Png => tiny_png(),
                ImageKind::Jpeg => tiny_jpeg(),
            };
            Ok(RenderOutput { bytes, kind: self.kind, model: self.model.clone(), seconds: 0.25 })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn png_header_is_read() {
        assert_eq!(png_size(&tiny_png()), Some((1, 1)));
        assert_eq!(png_size(&png_of_size(1920, 1080)), Some((1920, 1080)));
        assert_eq!(png_size(b"not a png"), None);
        assert_eq!(png_size(&tiny_jpeg()), None);
    }

    #[test]
    fn aspect_ratio_snaps_to_an_accepted_value() {
        assert_eq!(aspect_ratio_for(&png_of_size(1920, 1080)), "16:9");
        assert_eq!(aspect_ratio_for(&png_of_size(1000, 1000)), "1:1");
        assert_eq!(aspect_ratio_for(&png_of_size(1200, 800)), "3:2");
        assert_eq!(aspect_ratio_for(&png_of_size(1024, 768)), "4:3");
        assert_eq!(aspect_ratio_for(&png_of_size(1080, 1920)), "9:16");
        assert_eq!(aspect_ratio_for(&png_of_size(2560, 1080)), "21:9");
        // An unreadable capture still gets a valid ratio.
        assert_eq!(aspect_ratio_for(b"garbage"), "16:9");
    }
}
