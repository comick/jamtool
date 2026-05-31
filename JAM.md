# JAM File Format

This document describes the `.JAM` file format as used by *Grand Prix 2* and as implemented by this project.

> Based on the original documentation at [grandprix2.de](https://www.grandprix2.de/Anleitung/tutus/Jam%20Structure/Jam%20Structure.html)
> and the [GP2JAM source code](https://github.com/tkellaway/gp2-utils/tree/master/GP2JAM) by Trevor Kellaway.

## Overview

A JAM file is an encrypted archive of indexed-color textures used by the GP2 game engine. Textures are stored as 
regions within a single **canvas** — a 2D grid 256 pixels wide with a height given in the file header. Each texture 
defines a rectangular area on this canvas, along with four **local palettes** (for distance-based "hazing") that map 
canvas pixel values to colors in a **global palette** shared by all JAM files.

---

## File Layout (decrypted)

```
+---------------------------+------------------------------+
| Header (4 bytes)          | num_textures, canvas_h       |
+---------------------------+------------------------------+
| Texture records           | 32 bytes × num_textures      |
+---------------------------+------------------------------+
| Palette data              | concatenated local palettes  |
+---------------------------+------------------------------+
| Canvas data               | 256 × canvas_h bytes         |
+---------------------------+------------------------------+
```

All multi-byte integer values are **little-endian**.

### 1. Header — 4 bytes

| Offset | Size | Type | Field |
|--------|------|------|-------|
| 0 | 2 | `u16` | `num_textures` — number of texture records |
| 2 | 2 | `u16` | `canvas_h` — height of the canvas in pixels |

The canvas is always 256 pixels wide. The total canvas data size is `256 × canvas_h` bytes.

### 2. Texture Records — 32 bytes each

| Offset | Size | Type | Field |
|--------|------|------|-------|
| 0 | 1 | `u8` | `left` — X position on the 256-wide canvas |
| 1 | 1 | `u8` | `top` — Y position on the canvas |
| 2 | 2 | `u16` | `unk02` — unknown, often 0 |
| 4 | 2 | `u16` | `width` — texture width in pixels |
| 6 | 2 | `u16` | `height` — texture height in pixels |
| 8 | 2 | `u16` | `unk08` — unknown, often 0 |
| 10 | 2 | `u16` | `unk0a` — unknown (10496 in track textures, 0 in car textures) |
| 12 | 2 | `u16` | `image_ptr` — offset within canvas data (unused by this implementation) |
| 14 | 2 | `u16` | `unk0e` — unknown, often 0 |
| 16 | 2 | `u16` | `quarter_palette_size` — number of entries per haze palette |
| 18 | 2 | `u16` | `texture_id` — unique ID used by the game engine to reference this texture |
| 20 | 2 | `u16` | `transparent` — transparency flag (8 = transparent, 0 = opaque) |
| 22 | 1 | `u8` | `unk16` — unknown (small value: 0, 40, etc.) |
| 23 | 1 | `u8` | `unk17` — unknown (72, 201, 202, etc.) |
| 24 | 8 | `u8[8]` | `unk18` — unknown, usually all zeros |

A texture occupies the region `(left, top)` to `(left+width, top+height)` on the canvas. The coordinates are measured from the top-left corner of the canvas.

`left` and `top` are `u8`, so their valid range is 0–255. Since the canvas is 256 wide, `left + width` must not exceed 256. `top + height` must not exceed `canvas_h`.

### 3. Palette Data

Palettes are stored **sequentially** per texture. For each texture `t`:

```
[palette_haze1: quarter_palette_size bytes]
[palette_haze2: quarter_palette_size bytes]
[palette_haze3: quarter_palette_size bytes]
[palette_haze4: quarter_palette_size bytes]
```

Total palette size for texture `t` = `quarter_palette_size × 4`.

Each palette entry is a single byte that acts as an **index into the global GP2 palette** (256 colors, 3 bytes per color — RGB). The global palette is **not stored in the JAM file**; it is hardcoded in the game engine (and in this implementation in `src/palette.rs`).

#### Hazing

The four local palettes implement a distance-based "hazing" effect:

- **Palette 1** (haze 0): used for textures close to the camera — sharp, full color.
- **Palette 2** (haze 1): mid-distance, slightly blurred.
- **Palette 3** (haze 2): further distance, more blurred.
- **Palette 4** (haze 3): far distance, most blurred.

Each successive haze produces a median-filtered version of the previous one. The hazing effect is achieved by having multiple entries in the local palettes that share the same *haze-0* mapping but diverge in higher hazes. The `quarter_palette_size` determines how many unique color quartets (one per haze) the texture uses.

### 4. Canvas Data

A flat array of `256 × canvas_h` bytes, stored row-major (row by row from top to bottom, each row 256 bytes left to right).

Each byte is a **local palette index**: it references an entry in the texture's local palette for the pixel position. The game engine reads this index and performs a two-step lookup:

```
local_palette[canvas_pixel] → global_palette_index
global_palette[global_palette_index] → RGB color
```

Only the haze-0 palette is used for actual rendering at close range. The canvas pixels always index into the **local** palette, never directly into the global palette.

---

## Encryption

JAM files are encrypted with a rolling XOR. The entire file (header, records, palettes, and canvas data) is encrypted as one continuous block. Encryption and decryption use the same operation (XOR is its own inverse).

**Algorithm:**

```
key = 0xB082F165
for each byte b at position i:
    shift = 8 × (i mod 4)
    b = b XOR ((key >> shift) & 0xFF)
    if (i mod 4) == 3:
        key = key × 5   (wrapping 32-bit multiplication)
```

The key is updated every 4 bytes by multiplying by 5.

---

## Pixel Color Pipeline

```
canvas pixel (byte)
        ↓  (index into local palette)
local palette entry (byte)  — 1 of 4 hazes per texture
        ↓  (index into global palette)
global palette entry (3 bytes — R, G, B)
        ↓
display color
```

---

## Roundtrip (decode → encode)

When the project decodes a JAM:

1. The raw file is decrypted with `decrypt_encrypt_jam()`.
2. Header, texture records, palettes, and canvas data are parsed.
3. The canvas stores **local palette indices** — these can be converted to **global GP2 indices** via `canvas_to_global_indices()` for display.

When encoding:

1. **Repalettization** occurs automatically: unique global GP2 indices used in each texture's pixel data are collected into a new local palette, and pixels are remapped to local indices.
2. The local palettes, records, and canvas are assembled into the binary layout.
3. The resulting buffer is encrypted with `decrypt_encrypt_jam()`.

Because repalettization re-indexes the local palette from scratch, the `quarter_palette_size` and local palette contents may change upon roundtrip, but the global-indexed image data is preserved.

---

## Metadata (.jammeta.txt)

When exporting, the project produces a `.jammeta.txt` file that preserves all texture headers and palette data for roundtrip fidelity:

```
JAMMETA 1
stem <filename>
num_textures <N>
canvas_h <H>
texture <idx> left <L> top <T> width <W> height <H> unk02 <V> unk08 <V> unk0a <V> image_ptr <P> unk0e <V> qps <Q> texture_id <ID> transparent <T> unk16 <V> unk17 <V> png <filename.png>
unk18 <b0> <b1> <b2> <b3> <b4> <b5> <b6> <b7>
pal1 <e0> <e1> ... <eN>
pal2 <e0> <e1> ... <eN>
pal3 <e0> <e1> ... <eN>
pal4 <e0> <e1> ... <eN>
```

Each texture record is followed by its 4 palette lines. The PNG pixel data uses **local indices** (0 to `quarter_palette_size - 1`) matching the embedded palette.
