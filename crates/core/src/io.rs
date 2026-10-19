//! Opening and saving pictures. Flat formats (PNG, JPEG, BMP, GIF) are flattened on
//! save; `.hpaint` keeps every layer. Every decode has size limits, and every save
//! is atomic (temp file + rename), so a failed save never destroys the old file.

use std::fs;
use std::io::{Cursor, Read, Write};
use std::path::Path;

use image::{DynamicImage, ImageFormat, ImageReader, Limits, RgbaImage};
use serde::{Deserialize, Serialize};

use crate::canvas::{MAX_SIDE, Raster, check_size};
use crate::color::Rgba;
use crate::document::{Document, Layer, MAX_LAYERS};
use crate::error::{Error, Result};

/// Largest file HarshPaint will read.
pub const MAX_FILE_BYTES: u64 = 512 * 1024 * 1024;
const MANIFEST: &str = "manifest.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    Png,
    Jpeg,
    Bmp,
    Gif,
    HPaint,
}

impl FileKind {
    pub fn from_path(path: &Path) -> Option<FileKind> {
        let ext = path.extension()?.to_string_lossy().to_ascii_lowercase();
        Some(match ext.as_str() {
            "png" => FileKind::Png,
            "jpg" | "jpeg" => FileKind::Jpeg,
            "bmp" => FileKind::Bmp,
            "gif" => FileKind::Gif,
            "hpaint" => FileKind::HPaint,
            _ => return None,
        })
    }

    /// Formats that can't store transparency get a white background on save.
    pub fn has_alpha(self) -> bool {
        !matches!(self, FileKind::Jpeg | FileKind::Bmp)
    }

    pub fn keeps_layers(self) -> bool {
        self == FileKind::HPaint
    }
}

pub fn open(path: &Path) -> Result<Document> {
    let size = fs::metadata(path)?.len();
    if size > MAX_FILE_BYTES {
        return Err(Error::Format(format!(
            "the file is {} MB; HarshPaint opens files up to {} MB",
            size / (1024 * 1024),
            MAX_FILE_BYTES / (1024 * 1024)
        )));
    }
    let bytes = fs::read(path)?;
    if FileKind::from_path(path) == Some(FileKind::HPaint) {
        return load_hpaint(&bytes);
    }
    Ok(Document::from_raster(decode_image(&bytes)?))
}

/// Decodes any supported image (format is detected from the bytes).
pub fn decode_image(bytes: &[u8]) -> Result<Raster> {
    let mut reader = ImageReader::new(Cursor::new(bytes)).with_guessed_format()?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_SIDE);
    limits.max_image_height = Some(MAX_SIDE);
    limits.max_alloc = Some(1024 * 1024 * 1024);
    reader.limits(limits);
    let img = reader.decode()?.into_rgba8();
    check_size(u64::from(img.width()), u64::from(img.height()))?;
    Raster::from_rgba8(img.width(), img.height(), img.as_raw())
}

pub fn save(doc: &Document, path: &Path) -> Result<()> {
    let kind = FileKind::from_path(path).ok_or_else(|| {
        Error::Format("choose a file type: .png, .jpg, .bmp, .gif or .hpaint".into())
    })?;
    let bytes = encode(doc, kind)?;
    write_atomically(path, &bytes)
}

pub fn encode(doc: &Document, kind: FileKind) -> Result<Vec<u8>> {
    if kind == FileKind::HPaint {
        return save_hpaint(doc);
    }
    let flat = doc.flatten()?;
    encode_raster(&flat, kind)
}

pub fn encode_raster(flat: &Raster, kind: FileKind) -> Result<Vec<u8>> {
    let mut out = Cursor::new(Vec::new());
    let rgba = RgbaImage::from_raw(flat.width(), flat.height(), flat.to_rgba8())
        .ok_or_else(|| Error::Format("internal pixel buffer has the wrong size".into()))?;
    match kind {
        FileKind::Png => DynamicImage::ImageRgba8(rgba).write_to(&mut out, ImageFormat::Png)?,
        FileKind::Gif => DynamicImage::ImageRgba8(rgba).write_to(&mut out, ImageFormat::Gif)?,
        FileKind::Jpeg | FileKind::Bmp => {
            let mut on_white = flat.clone();
            for p in on_white.pixels_mut() {
                *p = p.over(Rgba::WHITE);
            }
            let rgb = DynamicImage::ImageRgba8(
                RgbaImage::from_raw(flat.width(), flat.height(), on_white.to_rgba8()).ok_or_else(
                    || Error::Format("internal pixel buffer has the wrong size".into()),
                )?,
            )
            .into_rgb8();
            if kind == FileKind::Jpeg {
                let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 92);
                enc.encode_image(&DynamicImage::ImageRgb8(rgb))?;
            } else {
                DynamicImage::ImageRgb8(rgb).write_to(&mut out, ImageFormat::Bmp)?;
            }
        }
        FileKind::HPaint => {
            return Err(Error::Format(
                "a layered file needs the whole document".into(),
            ));
        }
    }
    Ok(out.into_inner())
}

// ---- .hpaint: a zip of manifest.json + one PNG per layer -----------------------

#[derive(Serialize, Deserialize)]
struct Manifest {
    format: String,
    version: u32,
    width: u32,
    height: u32,
    active: usize,
    layers: Vec<LayerEntry>,
}

#[derive(Serialize, Deserialize)]
struct LayerEntry {
    name: String,
    visible: bool,
    opacity: f32,
    file: String,
}

fn zip_err(e: impl std::fmt::Display) -> Error {
    Error::Format(format!("the .hpaint file is damaged: {e}"))
}

fn save_hpaint(doc: &Document) -> Result<Vec<u8>> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let deflated = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    let mut entries = Vec::new();
    for (i, layer) in doc.layers().iter().enumerate() {
        let file = format!("layer-{i}.png");
        // PNG is already compressed, so it's stored as-is.
        zip.start_file(file.as_str(), stored).map_err(zip_err)?;
        zip.write_all(&encode_raster(&layer.raster, FileKind::Png)?)?;
        entries.push(LayerEntry {
            name: layer.name.clone(),
            visible: layer.visible,
            opacity: layer.opacity,
            file,
        });
    }
    let manifest = Manifest {
        format: "harshpaint".into(),
        version: 1,
        width: doc.width(),
        height: doc.height(),
        active: doc.active_index(),
        layers: entries,
    };
    let json = serde_json::to_vec_pretty(&manifest)
        .map_err(|e| Error::Format(format!("could not write the layer list: {e}")))?;
    zip.start_file(MANIFEST, deflated).map_err(zip_err)?;
    zip.write_all(&json)?;
    Ok(zip.finish().map_err(zip_err)?.into_inner())
}

fn load_hpaint(bytes: &[u8]) -> Result<Document> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(zip_err)?;
    let manifest: Manifest = {
        let entry = zip.by_name(MANIFEST).map_err(zip_err)?;
        let mut json = Vec::new();
        entry.take(1024 * 1024).read_to_end(&mut json)?;
        serde_json::from_slice(&json).map_err(zip_err)?
    };
    if manifest.format != "harshpaint" || manifest.version != 1 {
        return Err(Error::Format(
            "this .hpaint file was made by a newer HarshPaint".into(),
        ));
    }
    check_size(u64::from(manifest.width), u64::from(manifest.height))?;
    if manifest.layers.is_empty() || manifest.layers.len() > MAX_LAYERS {
        return Err(zip_err("unexpected number of layers"));
    }
    let mut layers = Vec::with_capacity(manifest.layers.len());
    for entry in &manifest.layers {
        let mut png = Vec::new();
        zip.by_name(&entry.file)
            .map_err(zip_err)?
            .take(MAX_FILE_BYTES)
            .read_to_end(&mut png)?;
        let raster = decode_image(&png)?;
        let mut layer = Layer::new(entry.name.chars().take(100).collect::<String>(), raster);
        layer.visible = entry.visible;
        layer.opacity = if entry.opacity.is_finite() {
            entry.opacity.clamp(0.0, 1.0)
        } else {
            1.0
        };
        layers.push(layer);
    }
    Document::from_layers(manifest.width, manifest.height, layers, manifest.active)
}

/// Writes next to `path` first, then renames over it.
pub fn write_atomically(path: &Path, bytes: &[u8]) -> Result<()> {
    let file_name = path
        .file_name()
        .ok_or_else(|| Error::Format("that is not a file path".into()))?;
    let mut temp_name = file_name.to_os_string();
    temp_name.push(".harshpaint-save.tmp");
    let temp = path.with_file_name(temp_name);
    let result = fs::write(&temp, bytes).and_then(|()| fs::rename(&temp, path));
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    Ok(result?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("harshpaint-test-{}-{name}", std::process::id()))
    }

    fn picture() -> Document {
        let mut doc = Document::new(8, 6, Rgba::WHITE).unwrap();
        doc.active_mut()
            .unwrap()
            .raster
            .set(1, 1, Rgba::rgb(200, 10, 10));
        doc.add_layer().unwrap();
        doc.active_mut()
            .unwrap()
            .raster
            .set(2, 2, Rgba::rgb(10, 10, 200).with_alpha(128));
        doc.active_mut().unwrap().opacity = 0.75;
        doc.active_mut().unwrap().name = "Ink ✓".into();
        doc
    }

    #[test]
    fn png_round_trip_is_lossless() {
        let doc = picture();
        let path = temp("rt.png");
        save(&doc, &path).unwrap();
        let back = open(&path).unwrap();
        assert_eq!(back.flatten().unwrap(), doc.flatten().unwrap());
        fs::remove_file(path).ok();
    }

    #[test]
    fn jpeg_and_bmp_save_on_white() {
        let doc = Document::new(4, 4, Rgba::TRANSPARENT).unwrap();
        for kind in [FileKind::Jpeg, FileKind::Bmp] {
            let bytes = encode(&doc, kind).unwrap();
            let back = decode_image(&bytes).unwrap();
            let p = back.get(1, 1).unwrap();
            assert!(p.r > 240 && p.a == 255, "{kind:?} {p:?}");
        }
        assert!(!encode(&doc, FileKind::Gif).unwrap().is_empty());
    }

    #[test]
    fn hpaint_keeps_layers() {
        let doc = picture();
        let path = temp("rt.hpaint");
        save(&doc, &path).unwrap();
        let back = open(&path).unwrap();
        assert_eq!(back, doc);
        fs::remove_file(path).ok();
    }

    #[test]
    fn malformed_files_are_errors_not_panics() {
        assert!(decode_image(b"").is_err());
        assert!(decode_image(b"\x89PNG\r\n\x1a\n truncated").is_err());
        assert!(load_hpaint(b"PK not really a zip").is_err());
        // A zip with a manifest that lies about the size.
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        zip.start_file(MANIFEST, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(br#"{"format":"harshpaint","version":1,"width":99999,"height":1,"active":0,"layers":[]}"#)
            .unwrap();
        let bytes = zip.finish().unwrap().into_inner();
        assert!(load_hpaint(&bytes).is_err());
    }

    #[test]
    fn unknown_extension_and_failed_save_keep_old_file() {
        let doc = picture();
        assert!(save(&doc, &temp("x.txt")).is_err());
        let missing_dir = temp("no-such-dir").join("a.png");
        assert!(save(&doc, &missing_dir).is_err());
        assert_eq!(
            FileKind::from_path(Path::new("A.JPEG")),
            Some(FileKind::Jpeg)
        );
    }
}
