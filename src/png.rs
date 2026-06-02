use crate::{JamParsed, Result, extract_texture_image, texture_palette_offset};
use png::{BitDepth, ColorType, Decoder, Encoder};
use std::fs::File;
use std::io::{BufReader, BufWriter, Cursor};
use std::path::Path;

pub fn write_png_indexed(
    path: &Path,
    indices: &[u8],
    w: usize,
    h: usize,
    palette_rgb: &[u8; 256 * 3],
    transparent: bool,
) -> Result<()> {
    let f = File::create(path).map_err(|e| format!("create {}: {}", path.display(), e))?;
    let mut writer = {
        let mut encoder = Encoder::new(BufWriter::new(f), w as u32, h as u32);
        encoder.set_color(ColorType::Indexed);
        encoder.set_depth(BitDepth::Eight);
        encoder.set_palette(palette_rgb.to_vec());
        if transparent {
            let mut trns = vec![0xffu8; 256];
            trns[0] = 0;
            encoder.set_trns(trns);
        }
        encoder
            .write_header()
            .map_err(|e| format!("write png header {}: {}", path.display(), e))?
    };

    writer
        .write_image_data(indices)
        .map_err(|e| format!("write png data {}: {}", path.display(), e))?;

    Ok(())
}

pub fn read_png_indexed(path: &Path) -> Result<(Vec<u8>, usize, usize)> {
    let f = File::open(path).map_err(|e| format!("open png {}: {}", path.display(), e))?;
    let decoder = Decoder::new(BufReader::new(f));
    let mut reader = decoder
        .read_info()
        .map_err(|e| format!("read png info {}: {}", path.display(), e))?;
    let info0 = reader.info();
    if info0.color_type != ColorType::Indexed {
        return Err(format!("PNG {} is not indexed color", path.display()).into());
    }
    if info0.bit_depth != BitDepth::Eight {
        return Err(format!("PNG {} must be 8-bit indexed", path.display()).into());
    }

    let mut buf = vec![0u8; reader.output_buffer_size().unwrap_or(0)];
    let out = reader
        .next_frame(&mut buf)
        .map_err(|e| format!("read png frame {}: {}", path.display(), e))?;
    let width = out.width as usize;
    let height = out.height as usize;
    let size = out.buffer_size();
    buf.truncate(size);
    if size != width * height {
        return Err(format!("PNG {} row size mismatch", path.display()).into());
    }
    Ok((buf, width, height))
}

pub fn encode_indexed_png_to_bytes(
    indices: &[u8],
    w: usize,
    h: usize,
    palette_rgb: &[u8; 768],
    transparent: bool,
) -> Result<Vec<u8>> {
    let mut png_data = Vec::new();
    {
        let mut encoder = Encoder::new(&mut png_data, w as u32, h as u32);
        encoder.set_color(ColorType::Indexed);
        encoder.set_depth(BitDepth::Eight);
        encoder.set_palette(palette_rgb.to_vec());
        if transparent {
            let mut trns = vec![0xffu8; 256];
            trns[0] = 0;
            encoder.set_trns(trns);
        }
        let mut writer = encoder
            .write_header()
            .map_err(|e| format!("write png header: {}", e))?;
        writer
            .write_image_data(indices)
            .map_err(|e| format!("write png data: {}", e))?;
    }
    Ok(png_data)
}

pub fn decode_indexed_png_from_bytes(data: &[u8]) -> Result<(Vec<u8>, u32, u32)> {
    let mut reader = Decoder::new(Cursor::new(data))
        .read_info()
        .map_err(|e| format!("read png info: {}", e))?;

    let info = reader.info();
    if info.color_type != ColorType::Indexed {
        return Err("PNG is not indexed color".into());
    }

    let mut buf = vec![0u8; reader.output_buffer_size().unwrap_or(0)];
    let out = reader
        .next_frame(&mut buf)
        .map_err(|e| format!("read png frame: {}", e))?;
    buf.truncate(out.buffer_size());

    Ok((buf, out.width, out.height))
}

/// Encode a single texture+haze combination as an indexed PNG, returning
/// the suggested filename and the raw PNG bytes.
pub fn export_texture_png(
    parsed: &JamParsed,
    tex_idx: usize,
    haze: usize,
    global_pal: &[u8],
) -> Result<(String, Vec<u8>)> {
    let tx = &parsed.textures[tex_idx];
    let w = tx.width as usize;
    let h = tx.height as usize;
    let qps = tx.quarter_palette_size as usize;
    let transparent = tx.transparent != 0;
    let pal_off = texture_palette_offset(parsed, tex_idx);

    let img = extract_texture_image(parsed, tex_idx);
    let rgb_pal = build_palette(&parsed.palette_data, pal_off, haze, qps, global_pal);
    let png_data = encode_indexed_png_to_bytes(&img, w, h, &rgb_pal, transparent)?;

    let filename = format!("{}_{}.png", parsed.stem, tex_idx);

    Ok((filename, png_data))
}

pub fn build_palette(
    palette_data: &[u8],
    pal_off: usize,
    haze: usize,
    qps: usize,
    global_pal: &[u8],
) -> [u8; 768] {
    let mut rgb_pal = [0u8; 768];
    for idx in 0..qps {
        let gp2_idx = palette_data[pal_off + haze * qps + idx] as usize;
        rgb_pal[idx * 3] = global_pal[gp2_idx * 3];
        rgb_pal[idx * 3 + 1] = global_pal[gp2_idx * 3 + 1];
        rgb_pal[idx * 3 + 2] = global_pal[gp2_idx * 3 + 2];
    }
    rgb_pal
}
