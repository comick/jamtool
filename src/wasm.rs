use crate::{
    decrypt_encrypt_jam, encode_from_meta, parse_jam_decrypted, JamMeta, JamParsed, MetaTexture,
};
use serde_wasm_bindgen;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub fn decode_jam_wasm(jam_data: &[u8]) -> Result<JsValue, JsValue> {
    let mut data = jam_data.to_vec();
    decrypt_encrypt_jam(&mut data);
    let parsed = parse_jam_decrypted(&data).map_err(|e| JsValue::from_str(&e.to_string()))?;
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
