# Local Nikon Image SDK helper

This Windows x64 helper calls a separately obtained Nikon NEF/NRW Image SDK. It
initializes the official library, reads image metadata, and optionally develops a
NEF into a new sRGB PPM file using the original camera settings or applicable
editing records already embedded in the input. It does **not** import NP3, apply a
cloud recipe itself, modify a camera, or claim that a cloud style has been reproduced.

No Nikon SDK source, header, profile, library, or example image is included here.
Obtain the SDK under Nikon's license before using it. The local adapter includes
the original SDK header at build time without modifying it.

## Build and prepare a local installation

From the repository root, using Windows x64 clang with MinGW/UCRT headers:

```powershell
./tools/nikon-image/build.ps1 `
  -SdkRoot '<extracted Image SDK directory>' `
  -Compiler '<clang++.exe>'
```

The default output is `.research-cloud/nikon-image-probe.exe`. An optional
`-OutputPath` selects a different executable output. The tested compiler was
MSYS2 UCRT64 clang 23.1.3. Only Win32, OLE, and C runtime facilities are used; no
C++ runtime is required.

The SDK 1.47 documentation requires its color profiles at
`C:\Program Files\Common Files\Nikon\Profiles`. If they are missing, explicitly
run `install-profiles.ps1 -SdkRoot '<SDK directory>'` in an administrator
PowerShell. It adds only missing files; existing files are preserved and their
hash matches are reported. The probe never installs profiles or requests elevation.

Copy **only this project's helper executable** into the SDK's existing
`Library/win/Bin/x64/Release` directory. Keep the Nikon DLLs and supporting data
from the SDK together there. Do not add the SDK package to the app repository or
release bundle. This placement is required for rendering: changing the working
directory and DLL search paths alone allowed initialization and metadata reads,
but `GetImageData` failed with `0x00000004`. With the helper beside the SDK files,
the same image and parameters rendered successfully. The helper reports an
actionable deployment error if rendering is requested from another directory.

The official SDK documentation supports Windows 11. The local Windows 10 test
described below confirms this specific baseline workflow on one machine; it does
not establish general Windows 10 support.

## CLI and integration contract

The desktop preset page now exposes a local rendering panel. Select this prepared
SDK Release directory, choose a NEF/NRW, and save to a new PNG. The Rust bridge
requires the requested mode to match the helper report, checks the pixel dimensions,
and embeds the SDK's sRGB ICC profile without changing RGB values. It writes via a
temporary file and refuses to replace an existing destination. PNG output is 8-bit
and does not copy camera EXIF. It does not automatically apply a selected cloud recipe.

The same application command is available as:
`nikon-photo-studio.exe --diagnostic nikon-render <SDK Release directory> <NEF> <new PNG> <as-shot|saved-edits>`.

```text
nikon-image-probe.exe <SDK Release Bin directory> [NEF path] [new output.ppm]
nikon-image-probe.exe <SDK Release Bin directory> <NEF path> <new output.ppm> --saved-edits
```

- One argument: initialize and close the library.
- Two arguments: also open the image and report metadata.
- Three arguments: develop with `RawParameterSet_AsShot`, high quality, 8-bit
  RGB, and the SDK's `NKsRGB.icm` for both output and output device profiles.
  Write a binary P6 PPM only after the SDK has closed successfully.
- Add the trailing `--saved-edits` flag to a render request to read embedded
  edits. The helper requires `GetEditState == ApplicableEdit` before selecting
  `RawParameterSet_LastSaveSetting`. Missing, inapplicable, or unknown edit states
  fail without rendering. The SDK does not read NX Studio `.nksc` sidecar files.
  Save edits into a separate NEF/NRW copy with NX Studio before using this mode.
- Saved-edit mode sets high quality before selecting `LastSaveSetting`, then
  refreshes image information. It does not issue further development adjustments
  after selecting saved edits. Any SDK warning, including `0x0102` (no edits) or
  `0x0103` (inapplicable edits), fails instead of accepting an as-shot fallback.
- Exit code `0`: the requested operation completed. Exit code `1`: failure;
  inspect `error`. Treat any other process termination as a failure as well.
- Standard output contains one UTF-8 JSON object followed by a newline. A caller
  should parse the last nonempty JSON line defensively if a future SDK logs text.
- Launch using argument arrays (for example Rust `Command::arg`), not a shell.
  Use the executable inside the selected SDK directory for a render. Capture both
  output streams and require exit code `0` and `rendered: true` before offering an
  output image. Use unique new output paths. Run away from the UI thread; the
  baseline full-resolution render took about 7–15 seconds on the test machine.
- Unicode input/output paths, normal spaces, and an ideographic space were tested.
- The input is never a save target. Existing output paths are rejected before SDK
  loading; `CREATE_NEW` additionally protects against a race. Partial newly-created
  PPM output is removed on a write failure. The file has an sRGB interpretation but
  PPM does not embed an ICC profile. A JPEG/TIFF conversion must preserve or embed
  the declared sRGB color space.

Success example:

```json
{"schemaVersion":1,"sdkLoaded":true,"initialized":true,"inputOpened":true,"format":1048576,"width":5568,"height":3712,"byteDepth":1,"color":32,"orientation":1,"rendered":true,"requestedSettings":"as-shot","outputPath":"D:\\example\\new-output.ppm","colorSpace":"sRGB","settings":"as-shot"}
```

The four booleans `sdkLoaded`, `initialized`, `inputOpened`, and `rendered` are
always present with `schemaVersion`. Metadata fields are present only when an
input was opened and the request succeeded. The metadata `byteDepth` is 2 for the
untouched NEF query and 1 after the requested 8-bit rendering. `outputPath`,
`colorSpace`, and `settings` appear only for a successful render. A failure adds
the string `error` and always reports `rendered: false`; it does not claim partial
success as a usable image. SDK warning responses are treated as failures.
For accepted render arguments, `requestedSettings` records `as-shot` or
`saved-edits`, including on a subsequent failure. `settings` uses the same values
only after a successful render. `editState` appears if the saved-edit state query
succeeded: `0` means absent, `1` applicable, and `2` inapplicable. Unknown values
are rejected. A successful `saved-edits` render indicates that the SDK accepted
the embedded edit record; it does not establish NP3 completeness or visual fidelity.

## Baseline verification, 2026-10-25

The signed Nikon Image SDK DLL reported file version `1.47.0.3000`. The sample was
the publicly provided Photography Blog Z30 RAW image:
<https://img.photographyblog.com/reviews/nikon_z30/sample_images/nikon_z30_01.nef>.
It was downloaded only into the ignored research directory.

- Input: 26,042,081 bytes; SHA-256 before and after rendering:
  `1B612601ADBA9BD8F9281DC154818BA02BA100FC8A16F7F8ECA403E401859708`.
- Output: 5568 × 3712 RGB, 8 bits per channel; PPM 62,005,265 bytes; SHA-256:
  `8905D3857B350927BAD0469FE6F90E7E16E042159ABC0A095321F3B986427E04`.
- A second render from a copied NEF at `显影　验证/尼康 Z30.nef` to a Chinese
  output path containing an ideographic space produced exactly the same hash.
- Pillow decoded both PPMs. Each RGB channel ranged from 0 to 255. A reduced
  preview was inspected and showed the expected outdoor tree scene.
- An existing-output request returned exit code 1 before SDK loading and left
  the output hash unchanged. A render with the helper outside the SDK directory
  returned exit code 1 with the deployment instruction and created no output.
- Local evidence is saved in `.research-cloud/image-render-verification.json`,
  with `.research-cloud/z30-sdk-as-shot.ppm` and its reduced preview.

## Saved-edit rejection and baseline regression, 2026-10-25

The same untouched Z30 sample returned `editState: 0` with `--saved-edits`.
The helper exited with code 1, reported `rendered: false`, and created no output.
Its input SHA-256 remained unchanged. The default as-shot render still produced
exactly the baseline PPM SHA-256 above. Results are recorded locally in
`.research-cloud/image-saved-edits-verification.json`.

A separate synthetic SDK stub tested missing, inapplicable, and unknown edit
states, an edit-state query warning, and post-selection warnings `0x0102`,
`0x0103`, and `0x0101`. All seven cases failed without producing an output.
The stub supplies no image pixels and is not an alternative rendering engine;
its ignored local evidence is `.research-cloud/image-probe-tests/results.json`.

A real NEF with NX Studio embedded edits has not yet been rendered with this
mode. NP3 import and comparison against an NX Studio reference export remain
separate validation work. Neither the rejection tests nor a successful baseline
render establish that a cloud recipe is reproduced faithfully.

## Desktop bridge verification

The production `render_nikon_raw` function rendered the same Z30 NEF to a new
5568 × 3712 PNG. Independent Pillow decoding confirmed that every RGB byte matches
the baseline PPM and that the embedded ICC bytes exactly match `NKsRGB.icm`.
The original NEF was unchanged and a second request for the same destination was
rejected without changing the PNG. Local evidence: `.research-cloud/app-render-verification.json`.
PNG SHA-256: `a49412258c98e5faa3fbdeaad5c10c30b0573211459fa9077e560cce4bd506c9`.

The opt-in Rust test uses `NPS_IMAGE_SDK_BIN`, `NPS_TEST_NEF`, and a new
`NPS_TEST_OUTPUT` PNG path. Run `cargo test real_sdk_png_render_preserves_source -- --ignored --nocapture`
from `src-tauri` after preparing the licensed SDK. Ordinary CI does not run this test.
