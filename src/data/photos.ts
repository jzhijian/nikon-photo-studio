// Shared photo data source used across Library / Organize / Transfer views.
// In production this is replaced by Tauri backend scan results.

export interface Photo {
  id: number;
  filename: string;
  format: "NEF" | "JPG";
  category: "portrait" | "landscape" | "other";
  blurScore: number; // 0-100, higher = sharper
  folder: string;
  marked: boolean;
  thumbUrl: string;
}

const HUES = [210, 340, 120, 45, 280, 190, 15, 160, 300, 60, 230, 90];
const FOLDERS = ["2026/07/21", "2026/07/20", "2026/07/19"];
const CATS: Photo["category"][] = ["portrait", "landscape", "other"];

function makeThumb(id: number, w = 300, h = 200, blur = 0): string {
  const hue = HUES[id % HUES.length];
  const sat = 40 + (id % 3) * 15;
  const light = 32 + (id % 4) * 8;
  const blurDef = blur > 0 ? `<filter id="b"><feGaussianBlur stdDeviation="${blur}"/></filter>` : "";
  const f = blur > 0 ? ` filter="url(#b)"` : "";
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="${w}" height="${h}">
    <defs>${blurDef}</defs>
    <rect width="${w}" height="${h}" fill="hsl(${hue},${sat}%,${light}%)"/>
    <g${f}>
      <circle cx="${w * 0.3 + (id % 4) * (w * 0.12)}" cy="${h * 0.38}" r="${h * 0.18}" fill="hsl(${(hue + 45) % 360},${sat + 10}%,${light + 22}%)"/>
      <rect x="${w * 0.12}" y="${h * 0.62}" width="${w * 0.76}" height="${h * 0.26}" rx="6" fill="hsl(${(hue + 130) % 360},${sat}%,${light + 12}%)"/>
    </g>
    <text x="${w / 2}" y="${h - 8}" text-anchor="middle" fill="rgba(255,255,255,0.85)" font-size="${Math.round(h * 0.06)}" font-family="sans-serif">DSC_${String(400 + id).padStart(4, "0")}</text>
  </svg>`;
  return `data:image/svg+xml,${encodeURIComponent(svg)}`;
}

export const PHOTOS: Photo[] = Array.from({ length: 48 }, (_, i) => {
  const isBlurry = i % 7 === 3 || i % 11 === 5;
  return {
    id: i,
    filename: `DSC_${String(400 + i).padStart(4, "0")}.${i % 2 === 0 ? "NEF" : "JPG"}`,
    format: i % 2 === 0 ? "NEF" : "JPG",
    category: CATS[i % CATS.length],
    blurScore: isBlurry ? 12 + (i % 3) * 6 : 55 + (i % 9) * 5,
    folder: FOLDERS[i % FOLDERS.length],
    marked: i % 9 === 0,
    thumbUrl: makeThumb(i, 300, 200, isBlurry ? 3 : 0),
  };
});

export const BLUR_THRESHOLD = 50;