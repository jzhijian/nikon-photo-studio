# Local Nikon Image SDK helper

This Windows x64 helper calls a separately obtained Nikon NEF/NRW Image SDK. It
initializes the official library, reads image metadata, and optionally develops a
NEF using the original camera settings into a new sRGB PPM file. This is a baseline
RAW renderer. It does **not** import NP3, apply a cloud recipe, modify a camera, or
claim that a cloud style has been reproduced.

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

```text
nikon-image-probe.exe <SDK Release Bin directory> [NEF path] [new output.ppm]
```

- One argument: initialize and close the library.
- Two arguments: also open the image and report metadata.
- Three arguments: develop with `RawParameterSet_AsShot`, high quality, 8-bit
  RGB, and the SDK's `NKsRGB.icm` for both output and output device profiles.
  Write a binary P6 PPM only after the SDK has closed successfully.
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
{"schemaVersion":1,"sdkLoaded":true,"initialized":true,"inputOpened":true,"format":1048576,"width":5568,"height":3712,"byteDepth":1,"color":32,"orientation":1,"rendered":true,"outputPath":"D:\\example\\new-output.ppm","colorSpace":"sRGB","settings":"as-shot"}
```

The four booleans `sdkLoaded`, `initialized`, `inputOpened`, and `rendered` are
always present with `schemaVersion`. Metadata fields are present only when an
input was opened and the request succeeded. The metadata `byteDepth` is 2 for the
untouched NEF query and 1 after the requested 8-bit rendering. `outputPath`,
`colorSpace`, and `settings` appear only for a successful render. A failure adds
the string `error` and always reports `rendered: false`; it does not claim partial
success as a usable image. SDK warning responses are treated as failures.

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

No style-fidelity comparison has been made. Reading embedded NX Studio edits with
`LastSaveSetting`, importing NP3, and matching a cloud recipe remain separate
unimplemented work. Any future saved-edit mode must check that the edit state is
applicable and reject fallback warnings instead of reporting an applied style.
