use jamtool::{Result, CANVAS_W};
use std::fs;
use std::path::Path;

fn filename_stem(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "output".to_string())
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: jamtool <file.jam> [outdir]");
        eprintln!("       jamtool --encode <meta.txt> <out.jam>");
        std::process::exit(1);
    }

    if args[1] == "--encode" {
        if args.len() < 4 {
            eprintln!("Usage: jamtool --encode <meta.txt> <out.jam>");
            std::process::exit(1);
        }
        encode(Path::new(&args[2]), Path::new(&args[3]))?;
    } else {
        let infile = Path::new(&args[1]);
        let outdir = if args.len() > 2 {
            Path::new(&args[2])
        } else {
            Path::new(".")
        };
        decode(infile, outdir)?;
    }
    Ok(())
}

fn decode(infile: &Path, outdir: &Path) -> Result<()> {
    let parsed = jamtool::decode_jam(infile)?;
    println!(
        "JAM header: textures={} canvas={}x{}",
        parsed.num_textures, CANVAS_W, parsed.canvas_h
    );

    fs::create_dir_all(outdir).map_err(|e| format!("mkdir {}: {}", outdir.display(), e))?;

    let stem = filename_stem(infile);
    let global_pal = jamtool::palette::GP2_PALETTE;

    let meta_path = outdir.join(format!("{}.jammeta.txt", stem));
    jamtool::write_meta_file(&meta_path, &stem, &parsed)?;
    println!("Writing {}", meta_path.display());

    let mut written = 0usize;
    let mut pal_off = 0usize;
    for (t, tx) in parsed.textures.iter().enumerate() {
        let w = tx.width as usize;
        let h = tx.height as usize;
        let x = tx.left as usize;
        let y = tx.top as usize;
        let qps = tx.quarter_palette_size as usize;
        let transparent = tx.transparent != 0;

        if w == 0 || h == 0 || x + w > CANVAS_W || y + h > parsed.canvas_h as usize {
            eprintln!(
                "Skipping texture {}: invalid geometry ({},{}, {}x{})",
                t, x, y, w, h
            );
            pal_off += qps * 4;
            continue;
        }
        if qps == 0 || qps > 256 {
            eprintln!(
                "Skipping texture {}: invalid palette quarter size {}",
                t, qps
            );
            continue;
        }
        if pal_off + qps * 4 > parsed.palette_data.len() {
            eprintln!("Skipping texture {}: palette data out of range", t);
            break;
        }

        let mut img = vec![0u8; w * h];
        for yy in 0..h {
            let src = (y + yy) * CANVAS_W + x;
            let dst = yy * w;
            img[dst..dst + w].copy_from_slice(&parsed.canvas[src..src + w]);
        }

        for haze in 0..4usize {
            let rgb_pal =
                jamtool::png::build_palette(&parsed.palette_data, pal_off, haze, qps, &global_pal);

            let out = outdir.join(format!(
                "{}_t{:03}_id{:04}_h{}_{}x{}.png",
                stem,
                t,
                tx.texture_id,
                haze + 1,
                w,
                h
            ));
            jamtool::png::write_png_indexed(&out, &img, w, h, &rgb_pal, transparent)?;
            println!("Writing {}", out.display());
            written += 1;
        }

        pal_off += qps * 4;
    }

    println!("Wrote {} PNG files", written);
    Ok(())
}

fn encode(meta_path: &Path, out_jam: &Path) -> Result<()> {
    let meta = jamtool::parse_meta_file(meta_path)?;
    let meta_dir = meta_path.parent().unwrap_or(Path::new("."));

    let mut texture_images = Vec::with_capacity(meta.textures.len());
    for mt in &meta.textures {
        let png_path = meta_dir.join(&mt.png_name);
        let (img, w, h) = jamtool::png::read_png_indexed(&png_path)?;
        if w != mt.tx.width as usize || h != mt.tx.height as usize {
            return Err(format!(
                "PNG dimensions mismatch for texture {} ({}x{} vs {}x{})",
                mt.png_name, w, h, mt.tx.width, mt.tx.height
            )
            .into());
        }
        texture_images.push(img);
    }

    // New: Always repalettize when encoding from PNGs,
    // to support PNGs that were edited using a global palette.
    let mut meta = meta;
    let texture_images = jamtool::repalettize_textures(&mut meta, &texture_images)?;

    let jam_data = jamtool::encode_from_meta(&meta, &texture_images)?;
    fs::write(out_jam, &jam_data).map_err(|e| format!("write {}: {}", out_jam.display(), e))?;

    println!(
        "Encoded JAM {} from {} (stem={}, textures={}, canvas=256x{})",
        out_jam.display(),
        meta_path.display(),
        meta.stem,
        meta.num_textures,
        meta.canvas_h
    );
    Ok(())
}
