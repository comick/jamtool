use crate::{JamMeta, JamParsed, MetaTexture, decrypt_encrypt_jam, encode, parse_jam_decrypted};
use serde::{Deserialize, Serialize};
use serde_wasm_bindgen;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
#[derive(Serialize, Deserialize)]
pub struct ExportedFile {
    name: String,
    data: Vec<u8>,
}

#[wasm_bindgen]
impl ExportedFile {
    #[wasm_bindgen(getter)]
    pub fn name(&self) -> String {
        self.name.clone()
    }
    #[wasm_bindgen(getter)]
    pub fn data(&self) -> Vec<u8> {
        self.data.clone()
    }
}

#[wasm_bindgen]
pub fn decode_jam_wasm(jam_data: &[u8]) -> Result<JsValue, JsValue> {
    let data = jam_data.to_vec();
    let mut data_mut = data;
    decrypt_encrypt_jam(&mut data_mut);
    let mut parsed = parse_jam_decrypted("boh".to_string(), &data_mut)
        .map_err(|e| JsValue::from_str(&e.to_string()))?;
    parsed.canvas = crate::canvas_to_global_indices(&parsed);
    serde_wasm_bindgen::to_value(&parsed).map_err(|e| JsValue::from_str(&e.to_string()))
}

#[wasm_bindgen]
pub fn encode_jam_wasm(parsed_js: JsValue) -> Result<Vec<u8>, JsValue> {
    let parsed: JamParsed =
        serde_wasm_bindgen::from_value(parsed_js).map_err(|e| JsValue::from_str(&e.to_string()))?;

    // parsed.canvas now contains GLOBAL indices.
    // We need to extract per-texture global images and then repalettize them.

    let texture_images_global: Vec<Vec<u8>> = (0..parsed.textures.len())
        .map(|t| crate::extract_texture_image(&parsed, t))
        .collect();

    let meta_textures: Vec<MetaTexture> = parsed
        .textures
        .clone()
        .into_iter()
        .map(|tex| MetaTexture {
            tx: tex,
            png_name: String::new(),
            palette: vec![],
        })
        .collect();

    let meta = JamMeta {
        stem: String::new(),
        num_textures: parsed.num_textures,
        canvas_h: parsed.canvas_h,
        textures: meta_textures,
    };

    // encode() repalettizes internally: global GP2 indices -> local indices
    let (_, encoded) =
        encode(&meta, &texture_images_global).map_err(|e| JsValue::from_str(&e.to_string()))?;
    Ok(encoded)
}

#[wasm_bindgen]
pub fn get_default_palette_wasm() -> Result<Vec<u8>, JsValue> {
    Ok(crate::palette::GP2_PALETTE.to_vec())
}

/// Quantize RGBA pixel data to nearest GP2 palette indices.
///
/// `rgba` must be a flat Uint8Array with 4 bytes per pixel (R, G, B, A).
/// Pixels with alpha < `alpha_threshold` are assigned index 0.
/// Returns one byte (GP2 index 0-255) per pixel.
#[wasm_bindgen]
pub fn quantize_rgba_to_gp2_indices_wasm(
    rgba: &[u8],
    alpha_threshold: u8,
) -> Result<Vec<u8>, JsValue> {
    if rgba.len() % 4 != 0 {
        return Err(JsValue::from_str(
            "RGBA data length must be a multiple of 4",
        ));
    }

    let num_pixels = rgba.len() / 4;
    let pal = crate::palette::GP2_PALETTE;
    let mut indices = Vec::with_capacity(num_pixels);

    for i in 0..num_pixels {
        let off = i * 4;
        let r = rgba[off] as i32;
        let g = rgba[off + 1] as i32;
        let b = rgba[off + 2] as i32;
        let a = rgba[off + 3];

        if a < alpha_threshold {
            indices.push(0);
            continue;
        }

        let mut best_idx = 0u8;
        let mut best_dist = i32::MAX;
        for j in 0..256u16 {
            let dr = r - pal[j as usize * 3] as i32;
            let dg = g - pal[j as usize * 3 + 1] as i32;
            let db = b - pal[j as usize * 3 + 2] as i32;
            let dist = dr * dr + dg * dg + db * db;
            if dist < best_dist {
                best_dist = dist;
                best_idx = j as u8;
            }
        }
        indices.push(best_idx);
    }

    Ok(indices)
}

#[wasm_bindgen]
pub fn export_to_zip_files_wasm(parsed_js: JsValue, stem: &str) -> Result<JsValue, JsValue> {
    let parsed: JamParsed =
        serde_wasm_bindgen::from_value(parsed_js).map_err(|e| JsValue::from_str(&e.to_string()))?;

    let mut files = Vec::new();

    // 1. Metadata
    let mut meta_content = Vec::new();
    crate::write_meta_json(&mut meta_content, stem, &parsed)
        .map_err(|e| JsValue::from_str(&e.to_string()))?;
    files.push(ExportedFile {
        name: format!("{}.json", stem),
        data: meta_content,
    });

    // 2. PNGs (one per texture)
    let global_pal = crate::palette::GP2_PALETTE;
    for t in 0..parsed.textures.len() {
        let tx = &parsed.textures[t];
        let w = tx.width as usize;
        let h = tx.height as usize;
        let x = tx.left as usize;
        let y = tx.top as usize;
        let qps = tx.quarter_palette_size as usize;

        if w == 0 || h == 0 || x + w > crate::CANVAS_W || y + h > parsed.canvas_h as usize {
            continue;
        }
        if qps == 0 || qps > 256 {
            continue;
        }

        let transparent = tx.transparent != 0;
        let pal_off = crate::texture_palette_offset(&parsed, t);
        let img_global = crate::extract_texture_image(&parsed, t);

        // Convert global GP2 indices -> local indices using haze-0 palette,
        // so the PNG pixel values correctly index into the embedded palette.
        let pal_slice = &parsed.palette_data[pal_off..][..qps];
        let img_local: Vec<u8> = img_global
            .iter()
            .map(|&g| {
                pal_slice.iter().position(|&p| p == g).unwrap_or(0) as u8
            })
            .collect();

        // Build RGB palette: local index i -> RGB color from global GP2
        let rgb_pal = crate::png::build_palette(
            &parsed.palette_data, pal_off, 0, qps, &global_pal,
        );

        let png_data = crate::png::encode_indexed_png_to_bytes(
            &img_local, w, h, &rgb_pal, transparent,
        )
        .map_err(|e| JsValue::from_str(&e.to_string()))?;

        let png_name = format!("{}_{}.png", stem, t);

        files.push(ExportedFile {
            name: png_name,
            data: png_data,
        });
    }

    serde_wasm_bindgen::to_value(&files).map_err(|e| JsValue::from_str(&e.to_string()))
}

#[wasm_bindgen]
pub fn import_from_zip_files_wasm(meta_str: &str, stem: &str, pngs_js: JsValue) -> Result<JsValue, JsValue> {
    let meta =
        crate::parse_meta_json(meta_str.as_bytes(), stem).map_err(|e| JsValue::from_str(&e.to_string()))?;

    // pngs_js should be a Map or an Object: filename -> Uint8Array
    let pngs: serde_json::Value =
        serde_wasm_bindgen::from_value(pngs_js).map_err(|e| JsValue::from_str(&e.to_string()))?;

    let texture_images_global: Vec<Vec<u8>> = meta
        .textures
        .iter()
        .map(|mt| -> Result<Vec<u8>, JsValue> {
            let png_data = pngs
                .get(&mt.png_name)
                .ok_or_else(|| JsValue::from_str(&format!("Missing PNG: {}", mt.png_name)))?;

            let data_bytes = if let Some(arr) = png_data.as_array() {
                arr.iter()
                    .map(|v| v.as_u64().unwrap_or(0) as u8)
                    .collect::<Vec<_>>()
            } else {
                return Err(JsValue::from_str("Invalid PNG data type"));
            };

            let (buf, width, height) = crate::png::decode_indexed_png_from_bytes(&data_bytes)
                .map_err(|e| JsValue::from_str(&e.to_string()))?;

            if width as u16 != mt.tx.width || height as u16 != mt.tx.height {
                return Err(JsValue::from_str(&format!(
                    "PNG {} dimensions mismatch",
                    mt.png_name
                )));
            }

            Ok(buf)
        })
        .collect::<Result<Vec<_>, _>>()?;

    // encode() repalettizes internally: global GP2 indices -> local indices
    let (meta, encoded_jam) = crate::encode(&meta, &texture_images_global)
        .map_err(|e| JsValue::from_str(&e.to_string()))?;

    // Now decode it back to get JamParsed with the global canvas we expect in the editor
    // Or we could try to construct JamParsed directly.
    // Decoding it back is safer to ensure consistency with what the editor expects.
    let mut data = encoded_jam;
    decrypt_encrypt_jam(&mut data);
    let mut parsed =
        parse_jam_decrypted(meta.stem, &data).map_err(|e| JsValue::from_str(&e.to_string()))?;

    // Convert back to global canvas (like in decode_jam_wasm)
    parsed.canvas = crate::canvas_to_global_indices(&parsed);

    serde_wasm_bindgen::to_value(&parsed).map_err(|e| JsValue::from_str(&e.to_string()))
}
