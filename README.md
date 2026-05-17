# jamtool

`jamtool` is a Rust-based library and command-line utility for manipulating `.JAM` texture files, used by the game 
*Grand Prix 2*. It also includes an interactive WebAssembly-based web editor for editing textures pixel-by-pixel.

> **Disclaimer**: this is not the classic early 2000 hero-level take on reverse engineering that made *Grand Prix 2* a legend, 
> but my mid-2026 personal excuse to learn Rust with help from generative AI on something I've never dared to do before.
> Because of [renewed](https://grandprix2.racing/file/misc/view/x86gp2) [interest](https://store.steampowered.com/app/3603720/GCR2_Geoff_Crammond/), I hope this can be useful to someone :).

![Web Editor Demo](demo.png)

Try it out at [jamtool.playlinux.net](https://jamtool.playlinux.net)!

## Installation

### Prerequisites

- [Rust](https://www.rust-lang.org/tools/install) (latest stable)
- [wasm-pack](https://rustwasm.github.io/wasm-pack/installer/) (for building the web editor)

### Building the CLI

```bash
cargo build --release
```

The binary will be available at `target/release/jamtool`.

### Building the Web Editor

```bash
wasm-pack build --target web
```

This will generate the WASM and JavaScript glue code in the `pkg/` directory.

## Usage

### CLI

#### Exporting to PNG

```bash
./target/release/jamtool <INPUT.JAM> <OUTPUT_DIR>
```

This extracts all textures into the specified directory as PNGs and creates a `.jammeta.txt` file.
Output also includes images using the *haze* palettes, though they are not explicitly included in the JAM file and are ignored when encoding back to JAM.

#### Importing from PNG

```bash
./target/release/jamtool --encode <INPUT_DIR> <OUTPUT.JAM>
```

This takes the PNGs and the `.jammeta.txt` file from the directory and encodes them back into a single JAM file.

### Web Editor Features

- **Pixel painting** — select any GP2 palette color and draw on textures click-by-click or drag
- **Zoom & pan** — slider, CTRL + scroll wheel, and fit-to-canvas for precise editing
- **Paste images from clipboard** — paste any image, position it by dragging, resize freely with corner handles, and commit with Enter — preview is quantized to GP2 colors in real time
- **Load & save .JAM** — open existing files and save modified ones
- **Import/Export ZIP** — roundtrip to indexed PNGs with metadata for external editing
- **Texture browser** — sidebar with thumbnails and detail panel showing all header fields
- **Pixel inspector** — hover any pixel to see its global palette index and RGB color

### Running Locally

1. Build the WASM package (see above).
2. Serve the project root using a web server (browsers block WASM via `file://`):

**Python:**

```bash
python3 -m http.server
```

**Node.js:**

```bash
npx serve .
```

3. Open `http://localhost:8000` (or the provided port) in your browser.

## References

I'm doing nothing new here, none of this would have been possible without the following resources:

- [JAM Structure](https://www.grandprix2.de/Anleitung/tutus/Jam%20Structure/Jam%20Structure.html)
- [GP2JAM Source Code](https://github.com/tkellaway/gp2-utils/tree/master/GP2JAM)

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

