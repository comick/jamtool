use jamtool::{CANVAS_W, Result, parse_meta};
use std::fs;
use std::fs::File;
use std::io::BufReader;
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
    let parsed = jamtool::decode(infile)?;
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
    for t in 0..parsed.textures.len() {
        let tx = &parsed.textures[t];
        let w = tx.width as usize;
        let h = tx.height as usize;
        let x = tx.left as usize;
        let y = tx.top as usize;
        let qps = tx.quarter_palette_size as usize;

        if w == 0 || h == 0 || x + w > CANVAS_W || y + h > parsed.canvas_h as usize {
            eprintln!(
                "Skipping texture {}: invalid geometry ({},{}, {}x{})",
                t, x, y, w, h
            );
            continue;
        }
        if qps == 0 || qps > 256 {
            eprintln!(
                "Skipping texture {}: invalid palette quarter size {}",
                t, qps
            );
            continue;
        }

        // TODO: only first haze is actually needed
        for haze in 0..4usize {
            let (name, png_data) = jamtool::png::export_texture_png(&parsed, t, haze, &global_pal)?;
            let out = outdir.join(&name);
            std::fs::write(&out, &png_data)
                .map_err(|e| format!("write {}: {}", out.display(), e))?;
            println!("Writing {}", out.display());
        }
        written += 4;
    }

    println!("Wrote {} PNG files", written);
    Ok(())
}

fn encode(meta_path: &Path, out_jam: &Path) -> Result<()> {
    let f =
        File::open(meta_path).map_err(|e| format!("open meta {}: {}", meta_path.display(), e))?;
    let meta = parse_meta(BufReader::new(f))?;
    let meta_dir = meta_path.parent().unwrap_or(Path::new("."));

    // load png into encodable texture
    let textures = meta
        .textures
        .iter()
        .map(|mt| {
            let png_path = meta_dir.join(&mt.png_name);
            let (img, w, h) = jamtool::png::read_png_indexed(&png_path)?;

            if w != mt.tx.width as usize || h != mt.tx.height as usize {
                return Err(format!(
                    "PNG dimensions mismatch for texture {} ({}x{} vs {}x{})",
                    mt.png_name, w, h, mt.tx.width, mt.tx.height
                )
                .into());
            }
            Ok(img)
        })
        .collect::<Result<Vec<Vec<u8>>>>()?;

    // Convert local-indexed pixels to global GP2 indices using the meta's
    // haze-0 palette. encode() will repalettize internally.
    let global_textures: Vec<Vec<u8>> = meta
        .textures
        .iter()
        .zip(textures.iter())
        .map(|(mt, img_local)| {
            let qps = mt.tx.quarter_palette_size as usize;
            let haze0 = &mt.pals[0];
            let mut img_global = Vec::with_capacity(img_local.len());
            for &local_idx in img_local {
                let global_idx = if (local_idx as usize) < qps && qps <= haze0.len() {
                    haze0[local_idx as usize]
                } else {
                    0
                };
                img_global.push(global_idx);
            }
            img_global
        })
        .collect();

    let (meta, jam_data) = jamtool::encode(&meta, &global_textures)?;
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
