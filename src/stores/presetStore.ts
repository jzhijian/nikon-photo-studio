import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { toast } from "./toastStore";

export type PresetGroup = "all" | "cloud" | "imported" | "favorites";
export type PresetFormat = "all" | "np3" | "ncp" | "xmp" | "cube" | "lrpreset" | "3dl" | "costyle";
export type PresetCategory = "all" | "portrait" | "landscape" | "film" | "mono" | "vintage" | "creative";

/** Develop parameters that a preset carries */
export interface PresetParams {
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
  clarity: number;
  dehaze: number;
  sharpening: number;
}

export const DEFAULT_PRESET_PARAMS: PresetParams = {
  exposure: 0, contrast: 0, highlights: 0, shadows: 0,
  whites: 0, blacks: 0, temperature: 5500, tint: 0,
  vibrance: 0, saturation: 0, clarity: 0, dehaze: 0, sharpening: 0,
};

export interface PresetInfo {
  id: number;
  name: string;
  format: PresetFormat;
  group: PresetGroup;
  category: PresetCategory;
  color: string;
  favorite: boolean;
  params: PresetParams;
  /** Original file path for imported presets */
  sourcePath?: string;
}

export const PRESET_CATEGORIES: { id: PresetCategory; icon: string; zh: string; en: string }[] = [
  { id: "all", icon: "🎨", zh: "全部", en: "All" },
  { id: "portrait", icon: "👤", zh: "人像", en: "Portrait" },
  { id: "landscape", icon: "🏞️", zh: "风光", en: "Landscape" },
  { id: "film", icon: "🎞️", zh: "胶片", en: "Film" },
  { id: "mono", icon: "⬛", zh: "黑白", en: "Mono" },
  { id: "vintage", icon: "📻", zh: "复古", en: "Vintage" },
  { id: "creative", icon: "✨", zh: "创意", en: "Creative" },
];

const PRESET_COLORS = [
  "#e8a87c", "#85cdca", "#d9a7c7", "#c7b8ea", "#f6c6ea",
  "#a8d8ea", "#fcbad3", "#ffffd2", "#a8e6cf", "#ffd3b6",
  "#ffaaa5", "#ff8b94", "#b5e8c3", "#f0c27f", "#4b6cb7",
];

let nextId = 100;

const BUILTIN_PRESETS: PresetInfo[] = [
  { id: 1, name: "Warm Glow", format: "xmp", group: "cloud", category: "portrait", color: "#e8a87c", favorite: true, params: { ...DEFAULT_PRESET_PARAMS, temperature: 6200, exposure: 0.3, saturation: 10 } },
  { id: 2, name: "Ocean Mist", format: "cube", group: "cloud", category: "landscape", color: "#85cdca", favorite: false, params: { ...DEFAULT_PRESET_PARAMS, temperature: 4800, contrast: 15, dehaze: 20 } },
  { id: 3, name: "Rose Dream", format: "np3", group: "cloud", category: "portrait", color: "#d9a7c7", favorite: false, params: { ...DEFAULT_PRESET_PARAMS, temperature: 5800, highlights: -20, saturation: 15, clarity: -10 } },
  { id: 4, name: "Lavender", format: "xmp", group: "cloud", category: "creative", color: "#c7b8ea", favorite: true, params: { ...DEFAULT_PRESET_PARAMS, tint: 15, vibrance: 20, highlights: -10 } },
  { id: 5, name: "Nikon Vivid", format: "np3", group: "imported", category: "landscape", color: "#f6c6ea", favorite: false, params: { ...DEFAULT_PRESET_PARAMS, contrast: 25, saturation: 30, sharpening: 20 } },
  { id: 6, name: "Nikon Neutral", format: "ncp", group: "imported", category: "portrait", color: "#a8d8ea", favorite: false, params: { ...DEFAULT_PRESET_PARAMS, contrast: -5, saturation: -5, sharpening: 10 } },
  { id: 7, name: "Portrait Soft", format: "lrpreset", group: "imported", category: "portrait", color: "#fcbad3", favorite: false, params: { ...DEFAULT_PRESET_PARAMS, exposure: 0.2, highlights: -30, clarity: -15, temperature: 5800 } },
  { id: 8, name: "Landscape Deep", format: "lrpreset", group: "imported", category: "landscape", color: "#ffffd2", favorite: false, params: { ...DEFAULT_PRESET_PARAMS, contrast: 20, shadows: 25, vibrance: 25, dehaze: 15 } },
  { id: 9, name: "Mono Contrast", format: "xmp", group: "imported", category: "mono", color: "#a8e6cf", favorite: false, params: { ...DEFAULT_PRESET_PARAMS, saturation: -100, contrast: 40, clarity: 20 } },
  { id: 10, name: "Sunset Gold", format: "cube", group: "cloud", category: "vintage", color: "#ffd3b6", favorite: false, params: { ...DEFAULT_PRESET_PARAMS, temperature: 7000, exposure: 0.1, vibrance: 15, highlights: -15 } },
  { id: 11, name: "Cherry Pop", format: "np3", group: "imported", category: "creative", color: "#ffaaa5", favorite: false, params: { ...DEFAULT_PRESET_PARAMS, tint: 20, saturation: 25, contrast: 10 } },
  { id: 12, name: "Cinema Teal", format: "cube", group: "cloud", category: "film", color: "#ff8b94", favorite: false, params: { ...DEFAULT_PRESET_PARAMS, temperature: 4500, tint: -10, contrast: 20, shadows: -15, highlights: -20 } },
];

interface PresetState {
  presets: PresetInfo[];
  group: PresetGroup;
  format: PresetFormat;
  category: PresetCategory;
  selectedId: number | null;
  appliedId: number | null;
  renamingId: number | null;
  setGroup: (g: PresetGroup) => void;
  setFormat: (f: PresetFormat) => void;
  setCategory: (c: PresetCategory) => void;
  select: (id: number | null) => void;
  apply: (id: number | null) => void;
  toggleFavorite: (id: number) => void;
  assignCategory: (id: number, cat: PresetCategory) => void;
  rename: (id: number, newName: string) => void;
  startRename: (id: number | null) => void;
  removePreset: (id: number) => void;
  importPresetFiles: () => Promise<number>;
  /** Get params for a preset by id */
  getParams: (id: number) => PresetParams | null;
}

export const usePresetStore = create<PresetState>((set, get) => ({
  presets: BUILTIN_PRESETS,
  group: "all",
  format: "all",
  category: "all",
  selectedId: null,
  appliedId: null,
  renamingId: null,

  setGroup: (group) => set({ group }),
  setFormat: (format) => set({ format }),
  setCategory: (category) => set({ category }),
  select: (selectedId) => set({ selectedId }),
  apply: (appliedId) => set({ appliedId }),
  startRename: (renamingId) => set({ renamingId }),

  toggleFavorite: (id) =>
    set((s) => ({
      presets: s.presets.map((p) =>
        p.id === id ? { ...p, favorite: !p.favorite } : p
      ),
    })),

  assignCategory: (id, cat) =>
    set((s) => ({
      presets: s.presets.map((p) =>
        p.id === id ? { ...p, category: cat } : p
      ),
    })),

  rename: (id, newName) =>
    set((s) => ({
      presets: s.presets.map((p) =>
        p.id === id ? { ...p, name: newName.trim() || p.name } : p
      ),
      renamingId: null,
    })),

  removePreset: (id) =>
    set((s) => ({
      presets: s.presets.filter((p) => p.id !== id),
      selectedId: s.selectedId === id ? null : s.selectedId,
      appliedId: s.appliedId === id ? null : s.appliedId,
    })),

  importPresetFiles: async () => {
    const picked = await open({
      multiple: true,
      filters: [
        { name: "Preset Files", extensions: ["xmp", "cube", "np3", "ncp", "lrpreset", "3dl", "costyle"] },
        { name: "All Files", extensions: ["*"] },
      ],
    });
    if (!picked) return 0;
    const paths = Array.isArray(picked) ? picked : [picked];
    let imported = 0;

    for (const filePath of paths) {
      if (typeof filePath !== "string") continue;

      const ext = filePath.split(".").pop()?.toLowerCase() ?? "";
      const fmtMap: Record<string, PresetFormat> = {
        xmp: "xmp", cube: "cube", np3: "np3", ncp: "ncp",
        lrpreset: "lrpreset", "3dl": "3dl", costyle: "costyle",
      };
      const fallbackName =
        filePath.split(/[\\/]/).pop()?.replace(/\.[^.]+$/, "") || "Preset";

      let name = fallbackName;
      let params: PresetParams = { ...DEFAULT_PRESET_PARAMS };

      try {
        // Only count a file once the backend has accepted its format.
        const result = await invoke<{
          name: string;
          format: string;
          params: {
            exposure: number | null;
            contrast: number | null;
            highlights: number | null;
            shadows: number | null;
            whites: number | null;
            blacks: number | null;
            temperature: number | null;
            tint: number | null;
            vibrance: number | null;
            saturation: number | null;
            clarity: number | null;
            sharpening: number | null;
          };
        }>("import_preset", { path: filePath });

        if (result?.name) name = result.name;
        params = {
          exposure: result.params.exposure ?? 0,
          contrast: result.params.contrast ?? 0,
          highlights: result.params.highlights ?? 0,
          shadows: result.params.shadows ?? 0,
          whites: result.params.whites ?? 0,
          blacks: result.params.blacks ?? 0,
          temperature: result.params.temperature ?? 5500,
          tint: result.params.tint ?? 0,
          vibrance: result.params.vibrance ?? 0,
          saturation: result.params.saturation ?? 0,
          clarity: result.params.clarity ?? 0,
          dehaze: 0,
          sharpening: result.params.sharpening ?? 0,
        };
      } catch (err) {
        toast(`无法导入 ${filePath.split(/[\\/]/).pop()}：${String(err)}`, "error");
        continue;
      }

      const newPreset: PresetInfo = {
        id: nextId++,
        name,
        format: fmtMap[ext] ?? "xmp",
        group: "imported",
        category: "all" as PresetCategory,
        color: PRESET_COLORS[imported % PRESET_COLORS.length],
        favorite: false,
        params,
        sourcePath: filePath,
      };

      set((s) => ({ presets: [...s.presets, newPreset] }));
      imported++;
    }
    return imported;
  },

  getParams: (id) => {
    const p = get().presets.find((pr) => pr.id === id);
    return p ? p.params : null;
  },
}));
