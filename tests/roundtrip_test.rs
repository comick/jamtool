use jamtool::encode;
use jamtool::png;
use jamtool::{decode, parse_meta};
use std::fs;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

fn filename_stem(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "output".to_string())
}

fn run_decode(infile: &Path, outdir: &Path) {
    let parsed = decode(infile).expect("Failed to decode JAM");
    let stem = filename_stem(infile);

    fs::create_dir_all(outdir).expect("Failed to create outdir");

    let global_pal = jamtool::palette::GP2_PALETTE;

    let meta_path = outdir.join(format!("{}.jammeta.txt", stem));
    jamtool::write_meta_file(&meta_path, &stem, &parsed).expect("Failed to write meta");

    let mut pal_off = 0usize;
    for (t, tx) in parsed.textures.iter().enumerate() {
        let w = tx.width as usize;
        let h = tx.height as usize;
        let x = tx.left as usize;
        let y = tx.top as usize;
        let qps = tx.quarter_palette_size as usize;
        let transparent = tx.transparent != 0;

        if w == 0 || h == 0 || x + w > jamtool::CANVAS_W || y + h > parsed.canvas_h as usize {
            pal_off += qps * 4;
            continue;
        }

        let mut img = vec![0u8; w * h];
        for yy in 0..h {
            let src = (y + yy) * jamtool::CANVAS_W + x;
            let dst = yy * w;
            img[dst..dst + w].copy_from_slice(&parsed.canvas[src..src + w]);
        }

        for haze in 0..4usize {
            let rgb_pal = png::build_palette(&parsed.palette_data, pal_off, haze, qps, &global_pal);

            let out = outdir.join(format!(
                "{}_t{:03}_id{:04}_h{}_{}x{}.png",
                stem,
                t,
                tx.texture_id,
                haze + 1,
                w,
                h
            ));
            png::write_png_indexed(&out, &img, w, h, &rgb_pal, transparent)
                .expect("Failed to write PNG");
        }
        pal_off += qps * 4;
    }
}

fn run_encode(meta_path: &Path, out_jam: &Path) {
    let f = File::open(meta_path).expect("Failed to open meta file");
    let meta = parse_meta(BufReader::new(f)).expect("Failed to parse meta file");
    let meta_dir = meta_path.parent().unwrap_or(Path::new("."));

    // Read PNGs (pixel data contains local indices, 0..qps-1)
    let local_textures: Vec<Vec<u8>> = meta
        .textures
        .iter()
        .map(|mt| {
            let png_path = meta_dir.join(&mt.png_name);
            let (img, _, _) = png::read_png_indexed(&png_path).expect("Failed to read PNG");
            img
        })
        .collect();

    // Convert local indices -> global GP2 indices using the meta's haze-0 palette
    let global_textures: Vec<Vec<u8>> = meta
        .textures
        .iter()
        .zip(local_textures.iter())
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

    // encode() repalettizes internally: global GP2 indices -> local indices
    let (_, jam_data) = encode(&meta, &global_textures).expect("Failed to encode JAM");
    fs::write(out_jam, &jam_data).expect("Failed to write JAM");
}

fn compare_dirs(dir1: &Path, dir2: &Path, prefix1: &str, prefix2: &str) {
    let entries1 = fs::read_dir(dir1).unwrap();
    let mut files1: Vec<_> = entries1
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().into_string().unwrap())
        .collect();
    files1.sort();

    for name1 in files1 {
        let path1 = dir1.join(&name1);
        if path1.is_dir() {
            continue;
        }

        let name2 = if !prefix1.is_empty() && name1.starts_with(prefix1) {
            format!("{}{}", prefix2, &name1[prefix1.len()..])
        } else {
            name1.clone()
        };

        let path2 = dir2.join(&name2);

        assert!(
            path2.exists(),
            "File {:?} (mapped from {:?}) missing in second directory",
            name2,
            name1
        );

        if name1.ends_with(".jammeta.txt") || name1.ends_with(".png") {
            // Meta files might have different PNG filenames in them if prefix changed
            // PNGs might have different palette indices but represent the same image
            continue;
        }

        let content1 = fs::read(path1).unwrap();
        let content2 = fs::read(path2).unwrap();

        assert_eq!(
            content1, content2,
            "Content mismatch for file {:?} vs {:?}",
            name1, name2
        );
    }
}

fn run_roundtrip_test(jam_name: &str) {
    let jam_path_str = format!("tests/data/{}.JAM", jam_name);
    let jam_path = Path::new(&jam_path_str);
    let golden_dir_str = format!("tests/data/golden/{}", jam_name);
    let golden_dir = Path::new(&golden_dir_str);
    let test_out_str = format!("target/test_out_{}", jam_name.to_lowercase());
    let test_out = Path::new(&test_out_str);

    if test_out.exists() {
        fs::remove_dir_all(test_out).unwrap();
    }
    fs::create_dir_all(test_out).unwrap();

    // 1. Decode
    run_decode(jam_path, test_out);

    // Generate missing PNGs/meta if golden doesn't exist
    if !golden_dir.exists() {
        println!("Generating missing golden data for {}", jam_name);
        fs::create_dir_all(golden_dir).expect("Failed to create golden dir");
        // We can just copy everything from test_out to golden_dir
        for entry in fs::read_dir(test_out).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.is_file() {
                let dest = golden_dir.join(path.file_name().unwrap());
                fs::copy(path, dest).unwrap();
            }
        }
    }

    // Verify against golden
    compare_dirs(golden_dir, test_out, jam_name, jam_name);

    let meta_path = test_out.join(format!("{}.jammeta.txt", jam_name));
    assert!(meta_path.exists());

    // 2. Encode
    let repacked_jam = test_out.join(format!("{}_repacked.JAM", jam_name));
    run_encode(&meta_path, &repacked_jam);
    assert!(repacked_jam.exists());

    // 3. Decode again to verify
    let redecoded_dir = test_out.join("redecoded");
    run_decode(&repacked_jam, &redecoded_dir);

    // Verify re-decoded output matches original extraction
    compare_dirs(
        golden_dir,
        &redecoded_dir,
        jam_name,
        &format!("{}_repacked", jam_name),
    );
}

#[test]
fn test_roundtrip_dtrees() {
    run_roundtrip_test("DTREES");
}

#[test]
fn test_roundtrip_ferrari() {
    run_roundtrip_test("FERRARI");
}

#[test]
fn test_roundtrip_hormag() {
    run_roundtrip_test("HORMAG");
}
