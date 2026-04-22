use crate::{
    decrypt_encrypt_jam, encode_from_meta, parse_jam_decrypted, JamMeta, JamParsed, MetaTexture,
};
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
    let parsed = parse_jam_decrypted(&data_mut).map_err(|e| JsValue::from_str(&e.to_string()))?;
    // 1. Convert local indices to global indices in the canvas
    let mut global_canvas = parsed.canvas.clone();
    for tex in &parsed.textures {
        let qps = tex.quarter_palette_size as usize;
        let pal_offset = get_palette_offset_internal(&parsed, tex.texture_id);
        let local_to_global = &parsed.palette_data[pal_offset..pal_offset + qps];

        for y in 0..tex.height as usize {
            for x in 0..tex.width as usize {
                let canvas_y = tex.top as usize + y;
                let canvas_x = tex.left as usize + x;
                let canvas_idx = canvas_y * crate::CANVAS_W + canvas_x;
                let local_idx = global_canvas[canvas_idx] as usize;
                if local_idx < qps {
                    global_canvas[canvas_idx] = local_to_global[local_idx];
                }
            }
        }
    }

    let mut parsed_wasm = parsed;
    parsed_wasm.canvas = global_canvas;

    serde_wasm_bindgen::to_value(&parsed_wasm).map_err(|e| JsValue::from_str(&e.to_string()))
}

fn get_palette_offset_internal(parsed: &JamParsed, texture_id: u16) -> usize {
    let mut offset = 0;
    for tex in &parsed.textures {
        if tex.texture_id == texture_id {
            return offset;
        }
        offset += tex.quarter_palette_size as usize * 4;
    }
    0
}

#[wasm_bindgen]
pub fn encode_jam_wasm(parsed_js: JsValue) -> Result<Vec<u8>, JsValue> {
    let parsed: JamParsed =
        serde_wasm_bindgen::from_value(parsed_js).map_err(|e| JsValue::from_str(&e.to_string()))?;

    // parsed.canvas now contains GLOBAL indices.
    // We need to extract per-texture global images and then repalettize them.

    let mut texture_images_global = Vec::with_capacity(parsed.textures.len());
    for tex in &parsed.textures {
        let mut img = vec![0u8; tex.width as usize * tex.height as usize];
        for y in 0..tex.height as usize {
            for x in 0..tex.width as usize {
                let canvas_y = tex.top as usize + y;
                let canvas_x = tex.left as usize + x;
                img[y * tex.width as usize + x] =
                    parsed.canvas[canvas_y * crate::CANVAS_W + canvas_x];
            }
        }
        texture_images_global.push(img);
    }

    let mut meta_textures = Vec::with_capacity(parsed.textures.len());
    for tex in parsed.textures.clone() {
        meta_textures.push(MetaTexture {
            tx: tex,
            png_name: String::new(),
            pals: [vec![], vec![], vec![], vec![]],
        });
    }

    let mut meta = JamMeta {
        stem: String::new(),
        num_textures: parsed.num_textures,
        canvas_h: parsed.canvas_h,
        textures: meta_textures,
    };

    // Use the core library's repalettize_textures
    let texture_images = crate::repalettize_textures(&mut meta, &texture_images_global)
        .map_err(|e| JsValue::from_str(&e.to_string()))?;

    let encoded =
        encode_from_meta(&meta, &texture_images).map_err(|e| JsValue::from_str(&e.to_string()))?;
    Ok(encoded)
}

#[wasm_bindgen]
pub fn get_default_palette_wasm() -> Result<Vec<u8>, JsValue> {
    Ok(crate::palette::GP2_PALETTE.to_vec())
}

#[wasm_bindgen]
pub fn export_to_zip_files_wasm(parsed_js: JsValue, stem: &str) -> Result<JsValue, JsValue> {
    let parsed: JamParsed =
        serde_wasm_bindgen::from_value(parsed_js).map_err(|e| JsValue::from_str(&e.to_string()))?;

    let mut files = Vec::new();

    // 1. Metadata
    let mut meta_content = Vec::new();
    crate::write_meta(&mut meta_content, stem, &parsed)
        .map_err(|e| JsValue::from_str(&e.to_string()))?;
    files.push(ExportedFile {
        name: format!("{}.jammeta.txt", stem),
        data: meta_content,
    });

    // 2. PNGs (equivalent to CLI)
    let global_pal = crate::palette::GP2_PALETTE;
    let mut pal_off = 0usize;
    for (t, tx) in parsed.textures.iter().enumerate() {
        let w = tx.width as usize;
        let h = tx.height as usize;
        let x = tx.left as usize;
        let y = tx.top as usize;
        let qps = tx.quarter_palette_size as usize;
        let transparent = tx.transparent != 0;

        if w == 0 || h == 0 || x + w > crate::CANVAS_W || y + h > parsed.canvas_h as usize {
            pal_off += qps * 4;
            continue;
        }

        let mut img = vec![0u8; w * h];
        for yy in 0..h {
            let src = (y + yy) * crate::CANVAS_W + x;
            let dst = yy * w;
            img[dst..dst + w].copy_from_slice(&parsed.canvas[src..src + w]);
        }

        for haze in 0..4usize {
            let rgb_pal =
                crate::png::build_palette(&parsed.palette_data, pal_off, haze, qps, &global_pal);

            let png_data =
                crate::png::encode_indexed_png_to_bytes(&img, w, h, &rgb_pal, transparent)
                    .map_err(|e| JsValue::from_str(&e.to_string()))?;

            let png_name = format!(
                "{}_t{:03}_id{:04}_h{}_{}x{}.png",
                stem,
                t,
                tx.texture_id,
                haze + 1,
                w,
                h
            );

            files.push(ExportedFile {
                name: png_name,
                data: png_data,
            });
        }
        pal_off += qps * 4;
    }

    serde_wasm_bindgen::to_value(&files).map_err(|e| JsValue::from_str(&e.to_string()))
}

#[wasm_bindgen]
pub fn import_from_zip_files_wasm(meta_str: &str, pngs_js: JsValue) -> Result<JsValue, JsValue> {
    let mut meta =
        crate::parse_meta(meta_str.as_bytes()).map_err(|e| JsValue::from_str(&e.to_string()))?;

    // pngs_js should be a Map or an Object: filename -> Uint8Array
    let pngs: serde_json::Value =
        serde_wasm_bindgen::from_value(pngs_js).map_err(|e| JsValue::from_str(&e.to_string()))?;

    let mut texture_images_global = Vec::with_capacity(meta.textures.len());
    for mt in &meta.textures {
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

        texture_images_global.push(buf);
    }

    // Repalettize
    let texture_images_local = crate::repalettize_textures(&mut meta, &texture_images_global)
        .map_err(|e| JsValue::from_str(&e.to_string()))?;

    // Encode to JAM data (but we actually want JamParsed to update the editor)
    let encoded_jam = crate::encode_from_meta(&meta, &texture_images_local)
        .map_err(|e| JsValue::from_str(&e.to_string()))?;

    // Now decode it back to get JamParsed with the global canvas we expect in the editor
    // Or we could try to construct JamParsed directly.
    // Decoding it back is safer to ensure consistency with what the editor expects.
    let mut data = encoded_jam;
    decrypt_encrypt_jam(&mut data);
    let mut parsed = parse_jam_decrypted(&data).map_err(|e| JsValue::from_str(&e.to_string()))?;

    // Convert back to global canvas (like in decode_jam_wasm)
    let mut global_canvas = parsed.canvas.clone();
    for tex in &parsed.textures {
        let qps = tex.quarter_palette_size as usize;
        let pal_offset = get_palette_offset_internal(&parsed, tex.texture_id);
        let local_to_global = &parsed.palette_data[pal_offset..pal_offset + qps];

        for y in 0..tex.height as usize {
            for x in 0..tex.width as usize {
                let canvas_y = tex.top as usize + y;
                let canvas_x = tex.left as usize + x;
                let canvas_idx = canvas_y * crate::CANVAS_W + canvas_x;
                let local_idx = global_canvas[canvas_idx] as usize;
                if local_idx < qps {
                    global_canvas[canvas_idx] = local_to_global[local_idx];
                }
            }
        }
    }
    parsed.canvas = global_canvas;

    serde_wasm_bindgen::to_value(&parsed).map_err(|e| JsValue::from_str(&e.to_string()))
}
