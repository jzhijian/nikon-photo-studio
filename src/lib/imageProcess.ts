/**
 * imageProcess.ts — Real pixel-processing engine for the Develop viewer.
 *
 * Unlike the previous CSS-filter approximation, this engine actually rewrites
 * pixel values using established color-science principles so that every slider
 * in the right panel produces a genuine, Lightroom-like result.
 *
 * Pipeline order (matches Lightroom's general signal flow):
 *   1. White balance (temperature/tint)  — channel gains from blackbody model
 *   2. Exposure                          — multiplicative 2^EV
 *   3. Highlights / Shadows              — luminance-weighted, hue-preserving
 *   4. Tone LUT (per channel):
 *        blacks/whites (levels remap) -> contrast -> user tone curve -> dehaze tone
 *   5. HSL mixer (8 hue ranges)          — RGB<->HSL, weighted per-pixel
 *   6. Vibrance (sat-weighted) + Saturation (uniform)
 *   7. Color grading (split toning)      — highlight/shadow tint by luminance
 *   8. Clarity / Texture / Dehaze-local  — separable box-blur unsharp on luminance
 *
 * All math is done in gamma (sRGB) space for speed; this is a pragmatic choice
 * that matches what the user sees on screen and keeps the engine dependency-free.
 */

import type { AdjustParams, HslChannelName } from "../stores/libraryStore";

// ---------------------------------------------------------------------------
// Small math helpers
// ---------------------------------------------------------------------------

const clamp = (v: number, lo: number, hi: number): number =>
  v < lo ? lo : v > hi ? hi : v;

const clamp01 = (v: number): number => (v < 0 ? 0 : v > 1 ? 1 : v);

/** Smooth Hermite interpolation between edge0 and edge1. */
function smoothstep(edge0: number, edge1: number, x: number): number {
  const t = clamp01((x - edge0) / (edge1 - edge0));
  return t * t * (3 - 2 * t);
}

// ---------------------------------------------------------------------------
// White balance — blackbody (Tanner Helland approximation)
// ---------------------------------------------------------------------------

/** Approximate sRGB (0..1) of a blackbody radiator at the given kelvin. */
function kelvinToRgb(kelvin: number): [number, number, number] {
  const t = kelvin / 100;
  let r: number, g: number, b: number;
  // Red
  if (t <= 66) r = 255;
  else r = 329.698727446 * Math.pow(t - 60, -0.1332047592);
  // Green
  if (t <= 66) g = 99.4708025861 * Math.log(t) - 161.1195681661;
  else g = 288.1221695283 * Math.pow(t - 60, -0.0755148492);
  // Blue
  if (t >= 66) b = 255;
  else if (t <= 19) b = 0;
  else b = 138.5177312231 * Math.log(t - 10) - 305.0447927307;
  return [clamp(r, 0, 255) / 255, clamp(g, 0, 255) / 255, clamp(b, 0, 255) / 255];
}

/**
 * Compute per-channel white-balance gains.
 *
 * Interpretation: the slider value is treated as the illuminant color
 * temperature we are correcting *for*. A higher value (physically bluer light)
 * therefore adds warmth to the result — exactly how Lightroom's Temp slider
 * behaves (drag right => warmer). Gains are normalized by luminance so a
 * neutral gray keeps its brightness.
 */
function whiteBalanceGains(
  temperature: number,
  tint: number
): [number, number, number] {
  const NEUTRAL = 5500;
  if (temperature === NEUTRAL && tint === 0) return [1, 1, 1];
  const [nr, ng, nb] = kelvinToRgb(NEUTRAL);
  const [tr, tg, tb] = kelvinToRgb(clamp(temperature, 1000, 40000));
  let gr = nr / Math.max(tr, 1e-4);
  let gg = ng / Math.max(tg, 1e-4);
  let gb = nb / Math.max(tb, 1e-4);
  // Tint: positive => magenta (reduce green), negative => green.
  gg *= 1 - (tint / 100) * 0.4;
  // Normalize so a neutral gray preserves luminance.
  const lum = 0.299 * gr + 0.587 * gg + 0.114 * gb;
  if (lum > 1e-4) {
    gr /= lum;
    gg /= lum;
    gb /= lum;
  }
  return [gr, gg, gb];
}

// ---------------------------------------------------------------------------
// Tone curve — monotone cubic (Fritsch–Carlson) spline through control points
// ---------------------------------------------------------------------------

/**
 * Build a 256-entry LUT evaluating a monotone cubic spline through the given
 * (x,y) control points (both in 0..1). Monotonicity is enforced so the curve
 * never produces inverted/banded tones.
 */
function buildSplineLut(points: [number, number][]): Float32Array {
  const lut = new Float32Array(256);
  // Sort & de-duplicate by x.
  const pts = [...points]
    .map((p) => [clamp01(p[0]), clamp01(p[1])] as [number, number])
    .sort((a, b) => a[0] - b[0]);
  const unique: [number, number][] = [];
  for (const p of pts) {
    if (unique.length === 0 || p[0] > unique[unique.length - 1][0]) unique.push(p);
    else unique[unique.length - 1][1] = p[1];
  }
  if (unique.length < 2) {
    for (let i = 0; i < 256; i++) lut[i] = i / 255;
    return lut;
  }
  const n = unique.length;
  const xs = unique.map((p) => p[0]);
  const ys = unique.map((p) => p[1]);
  // Secants.
  const dx: number[] = [];
  const m: number[] = [];
  for (let i = 0; i < n - 1; i++) {
    dx.push(xs[i + 1] - xs[i]);
    m.push((ys[i + 1] - ys[i]) / (dx[i] || 1e-6));
  }
  // Tangents (Fritsch–Carlson).
  const tan: number[] = new Array(n);
  tan[0] = m[0];
  tan[n - 1] = m[n - 2];
  for (let i = 1; i < n - 1; i++) {
    if (m[i - 1] * m[i] <= 0) tan[i] = 0;
    else {
      const w1 = 2 * dx[i] + dx[i - 1];
      const w2 = dx[i] + 2 * dx[i - 1];
      tan[i] = (w1 + w2) / (w1 / m[i - 1] + w2 / m[i]);
    }
  }
  // Evaluate.
  let seg = 0;
  for (let i = 0; i < 256; i++) {
    const x = i / 255;
    while (seg < n - 2 && x > xs[seg + 1]) seg++;
    const h = dx[seg] || 1e-6;
    const t = (x - xs[seg]) / h;
    const t2 = t * t;
    const t3 = t2 * t;
    const h00 = 2 * t3 - 3 * t2 + 1;
    const h10 = t3 - 2 * t2 + t;
    const h01 = -2 * t3 + 3 * t2;
    const h11 = t3 - t2;
    let y =
      h00 * ys[seg] +
      h10 * h * tan[seg] +
      h01 * ys[seg + 1] +
      h11 * h * tan[seg + 1];
    lut[i] = clamp01(y);
  }
  return lut;
}

/**
 * Build the combined per-channel tone LUT:
 *   blacks/whites levels remap -> contrast -> user curve -> dehaze tone.
 */
function buildToneLut(s: AdjustParams): Float32Array {
  const curveLut = buildSplineLut(s.curve.points);

  // Levels: black point / white point (0..1).
  const bp = clamp(-(s.blacks / 100) * 0.3, 0, 0.4); // +blacks => crush darks
  const wp = clamp(1 - (s.whites / 100) * 0.3, 0.6, 1); // +whites => brighter
  const span = Math.max(wp - bp, 1e-4);

  // Contrast factor (classic formula, symmetric around 0.5).
  const c = s.contrast / 100; // -1..1
  const cf = (259 * (c * 128 + 255)) / (255 * (259 - c * 128));

  // Dehaze global tone: lift contrast + slight darkening.
  const d = s.dehaze / 100; // -1..1
  const dFactor = 1 + d * 0.35;
  const dOffset = -d * 0.04;

  const lut = new Float32Array(256);
  for (let i = 0; i < 256; i++) {
    let v = i / 255;
    // Levels remap.
    v = (v - bp) / span;
    // Contrast around midpoint.
    v = cf * (v - 0.5) + 0.5;
    // User tone curve.
    v = curveLut[clamp(Math.round(clamp01(v) * 255), 0, 255)];
    // Dehaze global tone.
    v = dFactor * (v - 0.5) + 0.5 + dOffset;
    lut[i] = clamp01(v);
  }
  return lut;
}

// ---------------------------------------------------------------------------
// HSL mixer
// ---------------------------------------------------------------------------

const HUE_CENTERS: Record<HslChannelName, number> = {
  red: 0,
  orange: 30,
  yellow: 60,
  green: 120,
  aqua: 180,
  blue: 240,
  purple: 275,
  magenta: 315,
};
const HUE_HALF_WIDTH = 45; // degrees of influence per channel

/** Shortest angular distance (0..180) between two hues in degrees. */
function hueDist(a: number, b: number): number {
  let d = Math.abs(a - b) % 360;
  if (d > 180) d = 360 - d;
  return d;
}

function rgbToHsl(r: number, g: number, b: number): [number, number, number] {
  const mx = Math.max(r, g, b);
  const mn = Math.min(r, g, b);
  const l = (mx + mn) / 2;
  let h = 0;
  let s = 0;
  const d = mx - mn;
  if (d > 1e-6) {
    s = l > 0.5 ? d / (2 - mx - mn) : d / (mx + mn);
    if (mx === r) h = (g - b) / d + (g < b ? 6 : 0);
    else if (mx === g) h = (b - r) / d + 2;
    else h = (r - g) / d + 4;
    h *= 60;
  }
  return [h, s, l];
}

function hue2rgb(p: number, q: number, t: number): number {
  if (t < 0) t += 1;
  if (t > 1) t -= 1;
  if (t < 1 / 6) return p + (q - p) * 6 * t;
  if (t < 1 / 2) return q;
  if (t < 2 / 3) return p + (q - p) * (2 / 3 - t) * 6;
  return p;
}

function hslToRgb(h: number, s: number, l: number): [number, number, number] {
  h = ((h % 360) + 360) % 360 / 360;
  if (s < 1e-6) return [l, l, l];
  const q = l < 0.5 ? l * (1 + s) : l + s - l * s;
  const p = 2 * l - q;
  return [hue2rgb(p, q, h + 1 / 3), hue2rgb(p, q, h), hue2rgb(p, q, h - 1 / 3)];
}

function hsvToRgb(h: number, s: number, v: number): [number, number, number] {
  h = (((h % 360) + 360) % 360) / 60;
  const i = Math.floor(h);
  const f = h - i;
  const p = v * (1 - s);
  const q = v * (1 - s * f);
  const t = v * (1 - s * (1 - f));
  switch (i % 6) {
    case 0: return [v, t, p];
    case 1: return [q, v, p];
    case 2: return [p, v, t];
    case 3: return [p, q, v];
    case 4: return [t, p, v];
    default: return [v, p, q];
  }
}

// ---------------------------------------------------------------------------
// Fast separable box blur (for clarity/texture local contrast)
// ---------------------------------------------------------------------------

function boxBlur(src: Float32Array, w: number, h: number, radius: number): Float32Array {
  const tmp = new Float32Array(src.length);
  const dst = new Float32Array(src.length);
  const win = radius * 2 + 1;
  // Horizontal.
  for (let y = 0; y < h; y++) {
    const row = y * w;
    let acc = 0;
    for (let x = -radius; x <= radius; x++) acc += src[row + clamp(x, 0, w - 1)];
    for (let x = 0; x < w; x++) {
      tmp[row + x] = acc / win;
      const add = src[row + clamp(x + radius + 1, 0, w - 1)];
      const sub = src[row + clamp(x - radius, 0, w - 1)];
      acc += add - sub;
    }
  }
  // Vertical.
  for (let x = 0; x < w; x++) {
    let acc = 0;
    for (let y = -radius; y <= radius; y++) acc += tmp[clamp(y, 0, h - 1) * w + x];
    for (let y = 0; y < h; y++) {
      dst[y * w + x] = acc / win;
      const add = tmp[clamp(y + radius + 1, 0, h - 1) * w + x];
      const sub = tmp[clamp(y - radius, 0, h - 1) * w + x];
      acc += add - sub;
    }
  }
  return dst;
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/** True when no slider differs from its neutral value. */
export function isNeutral(s: AdjustParams): boolean {
  if (
    s.exposure !== 0 ||
    s.contrast !== 0 ||
    s.highlights !== 0 ||
    s.shadows !== 0 ||
    s.whites !== 0 ||
    s.blacks !== 0 ||
    s.temperature !== 5500 ||
    s.tint !== 0 ||
    s.vibrance !== 0 ||
    s.saturation !== 0 ||
    s.clarity !== 0 ||
    s.texture !== 0 ||
    s.dehaze !== 0
  )
    return false;
  for (const k of Object.keys(s.hsl) as HslChannelName[]) {
    const c = s.hsl[k];
    if (c.hue !== 0 || c.saturation !== 0 || c.luminance !== 0) return false;
  }
  const g = s.grading;
  if (g.highlightSat !== 0 || g.shadowSat !== 0) return false;
  for (const p of s.curve.points) {
    if (Math.abs(p[0] - p[1]) > 1e-4) return false;
  }
  return true;
}

/**
 * Process an ImageData buffer **in place** according to the develop settings.
 * Safe to call repeatedly; designed to run on a downscaled canvas (~1800px).
 */
export function processImageData(img: ImageData, s: AdjustParams): void {
  if (isNeutral(s)) return;

  const data = img.data;
  const w = img.width;
  const h = img.height;
  const n = w * h;

  // --- Precompute ---
  const [wr, wg, wb] = whiteBalanceGains(s.temperature, s.tint);
  const expFactor = Math.pow(2, s.exposure);
  const toneLut = buildToneLut(s);

  // Precompute the active HSL channels once (avoids per-pixel Object.keys()).
  const hslEntries: { center: number; hue: number; sat: number; lum: number }[] = [];
  for (const k of Object.keys(s.hsl) as HslChannelName[]) {
    const ch = s.hsl[k];
    if (ch.hue !== 0 || ch.saturation !== 0 || ch.luminance !== 0) {
      hslEntries.push({
        center: HUE_CENTERS[k],
        hue: ch.hue,
        sat: ch.saturation / 100,
        lum: ch.luminance / 100,
      });
    }
  }
  const doHsl = hslEntries.length > 0;
  const doVibrance = s.vibrance !== 0;
  const doSat = s.saturation !== 0;
  const satFactor = 1 + s.saturation / 100;

  const doGrading = s.grading.highlightSat !== 0 || s.grading.shadowSat !== 0;
  const balanceShift = (s.grading.balance / 100) * 0.25; // shifts split midpoint
  const [shr, shg, shb] = hsvToRgb(s.grading.shadowHue, s.grading.shadowSat / 100, 1);
  const [hr, hg, hb] = hsvToRgb(s.grading.highlightHue, s.grading.highlightSat / 100, 1);
  const shR = shr * 255, shG = shg * 255, shB = shb * 255;
  const hiR = hr * 255, hiG = hg * 255, hiB = hb * 255;
  const shAmt = s.grading.shadowSat / 100;
  const hiAmt = s.grading.highlightSat / 100;

  const highlights = s.highlights / 100;
  const shadows = s.shadows / 100;

  // --- Main pixel pass ---
  for (let idx = 0; idx < n; idx++) {
    const o = idx * 4;
    // 1. White balance + 2. Exposure.
    let r = data[o] * wr * expFactor;
    let g = data[o + 1] * wg * expFactor;
    let b = data[o + 2] * wb * expFactor;

    // 3. Highlights / Shadows (hue-preserving luminance move).
    if (highlights !== 0 || shadows !== 0) {
      const L = (0.299 * r + 0.587 * g + 0.114 * b) / 255;
      const wht = smoothstep(0.5, 1.0, L);
      const wsh = smoothstep(0.5, 0.0, L);
      const dL = highlights * wht * 0.4 + shadows * wsh * 0.4;
      if (dL !== 0) {
        const newL = clamp01(L + dL);
        const scale = L > 0.002 ? newL / L : 1;
        r *= scale;
        g *= scale;
        b *= scale;
      }
    }

    // 4. Tone LUT (per channel).
    r = toneLut[clamp(r, 0, 255) | 0] * 255;
    g = toneLut[clamp(g, 0, 255) | 0] * 255;
    b = toneLut[clamp(b, 0, 255) | 0] * 255;

    // 5. HSL mixer.
    if (doHsl) {
      let [hh, ss, ll] = rgbToHsl(r / 255, g / 255, b / 255);
      if (ss > 0.01) {
        let hueShift = 0;
        let satMul = 0;
        let lumAdd = 0;
        for (let e = 0; e < hslEntries.length; e++) {
          const en = hslEntries[e];
          const dist = hueDist(hh, en.center);
          if (dist >= HUE_HALF_WIDTH) continue;
          const weight = 1 - dist / HUE_HALF_WIDTH;
          hueShift += weight * en.hue;
          satMul += weight * en.sat;
          lumAdd += weight * en.lum;
        }
        hh += hueShift;
        ss = clamp01(ss * (1 + satMul));
        ll = clamp01(ll + lumAdd * 0.5);
        const [nr2, ng2, nb2] = hslToRgb(hh, ss, ll);
        r = nr2 * 255;
        g = ng2 * 255;
        b = nb2 * 255;
      }
    }

    // 6. Vibrance (sat-weighted) + Saturation (uniform).
    if (doVibrance || doSat) {
      const gray = 0.299 * r + 0.587 * g + 0.114 * b;
      const mx = Math.max(r, g, b);
      const mn = Math.min(r, g, b);
      const curSat = mx > 1e-3 ? (mx - mn) / mx : 0;
      let factor = satFactor;
      if (doVibrance) factor += (s.vibrance / 100) * (1 - curSat);
      r = gray + (r - gray) * factor;
      g = gray + (g - gray) * factor;
      b = gray + (b - gray) * factor;
    }

    // 7. Color grading (split toning).
    if (doGrading) {
      const L = (0.299 * r + 0.587 * g + 0.114 * b) / 255;
      const wS = smoothstep(0.5 + balanceShift, 0.0, L) * shAmt;
      const wH = smoothstep(0.5 + balanceShift, 1.0, L) * hiAmt;
      if (wS > 0) {
        const a = wS * (1 - L) * 0.7;
        r += (shR - r) * a;
        g += (shG - g) * a;
        b += (shB - b) * a;
      }
      if (wH > 0) {
        const a = wH * L * 0.7;
        r += (hiR - r) * a;
        g += (hiG - g) * a;
        b += (hiB - b) * a;
      }
    }

    data[o] = clamp(r, 0, 255);
    data[o + 1] = clamp(g, 0, 255);
    data[o + 2] = clamp(b, 0, 255);
  }

  // --- 8. Clarity / Texture / Dehaze-local (local contrast via unsharp) ---
  const clarity = s.clarity / 100;
  const texture = s.texture / 100;
  const dehazeLocal = s.dehaze / 100;
  if (clarity !== 0 || texture !== 0 || dehazeLocal !== 0) {
    applyLocalContrast(data, w, h, clarity, texture, dehazeLocal);
  }
}

/**
 * Local-contrast (unsharp-mask on luminance) for clarity, texture and the
 * local component of dehaze. Clarity is weighted toward midtones; texture uses
 * a finer radius and even weighting; dehaze adds a broad, strong boost.
 */
function applyLocalContrast(
  data: Uint8ClampedArray,
  w: number,
  h: number,
  clarity: number,
  texture: number,
  dehaze: number
): void {
  const n = w * h;
  const lum = new Float32Array(n);
  for (let i = 0; i < n; i++) {
    const o = i * 4;
    lum[i] = (0.299 * data[o] + 0.587 * data[o + 1] + 0.114 * data[o + 2]) / 255;
  }

  // Radius scales with image size for consistent look.
  const base = Math.max(w, h);
  const rClarity = Math.max(4, Math.round(base / 90)); // broad midtone contrast
  const rTexture = Math.max(2, Math.round(base / 220)); // fine detail
  const rDehaze = Math.max(6, Math.round(base / 60)); // very broad

  const blurClarity = clarity !== 0 ? boxBlur(lum, w, h, rClarity) : null;
  const blurTexture = texture !== 0 ? boxBlur(lum, w, h, rTexture) : null;
  const blurDehaze = dehaze !== 0 ? boxBlur(lum, w, h, rDehaze) : null;

  for (let i = 0; i < n; i++) {
    const L = lum[i];
    let dL = 0;
    if (blurClarity) {
      // Midtone-weighted: strongest near L=0.5, gentle at extremes.
      const midWeight = 4 * L * (1 - L); // peaks at 0.5
      dL += (L - blurClarity[i]) * clarity * 1.1 * midWeight;
    }
    if (blurTexture) {
      dL += (L - blurTexture[i]) * texture * 0.7;
    }
    if (blurDehaze) {
      dL += (L - blurDehaze[i]) * dehaze * 0.9;
    }
    if (dL === 0) continue;
    const newL = clamp01(L + dL);
    const scale = L > 0.002 ? newL / L : 1;
    const o = i * 4;
    data[o] = clamp(data[o] * scale, 0, 255);
    data[o + 1] = clamp(data[o + 1] * scale, 0, 255);
    data[o + 2] = clamp(data[o + 2] * scale, 0, 255);
  }
}