use std::fs::{self, File};
use std::io::{BufRead, BufWriter, Write};
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

#[derive(Clone, Debug)]
pub struct MetaTexture {
    pub tx: JamTexture,
    pub png_name: String,
    pub pals: [Vec<u8>; 4],
}

#[derive(Clone, Debug)]
pub struct JamMeta {
    pub stem: String,
    pub num_textures: u16,
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

pub fn write_meta<W: Write>(mut f: W, stem: &str, parsed: &JamParsed) -> Result<()> {
    writeln!(f, "JAMMETA 1")?;
    writeln!(f, "stem {}", stem)?;
    writeln!(f, "num_textures {}", parsed.num_textures)?;
    writeln!(f, "canvas_h {}", parsed.canvas_h)?;

    let mut pal_off = 0usize;
    for (t, tx) in parsed.textures.iter().enumerate() {
        let qps = tx.quarter_palette_size as usize;
        writeln!(
            f,
            "texture {} left {} top {} width {} height {} unk02 {} unk08 {} unk0a {} image_ptr {} unk0e {} qps {} texture_id {} transparent {} unk16 {} unk17 {} png {}_t{:03}_id{:04}_h1_{}x{}.png",
            t,
            tx.left,
            tx.top,
            tx.width,
            tx.height,
            tx.unk02,
            tx.unk08,
            tx.unk0a,
            tx.image_ptr,
            tx.unk0e,
            tx.quarter_palette_size,
            tx.texture_id,
            tx.transparent,
            tx.unk16,
            tx.unk17,
            stem,
            t,
            tx.texture_id,
            tx.width,
            tx.height
        )?;

        write!(f, "unk18")?;
        for v in tx.unk18 {
            write!(f, " {}", v)?;
        }
        writeln!(f)?;

        for haze in 0..4usize {
            write!(f, "pal{}", haze + 1)?;
            for i in 0..qps {
                write!(f, " {}", parsed.palette_data[pal_off + haze * qps + i])?;
            }
            writeln!(f)?;
        }

        pal_off += qps * 4;
    }

    Ok(())
}

// TODO drop
pub fn write_meta_file(path: &Path, stem: &str, parsed: &JamParsed) -> Result<()> {
    let f = BufWriter::new(
        File::create(path).map_err(|e| format!("create {}: {}", path.display(), e))?,
    );
    write_meta(f, stem, parsed)
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

fn parse_meta_texture(line: &str) -> Result<(usize, JamTexture, String)> {
    let parts = line.split_whitespace().collect::<Vec<_>>();
    if parts.len() < 32 || parts[0] != "texture" {
        return Err("Invalid texture line in metadata".into());
    }

    let mut i = 1usize;
    let parse_u16 = |s: &str| -> Result<u16> { Ok(s.parse::<u16>()?) };
    let parse_u8 = |s: &str| -> Result<u8> {
        let v = s.parse::<u16>()?;
        if v > 255 {
            return Err(format!("value {} out of u8 range", v).into());
        }
        Ok(v as u8)
    };

    let tex_index = parts[i].parse::<usize>()?;
    i += 1;

    let expect = |parts: &[&str], i: &mut usize, key: &str| -> Result<()> {
        if parts.get(*i).copied() != Some(key) {
            return Err(format!("Expected {} in texture line", key).into());
        }
        *i += 1;
        Ok(())
    };

    expect(&parts, &mut i, "left")?;
    let left = parse_u8(parts[i])?;
    i += 1;
    expect(&parts, &mut i, "top")?;
    let top = parse_u8(parts[i])?;
    i += 1;
    expect(&parts, &mut i, "width")?;
    let width = parse_u16(parts[i])?;
    i += 1;
    expect(&parts, &mut i, "height")?;
    let height = parse_u16(parts[i])?;
    i += 1;
    expect(&parts, &mut i, "unk02")?;
    let unk02 = parse_u16(parts[i])?;
    i += 1;
    expect(&parts, &mut i, "unk08")?;
    let unk08 = parse_u16(parts[i])?;
    i += 1;
    expect(&parts, &mut i, "unk0a")?;
    let unk0a = parse_u16(parts[i])?;
    i += 1;
    expect(&parts, &mut i, "image_ptr")?;
    let image_ptr = parse_u16(parts[i])?;
    i += 1;
    expect(&parts, &mut i, "unk0e")?;
    let unk0e = parse_u16(parts[i])?;
    i += 1;
    expect(&parts, &mut i, "qps")?;
    let qps = parse_u16(parts[i])?;
    i += 1;
    expect(&parts, &mut i, "texture_id")?;
    let texture_id = parse_u16(parts[i])?;
    i += 1;
    expect(&parts, &mut i, "transparent")?;
    let transparent = parse_u16(parts[i])?;
    i += 1;
    expect(&parts, &mut i, "unk16")?;
    let unk16 = parse_u8(parts[i])?;
    i += 1;
    expect(&parts, &mut i, "unk17")?;
    let unk17 = parse_u8(parts[i])?;
    i += 1;
    expect(&parts, &mut i, "png")?;
    let png_name = parts[i].to_string();

    let tx = JamTexture {
        left,
        top,
        unk02,
        width,
        height,
        unk08,
        unk0a,
        image_ptr,
        unk0e,
        quarter_palette_size: qps,
        texture_id,
        transparent,
        unk16,
        unk17,
        unk18: [0u8; 8],
    };

    Ok((tex_index, tx, png_name))
}

pub fn parse_meta<R: BufRead>(reader: R) -> Result<JamMeta> {
    let mut lines = reader.lines();

    let hdr = lines.next().transpose()?.ok_or_else(|| "empty meta file")?;
    if hdr.trim() != "JAMMETA 1" {
        return Err("Invalid metadata format header".into());
    }

    let stem_line = lines
        .next()
        .transpose()?
        .ok_or_else(|| "Missing stem in metadata")?;
    let stem = stem_line
        .strip_prefix("stem ")
        .ok_or_else(|| "Missing stem in metadata")?
        .trim()
        .to_string();

    let nt_line = lines
        .next()
        .transpose()?
        .ok_or_else(|| "Missing num_textures in metadata")?;
    let num_textures: u16 = nt_line
        .strip_prefix("num_textures ")
        .ok_or_else(|| "Missing num_textures in metadata")?
        .trim()
        .parse()?;

    let ch_line = lines
        .next()
        .transpose()?
        .ok_or_else(|| "Missing canvas_h in metadata")?;
    let canvas_h: u16 = ch_line
        .strip_prefix("canvas_h ")
        .ok_or_else(|| "Missing canvas_h in metadata")?
        .trim()
        .parse()?;

    if num_textures == 0 || canvas_h == 0 {
        return Err(format!(
            "Invalid metadata: num_textures={} canvas_h={}",
            num_textures, canvas_h
        )
        .into());
    }

    let mut textures = Vec::with_capacity(num_textures as usize);
    for t in 0..num_textures as usize {
        let tline = lines
            .next()
            .transpose()?
            .ok_or_else(|| "Invalid texture line in metadata")?;
        let (tex_index, mut tx, png_name) = parse_meta_texture(&tline)?;
        if tex_index != t {
            return Err(format!("Invalid texture values in metadata at texture {}", t).into());
        }
        let qps = tx.quarter_palette_size as usize;
        if qps > 256
            || tx.width == 0
            || tx.height == 0
            || tx.left as usize + tx.width as usize > CANVAS_W
            || tx.top as usize + tx.height as usize > canvas_h as usize
        {
            return Err(format!("Invalid texture values in metadata at texture {}", t).into());
        }

        let unk_line = lines
            .next()
            .transpose()?
            .ok_or_else(|| format!("Missing unk18 line for texture {}", t))?;
        let parts = unk_line.split_whitespace().collect::<Vec<_>>();
        if parts.len() != 9 || parts[0] != "unk18" {
            return Err(format!("Invalid unk18 values for texture {}", t).into());
        }
        for i in 0..8usize {
            tx.unk18[i] = parts[i + 1].parse::<u16>()?.min(255) as u8;
        }

        let mut pals = [
            Vec::<u8>::new(),
            Vec::<u8>::new(),
            Vec::<u8>::new(),
            Vec::<u8>::new(),
        ];
        for haze in 0..4usize {
            let pline = lines
                .next()
                .transpose()?
                .ok_or_else(|| format!("Missing palette line for texture {}", t))?;
            let pparts = pline.split_whitespace().collect::<Vec<_>>();
            let expected = format!("pal{}", haze + 1);
            if pparts.first().copied() != Some(expected.as_str()) {
                return Err(format!("Expected {} line for texture {}", expected, t).into());
            }
            if pparts.len() < 1 + qps {
                return Err(format!(
                    "Not enough palette entries for texture {} haze {}",
                    t,
                    haze + 1
                )
                .into());
            }
            let mut p = Vec::with_capacity(qps);
            for idx in 0..qps {
                let v = pparts[idx + 1].parse::<u16>()?;
                p.push((v & 0xff) as u8);
            }
            pals[haze] = p;
        }

        textures.push(MetaTexture { tx, png_name, pals });
    }

    Ok(JamMeta {
        stem,
        num_textures,
        canvas_h,
        textures,
    })
}

/// Encodes a JAM file from its internal structure.
///
/// It rebuilds the JAM canvas and palettes from the provided metadata and image data.
/// The resulting JAM is encrypted.
pub fn encode(meta: &JamMeta, textures: &[Vec<u8>]) -> Result<(JamMeta, Vec<u8>)> {
    // Always repalettize to support images that were edited using a global palette.
    let (meta, textures) = repalettize_textures(&meta, &textures)?;

    let mut palette_data = Vec::new();
    for mt in &meta.textures {
        let qps = mt.tx.quarter_palette_size as usize;
        for haze in 0..4usize {
            if mt.pals[haze].len() != qps {
                return Err("invalid palette size in metadata".into());
            }
            palette_data.extend_from_slice(&mt.pals[haze]);
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
    Ok((meta, jam))
}

/// Helper to map global-indexed image data to local-indexed data and rebuild local palettes.
/// This is useful when you have images edited externally with a global palette and want to
/// encode them into a JAM.
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

        for haze in 0..4 {
            mt.pals[haze] = local_pal.clone();
        }
    }

    Ok((meta_out, new_texture_images))
}
