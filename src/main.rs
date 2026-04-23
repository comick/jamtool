use jamtool::{parse_meta, Result, CANVAS_W};
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

    // Use try_fold to avoid mutable loops: state is (pal_off, written).
    let (_pal_off, written) = parsed.textures.iter().enumerate().try_fold(
        (0usize, 0usize),
        |(pal_off, written), (t, tx)| -> Result<(usize, usize)> {
            // if pal_off == usize::MAX we signalled to stop early; noop further entries
            if pal_off == usize::MAX {
                return Ok((pal_off, written));
            }

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
                return Ok((pal_off + qps * 4, written));
            }
            if qps == 0 || qps > 256 {
                eprintln!(
                    "Skipping texture {}: invalid palette quarter size {}",
                    t, qps
                );
                return Ok((pal_off, written));
            }
            if pal_off + qps * 4 > parsed.palette_data.len() {
                eprintln!("Skipping texture {}: palette data out of range", t);
                // signal stopping further processing by setting pal_off to a sentinel
                return Ok((usize::MAX, written));
            }

            // build image without mut loops
            let img: Vec<u8> = (0..h)
                .flat_map(|yy| {
                    let src = (y + yy) * CANVAS_W + x;
                    parsed.canvas[src..src + w].iter().cloned()
                })
                .collect();

            // write the 4 haze variants using iterator-based try_for_each
            (0..4usize).try_for_each(|haze| -> Result<()> {
                let rgb_pal = jamtool::png::build_palette(
                    &parsed.palette_data,
                    pal_off,
                    haze,
                    qps,
                    &global_pal,
                );

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
                Ok(())
            })?;

            Ok((pal_off + qps * 4, written + 4))
        },
    )?;

    println!("Wrote {} PNG files", written);
    Ok(())
}

fn encode(meta_path: &Path, out_jam: &Path) -> Result<()> {
    let f = File::open(meta_path).map_err(|e| format!("open meta {}: {}", meta_path.display(), e))?;
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

    let (meta, jam_data) = jamtool::encode(&meta, &textures)?;
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
