use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::Path;

use serde::{Deserialize, Serialize};

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

const JAM_KEY_START: u32 = 0xB082_F165;
pub const CANVAS_W: usize = 256;
pub mod palette;
pub mod png;
#[cfg(target_arch = "wasm32")]
pub mod wasm;

/// JAM texture record (32 bytes). Each record defines a texture region on a 256-wide canvas.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JamTexture {
    /// X coordinate on the 256-wide canvas.
    pub left: u8,
    /// Y coordinate on the canvas.
    pub top: u8,
    /// Unknown. Often 0.
    pub unk02: u16,
    /// Width of the texture region in pixels.
    pub width: u16,
    /// Height of the texture region in pixels.
    pub height: u16,
    /// Unknown. Often 0.
    pub unk08: u16,
    /// Unknown. Often 10496 in track textures, 0 in car textures. Might relate to mapping type.
    pub unk0a: u16,
    /// Offset within the canvas data where this texture's pixels start.
    pub image_ptr: u16,
    /// Unknown. Often 0.
    pub unk0e: u16,
    /// Size of a single haze palette (there are 4 such palettes per texture).
    /// Each entry in a haze palette is a byte that maps to the global palette.
    pub quarter_palette_size: u16,
    /// Unique identifier for this texture used by the game engine.
    pub texture_id: u16,
    /// Flag indicating if this texture has transparency (often 8 or 0, but can vary).
    pub transparent: u16,
    /// Unknown. Small value (e.g., 0, 40).
    pub unk16: u8,
    /// Unknown. Small value (e.g., 72, 201, 202).
    pub unk17: u8,
    /// 8 bytes of unknown data. Usually all zeros.
    pub unk18: [u8; 8],
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JamParsed {
    pub stem: String,
    pub num_textures: u16,
    pub canvas_h: u16,
    pub textures: Vec<JamTexture>,
    pub palette_data: Vec<u8>,
    pub canvas: Vec<u8>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MetaTexture {
    #[serde(flatten)]
    pub tx: JamTexture,
    #[serde(rename = "png")]
    pub png_name: String,
    #[serde(rename = "palette")]
    pub palette: Vec<u8>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JamMeta {
    #[serde(skip)]
    pub stem: String,
    #[serde(skip)]
    pub num_textures: u16,
    #[serde(skip)]
    pub canvas_h: u16,
    pub textures: Vec<MetaTexture>,
}

fn read_le16(buf: &[u8], off: usize) -> Result<u16> {
    let bytes = buf
        .get(off..off + 2)
        .ok_or_else(|| format!("out-of-range le16 read at {off}"))?;
    Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
}

fn write_le16(dst: &mut [u8], off: usize, value: u16) -> Result<()> {
    let bytes = dst
        .get_mut(off..off + 2)
        .ok_or_else(|| format!("out-of-range le16 write at {off}"))?;
    bytes.copy_from_slice(&value.to_le_bytes());
    Ok(())
}

/// Encrypts or decrypts a JAM buffer in-place.
/// Uses a rolling XOR key that is multiplied by 5 every 4 bytes.
pub fn decrypt_encrypt_jam(buf: &mut [u8]) {
    let mut key = JAM_KEY_START;
    for (i, b) in buf.iter_mut().enumerate() {
        let shift = 8 * (i % 4);
        *b ^= ((key >> shift) & 0xff) as u8;
        if (i % 4) == 3 {
            key = key.wrapping_mul(5);
        }
    }
}

/// Parses a decrypted JAM buffer.
///
/// Layout of a JAM file:
/// 1. Header (4 bytes): `num_textures` (u16), `canvas_h` (u16).
/// 2. Texture Records (32 bytes each): metadata for each texture.
/// 3. Palette Data: all local palettes for all textures, concatenated.
///    Each texture has 4 local palettes, each of `quarter_palette_size` bytes.
/// 4. Canvas Data: a 256-wide by `canvas_h` tall grid of pixel indices.
pub fn parse_jam_decrypted(stem: String, jam: &[u8]) -> Result<JamParsed> {
    if jam.len() < 4 {
        return Err("Decoded stream too small for JAM header".into());
    }

    let num_textures = read_le16(jam, 0)?;
    let canvas_h = read_le16(jam, 2)?;
    if canvas_h == 0 {
        return Err("Invalid JAM canvas height: 0".into());
    }

    let headers_off = 4usize;
    let headers_size = num_textures as usize * 32;
    if headers_off + headers_size > jam.len() {
        return Err("Decoded stream too small for JAM texture headers".into());
    }

    let mut palette_size = 0usize;
    for t in 0..num_textures as usize {
        let th = headers_off + t * 32;
        let qps = read_le16(jam, th + 16)? as usize;
        if qps > 256 {
            return Err(format!("Invalid palette quarter size in texture {}: {}", t, qps).into());
        }
        palette_size += qps * 4;
    }

    let canvas_size = CANVAS_W * canvas_h as usize;
    if headers_off + headers_size + palette_size + canvas_size > jam.len() {
        return Err(format!(
            "Decoded stream truncated (textures={}, canvas_h={}, dec={})",
            num_textures,
            canvas_h,
            jam.len()
        )
        .into());
    }

    let mut textures = Vec::with_capacity(num_textures as usize);
    for t in 0..num_textures as usize {
        let th = headers_off + t * 32;
        let mut unk18 = [0u8; 8];
        unk18.copy_from_slice(&jam[th + 24..th + 32]);
        textures.push(JamTexture {
            left: jam[th],
            top: jam[th + 1],
            unk02: read_le16(jam, th + 2)?,
            width: read_le16(jam, th + 4)?,
            height: read_le16(jam, th + 6)?,
            unk08: read_le16(jam, th + 8)?,
            unk0a: read_le16(jam, th + 10)?,
            image_ptr: read_le16(jam, th + 12)?,
            unk0e: read_le16(jam, th + 14)?,
            quarter_palette_size: read_le16(jam, th + 16)?,
            texture_id: read_le16(jam, th + 18)?,
            transparent: read_le16(jam, th + 20)?,
            unk16: jam[th + 22],
            unk17: jam[th + 23],
            unk18,
        });
    }

    let palette_start = headers_off + headers_size;
    let canvas_start = palette_start + palette_size;
    let palette_data = jam[palette_start..canvas_start].to_vec();
    let canvas = jam[canvas_start..canvas_start + canvas_size].to_vec();

    Ok(JamParsed {
        stem,
        num_textures,
        canvas_h,
        textures,
        palette_data,
        canvas,
    })
}

/// Extract the pixel data for a specific texture from the canvas,
/// using whatever indices (local or global) are currently stored.
pub fn extract_texture_image(parsed: &JamParsed, tex_idx: usize) -> Vec<u8> {
    let tx = &parsed.textures[tex_idx];
    let w = tx.width as usize;
    let h = tx.height as usize;
    let x = tx.left as usize;
    let y = tx.top as usize;

    (0..h)
        .flat_map(|yy| {
            let src = (y + yy) * CANVAS_W + x;
            parsed.canvas[src..src + w].iter().cloned()
        })
        .collect()
}

/// Return the byte offset into `palette_data` for a given texture index.
///
/// Palettes are stored sequentially: for each texture, `quarter_palette_size * 4` bytes.
pub fn texture_palette_offset(parsed: &JamParsed, tex_idx: usize) -> usize {
    parsed.textures[..tex_idx]
        .iter()
        .map(|tx| tx.quarter_palette_size as usize * 4)
        .sum()
}

/// Convert all texture regions in the canvas from local palette indices
/// (pointing into each texture's own sub-palette) to global GP2 palette indices.
pub fn canvas_to_global_indices(parsed: &JamParsed) -> Vec<u8> {
    let mut global = parsed.canvas.clone();
    for (i, tex) in parsed.textures.iter().enumerate() {
        let qps = tex.quarter_palette_size as usize;
        if qps == 0 {
            continue;
        }
        let off = texture_palette_offset(parsed, i);
        let map = &parsed.palette_data[off..off + qps];
        for y in 0..tex.height as usize {
            for x in 0..tex.width as usize {
                let dst = (tex.top as usize + y) * CANVAS_W + (tex.left as usize + x);
                let local = global[dst] as usize;
                if local < qps {
                    global[dst] = map[local];
                }
            }
        }
    }
    global
}

pub fn write_meta_json<W: Write>(f: W, stem: &str, parsed: &JamParsed) -> Result<()> {
    let mut pal_off = 0usize;
    let mut meta_textures = Vec::with_capacity(parsed.textures.len());
    for (t, tx) in parsed.textures.iter().enumerate() {
        let qps = tx.quarter_palette_size as usize;
        let palette = parsed.palette_data[pal_off..pal_off + qps].to_vec();
        pal_off += qps * 4;

        let png_name = format!("{}_{}.png", stem, t);

        meta_textures.push(MetaTexture {
            tx: tx.clone(),
            png_name,
            palette,
        });
    }

    let canvas_h = meta_textures
        .iter()
        .map(|mt| mt.tx.top as u16 + mt.tx.height)
        .max()
        .unwrap_or(0);
    let meta = JamMeta {
        stem: stem.to_string(),
        num_textures: meta_textures.len() as u16,
        canvas_h,
        textures: meta_textures,
    };

    serde_json::to_writer(f, &meta)?;
    Ok(())
}

pub fn write_meta_json_file(path: &Path, stem: &str, parsed: &JamParsed) -> Result<()> {
    let f = BufWriter::new(
        File::create(path).map_err(|e| format!("create {}: {}", path.display(), e))?,
    );
    write_meta_json(f, stem, parsed)
}

/// Decodes a JAM file into internal structure.
pub fn decode(jam_path: &Path) -> Result<JamParsed> {
    let mut data = fs::read(jam_path).map_err(|e| format!("open {}: {}", jam_path.display(), e))?;
    if data.is_empty() {
        return Err("File too small".into());
    }

    decrypt_encrypt_jam(&mut data);
    let stem = jam_path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "OUT".to_string());
    let parsed = parse_jam_decrypted(stem, &data)?;

    Ok(parsed)
}

pub fn parse_meta_json<R: std::io::Read>(reader: R, stem: &str) -> Result<JamMeta> {
    let mut meta: JamMeta = serde_json::from_reader(reader)?;
    meta.stem = stem.to_string();
    meta.num_textures = meta.textures.len() as u16;

    if meta.num_textures == 0 {
        return Err("Invalid metadata: no textures".into());
    }

    for (t, mt) in meta.textures.iter().enumerate() {
        let qps = mt.tx.quarter_palette_size as usize;
        if qps > 256
            || mt.tx.width == 0
            || mt.tx.height == 0
            || mt.tx.left as usize + mt.tx.width as usize > CANVAS_W
        {
            return Err(format!("Invalid texture values in metadata at texture {}", t).into());
        }
        if mt.palette.len() != qps {
            return Err(format!(
                "Palette size mismatch for texture {}: got {} expected {}",
                t,
                mt.palette.len(),
                qps
            )
            .into());
        }
    }

    meta.canvas_h = meta
        .textures
        .iter()
        .map(|mt| mt.tx.top as u16 + mt.tx.height)
        .max()
        .unwrap_or(0);

    Ok(meta)
}

/// Encodes a JAM file from the provided metadata and **global GP2 indexed** texture images.
///
/// The texture images must contain GP2 global indices (0-255), not local palette indices.
/// The function will automatically repalettize: build local palettes from the unique
/// global indices used, remap pixels to local indices, and encode the result.
///
/// The resulting JAM is encrypted.
pub fn encode(meta: &JamMeta, textures: &[Vec<u8>]) -> Result<(JamMeta, Vec<u8>)> {
    // Repalettize: global GP2 indices -> local indices, rebuilding palettes
    let (meta, textures) = repalettize_textures(meta, textures)?;

    let mut palette_data = Vec::new();
    for mt in &meta.textures {
        let qps = mt.tx.quarter_palette_size as usize;
        if mt.palette.len() != qps {
            return Err("invalid palette size in metadata".into());
        }
        // Replicate the single palette for all 4 hazes
        for _ in 0..4 {
            palette_data.extend_from_slice(&mt.palette);
        }
    }

    let mut canvas = vec![0u8; CANVAS_W * meta.canvas_h as usize];
    for (t, mt) in meta.textures.iter().enumerate() {
        let img = &textures[t];
        let w = mt.tx.width as usize;
        let h = mt.tx.height as usize;
        if img.len() != w * h {
            return Err(format!(
                "Image data size mismatch for texture {} ({} vs {}x{})",
                t,
                img.len(),
                w,
                h
            )
            .into());
        }
        let qps = mt.tx.quarter_palette_size as usize;
        for yy in 0..h {
            for xx in 0..w {
                let idx = img[yy * w + xx] as usize;
                if idx >= qps {
                    return Err(format!(
                        "Index out of range in texture {} at ({},{}): {} >= qps({})",
                        t, xx, yy, idx, qps
                    )
                    .into());
                }
                let dst = (mt.tx.top as usize + yy) * CANVAS_W + mt.tx.left as usize + xx;
                canvas[dst] = idx as u8;
            }
        }
    }

    let mut jam =
        vec![0u8; 4 + meta.num_textures as usize * 32 + palette_data.len() + canvas.len()];
    write_le16(&mut jam, 0, meta.num_textures)?;
    write_le16(&mut jam, 2, meta.canvas_h)?;

    for (t, mt) in meta.textures.iter().enumerate() {
        let th = 4 + t * 32;
        jam[th] = mt.tx.left;
        jam[th + 1] = mt.tx.top;
        write_le16(&mut jam, th + 2, mt.tx.unk02)?;
        write_le16(&mut jam, th + 4, mt.tx.width)?;
        write_le16(&mut jam, th + 6, mt.tx.height)?;
        write_le16(&mut jam, th + 8, mt.tx.unk08)?;
        write_le16(&mut jam, th + 10, mt.tx.unk0a)?;
        write_le16(&mut jam, th + 12, mt.tx.image_ptr)?;
        write_le16(&mut jam, th + 14, mt.tx.unk0e)?;
        write_le16(&mut jam, th + 16, mt.tx.quarter_palette_size)?;
        write_le16(&mut jam, th + 18, mt.tx.texture_id)?;
        write_le16(&mut jam, th + 20, mt.tx.transparent)?;
        jam[th + 22] = mt.tx.unk16;
        jam[th + 23] = mt.tx.unk17;
        jam[th + 24..th + 32].copy_from_slice(&mt.tx.unk18);
    }

    let pal_start = 4 + meta.num_textures as usize * 32;
    let canvas_start = pal_start + palette_data.len();
    jam[pal_start..canvas_start].copy_from_slice(&palette_data);
    jam[canvas_start..canvas_start + canvas.len()].copy_from_slice(&canvas);

    decrypt_encrypt_jam(&mut jam);
    Ok((meta.clone(), jam))
}

/// Convert local-indexed pixel data to global GP2 indices using a palette (local→global mapping).
pub fn local_to_global_indices(img_local: &[u8], palette: &[u8], qps: usize) -> Vec<u8> {
    img_local
        .iter()
        .map(|&local_idx| {
            if (local_idx as usize) < qps && qps <= palette.len() {
                palette[local_idx as usize]
            } else {
                0
            }
        })
        .collect()
}

/// Helper to map global-indexed image data to local-indexed data and rebuild local palettes.
fn repalettize_textures(
    meta_in: &JamMeta,
    texture_images_global: &[Vec<u8>],
) -> Result<(JamMeta, Vec<Vec<u8>>)> {
    if meta_in.textures.len() != texture_images_global.len() {
        return Err("Number of textures and images mismatch".into());
    }

    let mut new_texture_images = Vec::with_capacity(texture_images_global.len());
    let mut meta_out: JamMeta = meta_in.clone();
    for (t, mt) in meta_out.textures.iter_mut().enumerate() {
        let img_global = &texture_images_global[t];
        let mut unique_colors = Vec::new();
        for &global_idx in img_global {
            if !unique_colors.contains(&global_idx) {
                unique_colors.push(global_idx);
            }
        }

        let mut qps = mt.tx.quarter_palette_size as usize;
        if unique_colors.len() > qps {
            qps = unique_colors.len();
            // Cap at 256 as JAM format probably doesn't support more in a single haze
            if qps > 256 {
                return Err(format!("Texture {} uses too many colors: {}", t, qps).into());
            }
            mt.tx.quarter_palette_size = qps as u16;
        }

        let mut local_pal = unique_colors.clone();
        while local_pal.len() < qps {
            local_pal.push(0);
        }
        if local_pal.len() > qps {
            local_pal.truncate(qps);
        }

        let mut img_local = Vec::with_capacity(img_global.len());
        for &global_idx in img_global {
            let local_idx = local_pal.iter().position(|&c| c == global_idx).unwrap_or(0);
            img_local.push(local_idx as u8);
        }
        new_texture_images.push(img_local);

        mt.palette = local_pal;
    }

    Ok((meta_out, new_texture_images))
}
