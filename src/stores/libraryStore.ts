import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { prefetchThumbs } from "../components/Thumbnail";

export interface Photo {
  filePath: string;
  filename: string;
  format: string;
  fileSize: number;
  width: number;
  height: number;
  previewable: boolean;
  folder: string;
  category: "portrait" | "landscape" | "other";
  classificationConfidence: number;
  hasPerson: boolean;
  blurScore: number;
  marked: boolean;
  rating: number;
  selected: boolean;
}

export type LibFilter = "all" | "NEF" | "JPG" | "marked";
export type RatingOperator = "all" | "gte" | "lte";

interface ImportedRaw {
  file_path: string;
  filename: string;
  format: string;
  file_size: number;
  width: number;
  height: number;
  previewable: boolean;
}

/* ── HSL channel adjustments (8 color ranges) ── */
export interface HslChannel {
  hue: number;        // -180..180
  saturation: number; // -100..100
  luminance: number;  // -100..100
}

export const HSL_CHANNELS = ["red","orange","yellow","green","aqua","blue","purple","magenta"] as const;
export type HslChannelName = (typeof HSL_CHANNELS)[number];

export const DEFAULT_HSL: Record<HslChannelName, HslChannel> = Object.fromEntries(
  HSL_CHANNELS.map((c) => [c, { hue: 0, saturation: 0, luminance: 0 }])
) as Record<HslChannelName, HslChannel>;

/* ── Color grading (split toning) ── */
export interface ColorGrading {
  highlightHue: number;  // 0..360
  highlightSat: number;  // 0..100
  shadowHue: number;     // 0..360
  shadowSat: number;     // 0..100
  balance: number;       // -100..100
}

export const DEFAULT_GRADING: ColorGrading = {
  highlightHue: 0, highlightSat: 0, shadowHue: 0, shadowSat: 0, balance: 0,
};

/* ── Tone curve ── */
export interface ToneCurve {
  points: [number, number][]; // control points (0..1, 0..1)
}

export const DEFAULT_CURVE: ToneCurve = {
  points: [[0, 0], [0.25, 0.25], [0.5, 0.5], [0.75, 0.75], [1, 1]],
};

/* ── Full adjustment params ── */
export interface AdjustParams {
  exposure: number;
  contrast: number;
  highlights: number;
  shadows: number;
  whites: number;
  blacks: number;
  temperature: number;
  tint: number;
  vibrance: number;
  saturation: number;
  clarity: number; texture: number;
  dehaze: number;
  hsl: Record<HslChannelName, HslChannel>;
  grading: ColorGrading;
  curve: ToneCurve;
}

export const DEFAULT_ADJUST: AdjustParams = {
  exposure: 0, contrast: 0, highlights: 0, shadows: 0,
  whites: 0, blacks: 0, temperature: 5500, tint: 0,
  vibrance: 0, saturation: 0, clarity: 0, texture: 0, dehaze: 0,
  hsl: { ...DEFAULT_HSL },
  grading: { ...DEFAULT_GRADING },
  curve: { points: [...DEFAULT_CURVE.points] },
};

/** Generate a CSS filter string from AdjustParams for live preview */
export function adjustToCssFilter(a: AdjustParams): string {
  const parts: string[] = [];
  // Exposure → brightness (1 EV ≈ ×2 brightness)
  if (a.exposure !== 0) {
    const b = Math.pow(2, a.exposure * 0.5); // softer mapping
    parts.push(`brightness(${b.toFixed(3)})`);
  }
  // Contrast
  if (a.contrast !== 0) {
    const c = 1 + a.contrast / 100;
    parts.push(`contrast(${c.toFixed(3)})`);
  }
  // Highlights (approximate with slight brightness)
  if (a.highlights !== 0) {
    const h = 1 + a.highlights / 400;
    parts.push(`brightness(${h.toFixed(3)})`);
  }
  // Shadows (approximate)
  if (a.shadows !== 0) {
    const s = 1 + a.shadows / 500;
    parts.push(`brightness(${s.toFixed(3)})`);
  }
  // Saturation + Vibrance
  const satTotal = a.saturation + a.vibrance * 0.6;
  if (satTotal !== 0) {
    const s = 1 + satTotal / 100;
    parts.push(`saturate(${Math.max(0, s).toFixed(3)})`);
  }
  // Temperature → sepia + hue-rotate approximation
  if (a.temperature !== 5500) {
    const delta = a.temperature - 5500;
    if (delta > 0) {
      // Warmer: slight sepia
      const sep = Math.min(0.4, delta / 10000);
      parts.push(`sepia(${sep.toFixed(3)})`);
    } else {
      // Cooler: hue-rotate toward blue
      const deg = Math.max(-30, delta / 150);
      parts.push(`hue-rotate(${deg.toFixed(1)}deg)`);
    }
  }
  // Tint
  if (a.tint !== 0) {
    const deg = a.tint * 0.3;
    parts.push(`hue-rotate(${deg.toFixed(1)}deg)`);
  }
  // Clarity → slight contrast boost
  if (a.clarity !== 0) {
    const c = 1 + a.clarity / 300;
    parts.push(`contrast(${c.toFixed(3)})`);
  }
  // Dehaze → contrast + saturate
  if (a.dehaze !== 0) {
    const c = 1 + a.dehaze / 200;
    parts.push(`contrast(${c.toFixed(3)})`);
    if (a.dehaze > 0) parts.push(`saturate(${(1 + a.dehaze / 300).toFixed(3)})`);
  }
  // Color grading → subtle hue-rotate
  if (a.grading.highlightSat > 0 || a.grading.shadowSat > 0) {
    const avgHue = (a.grading.highlightHue + a.grading.shadowHue) / 2;
    const avgSat = (a.grading.highlightSat + a.grading.shadowSat) / 2;
    if (avgSat > 5) {
      parts.push(`hue-rotate(${(avgHue * 0.05).toFixed(1)}deg)`);
    }
  }
  return parts.length > 0 ? parts.join(" ") : "none";
}

interface LibraryState {
  photos: Photo[];
  folder: string | null;
  filter: LibFilter;
  ratingOperator: RatingOperator;
  ratingValue: number;
  selectedPath: string | null;
  previewPath: string | null;
  developPath: string | null;
  importing: boolean;
  detecting: boolean;
  thumbSize: number;
  adjust: AdjustParams;
  anchorPath: string | null;
  /** Undo history stack for adjustments */
  adjustHistory: AdjustParams[];
  setThumbSize: (n: number) => void;
  setFolder: (f: string | null) => void;
  setFilter: (f: LibFilter) => void;
  setRatingFilter: (operator: RatingOperator, value?: number) => void;
  select: (p: string | null) => void;
  openPreview: (p: string | null) => void;
  setDevelopPath: (p: string | null) => void;
  toggleMark: (p: string) => void;
  setRating: (p: string, r: number) => void;
  toggleSelected: (p: string) => void;
  clickSelect: (path: string, shiftKey: boolean, ctrlKey: boolean, orderedPaths: string[]) => void;
  selectMany: (paths: string[]) => void;
  deselectAll: () => void;
  removePhoto: (p: string) => void;
  removeMany: (paths: string[]) => void;
  removeSelected: () => void;
  importFromDialog: () => Promise<number>;
  runBlurDetection: (sensitivity: string, useGpu?: boolean) => Promise<number>;
  runClassification: () => Promise<number>;
  setAdjust: (params: Partial<AdjustParams>) => void;
  setHsl: (channel: HslChannelName, params: Partial<HslChannel>) => void;
  setGrading: (params: Partial<ColorGrading>) => void;
  setCurve: (points: [number, number][]) => void;
  resetAdjust: () => void;
  undoAdjust: () => void;
}

function parentFolder(path: string): string {
  const norm = path.replace(/\\/g, "/");
  const idx = norm.lastIndexOf("/");
  return idx > 0 ? norm.slice(0, idx) : norm;
}

export const useLibraryStore = create<LibraryState>((set, get) => ({
  photos: [],
  folder: null,
  filter: "all",
  ratingOperator: "all",
  ratingValue: 3,
  selectedPath: null,
  previewPath: null,
  developPath: null,
  importing: false,
  detecting: false,
  thumbSize: 160,
  adjust: { ...DEFAULT_ADJUST, hsl: { ...DEFAULT_HSL }, grading: { ...DEFAULT_GRADING }, curve: { points: [...DEFAULT_CURVE.points] } },
  anchorPath: null,
  adjustHistory: [],

  setFolder: (folder) => set({ folder, selectedPath: null }),
  setFilter: (filter) => set({ filter }),
  setRatingFilter: (ratingOperator, value) =>
    set((s) => ({
      ratingOperator,
      ratingValue: value === undefined ? s.ratingValue : Math.min(5, Math.max(1, value)),
    })),
  setThumbSize: (thumbSize) => set({ thumbSize }),
  select: (selectedPath) => set({ selectedPath, anchorPath: selectedPath }),
  openPreview: (previewPath) => set({ previewPath }),
  setDevelopPath: (p) => set(p ? { developPath: p, selectedPath: p, anchorPath: p } : { developPath: null }),

  toggleMark: (p) =>
    set((s) => ({ photos: s.photos.map((ph) => (ph.filePath === p ? { ...ph, marked: !ph.marked } : ph)) })),

  setRating: (p, r) =>
    set((s) => ({ photos: s.photos.map((ph) => (ph.filePath === p ? { ...ph, rating: r } : ph)) })),

  toggleSelected: (p) =>
    set((s) => ({ photos: s.photos.map((ph) => (ph.filePath === p ? { ...ph, selected: !ph.selected } : ph)), anchorPath: p })),

  clickSelect: (path, shiftKey, ctrlKey, orderedPaths) => {
    const { anchorPath } = get();
    if (ctrlKey) { get().toggleSelected(path); set({ selectedPath: path }); return; }
    if (shiftKey && anchorPath) {
      const a = orderedPaths.indexOf(anchorPath);
      const b = orderedPaths.indexOf(path);
      if (a >= 0 && b >= 0) {
        const [lo, hi] = a < b ? [a, b] : [b, a];
        const rangeSet = new Set(orderedPaths.slice(lo, hi + 1));
        set((s) => ({ photos: s.photos.map((ph) => rangeSet.has(ph.filePath) ? { ...ph, selected: true } : ph), selectedPath: path }));
        return;
      }
    }
    set({ selectedPath: path, anchorPath: path });
  },

  selectMany: (paths) => {
    const pathSet = new Set(paths);
    set((s) => ({ photos: s.photos.map((ph) => pathSet.has(ph.filePath) ? { ...ph, selected: true } : ph) }));
  },

  deselectAll: () =>
    set((s) => ({ photos: s.photos.map((ph) => ({ ...ph, selected: false })), anchorPath: null })),

  removePhoto: (p) =>
    set((s) => ({
      photos: s.photos.filter((ph) => ph.filePath !== p),
      selectedPath: s.selectedPath === p ? null : s.selectedPath,
      previewPath: s.previewPath === p ? null : s.previewPath,
      anchorPath: s.anchorPath === p ? null : s.anchorPath,
    })),

  removeMany: (paths) => {
    const set0 = new Set(paths);
    set((s) => ({
      photos: s.photos.filter((ph) => !set0.has(ph.filePath)),
      selectedPath: s.selectedPath && set0.has(s.selectedPath) ? null : s.selectedPath,
      previewPath: s.previewPath && set0.has(s.previewPath) ? null : s.previewPath,
      anchorPath: s.anchorPath && set0.has(s.anchorPath) ? null : s.anchorPath,
    }));
  },

  removeSelected: () => {
    const { photos } = get();
    const selected = photos.filter((p) => p.selected).map((p) => p.filePath);
    if (selected.length === 0) return;
    get().removeMany(selected);
  },

  importFromDialog: async () => {
    if (get().importing) return 0;
    set({ importing: true });
    try {
      const picked = await open({ directory: true, multiple: false });
      if (!picked || typeof picked !== "string") return 0;
      const raw = await invoke<ImportedRaw[]>("import_folder", { path: picked });
      const existing = new Set(get().photos.map((p) => p.filePath));
      const added: Photo[] = raw
        .filter((r) => !existing.has(r.file_path))
        .map((r) => ({
          filePath: r.file_path, filename: r.filename, format: r.format,
          fileSize: r.file_size, width: r.width, height: r.height,
          previewable: r.previewable, folder: parentFolder(r.file_path),
          category: "other" as const, classificationConfidence: -1, hasPerson: false,
          blurScore: -1, marked: false, rating: 0, selected: false,
        }));
      set((s) => ({ photos: [...s.photos, ...added], folder: null }));
      // Warm thumbnails in the background using backend-parallel batched IPC.
      // Registered synchronously before React re-renders, so the grid cards that
      // mount next dedup onto this batch instead of each firing its own decode.
      void prefetchThumbs(added.map((p) => p.filePath), 360);
      return added.length;
    } finally { set({ importing: false }); }
  },

  runBlurDetection: async (sensitivity: string, useGpu = false) => {
    const { photos, detecting } = get();
    if (detecting || photos.length === 0) return 0;
    set({ detecting: true });
    let updated = 0;
    try {
      const paths = photos.map((p) => p.filePath);
      // Single batched call; Rust decodes + scores all photos in parallel
      // across CPU cores (optionally on the accelerated/GPU fast path).
      const results = await invoke<{ file_path: string; score: number }[]>(
        "batch_detect_blur",
        { paths, sensitivity, useGpu }
      );
      const scoreMap = new Map(results.map((r) => [r.file_path, r.score]));
      updated = scoreMap.size;
      set((s) => ({
        photos: s.photos.map((ph) => {
          const sc = scoreMap.get(ph.filePath);
          return sc !== undefined ? { ...ph, blurScore: sc } : ph;
        }),
      }));
    } catch {
      /* keep any previously-computed scores on failure */
    } finally {
      set({ detecting: false });
    }
    return updated;
  },

  runClassification: async () => {
    const { photos, detecting } = get();
    if (detecting || photos.length === 0) return 0;
    set({ detecting: true });
    let updated = 0;
    try {
      const paths = photos.map((p) => p.filePath);
      const results = await invoke<{
        file_path: string;
        category: "portrait" | "landscape" | "other";
        confidence: number;
        has_person: boolean;
      }[]>("batch_classify", { paths, modelDir: "" });
      const resultMap = new Map(results.map((r) => [r.file_path, r]));
      set((s) => ({
        photos: s.photos.map((ph) => {
          const result = resultMap.get(ph.filePath);
          if (!result) return ph;
          updated++;
          return {
            ...ph,
            category: result.category,
            classificationConfidence: result.confidence,
            hasPerson: result.has_person,
          };
        }),
      }));
    } catch (error) {
      console.error("Machine-learning classification failed", error);
    } finally { set({ detecting: false }); }
    return updated;
  },

  setAdjust: (params) =>
    set((s) => ({
      adjustHistory: [...s.adjustHistory.slice(-30), s.adjust],
      adjust: { ...s.adjust, ...params },
    })),

  setHsl: (channel, params) =>
    set((s) => ({
      adjustHistory: [...s.adjustHistory.slice(-30), s.adjust],
      adjust: { ...s.adjust, hsl: { ...s.adjust.hsl, [channel]: { ...s.adjust.hsl[channel], ...params } } },
    })),

  setGrading: (params) =>
    set((s) => ({
      adjustHistory: [...s.adjustHistory.slice(-30), s.adjust],
      adjust: { ...s.adjust, grading: { ...s.adjust.grading, ...params } },
    })),

  setCurve: (points) =>
    set((s) => ({
      adjustHistory: [...s.adjustHistory.slice(-30), s.adjust],
      adjust: { ...s.adjust, curve: { points } },
    })),

  resetAdjust: () =>
    set((s) => ({
      adjustHistory: [...s.adjustHistory.slice(-30), s.adjust],
      adjust: { ...DEFAULT_ADJUST, hsl: { ...DEFAULT_HSL }, grading: { ...DEFAULT_GRADING }, curve: { points: [...DEFAULT_CURVE.points] } },
    })),

  undoAdjust: () =>
    set((s) => {
      if (s.adjustHistory.length === 0) return s;
      const prev = s.adjustHistory[s.adjustHistory.length - 1];
      return { adjust: prev, adjustHistory: s.adjustHistory.slice(0, -1) };
    }),
}));

// ---- Derived helpers ----
export function selectFiltered(s: {
  photos: Photo[];
  folder: string | null;
  filter: LibFilter;
  ratingOperator: RatingOperator;
  ratingValue: number;
}): Photo[] {
  return s.photos.filter((p) => {
    if (s.folder && p.folder !== s.folder) return false;
    if (s.ratingOperator === "gte" && p.rating < s.ratingValue) return false;
    if (s.ratingOperator === "lte" && p.rating > s.ratingValue) return false;
    if (s.filter === "NEF") return p.format === "NEF";
    if (s.filter === "JPG") return p.format !== "NEF";
    if (s.filter === "marked") return p.marked;
    return true;
  });
}

export function selectFolders(photos: Photo[]): { folder: string; count: number }[] {
  const map = new Map<string, number>();
  for (const p of photos) map.set(p.folder, (map.get(p.folder) ?? 0) + 1);
  return [...map.entries()].map(([folder, count]) => ({ folder, count })).sort((a, b) => b.folder.localeCompare(a.folder));
}
