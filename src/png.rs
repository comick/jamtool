use jamtool::Result;
use png::{BitDepth, ColorType, Decoder, Encoder};
use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::path::Path;

pub fn write_png_indexed(
    path: &Path,
    indices: &[u8],
    w: usize,
    h: usize,
    palette_rgb: &[u8; 768],
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
