import { create } from "zustand";

export type OrgCategory = "blurry" | "portrait" | "other";
export type BlurSensitivity = "strict" | "standard" | "lenient";

interface OrganizeState {
  /** Continuous blur threshold (0-100). Photos scoring below this are "blurry". */
  threshold: number;
  /** UltraFace confidence threshold (0-100) used to mark a portrait. */
  portraitThreshold: number;
  category: OrgCategory;
  /** Detection sensitivity preset (maps to score thresholds in Rust). */
  sensitivity: BlurSensitivity;
  /**
   * Enable the accelerated detection path. Labelled "GPU 加速" in the UI: it
   * switches blur analysis to a smaller, much faster buffer (a hardware-GPU
   * backend can later plug in behind this same flag).
   */
  useGpu: boolean;
  setThreshold: (t: number) => void;
  setPortraitThreshold: (t: number) => void;
  setCategory: (c: OrgCategory) => void;
  setSensitivity: (s: BlurSensitivity) => void;
  setUseGpu: (g: boolean) => void;
}

export const useOrganizeStore = create<OrganizeState>((set) => ({
  threshold: 55,
  portraitThreshold: 62,
  category: "blurry",
  sensitivity: "standard",
  useGpu: false,
  setThreshold: (threshold) => set({ threshold }),
  setPortraitThreshold: (portraitThreshold) => set({ portraitThreshold }),
  setCategory: (category) => set({ category }),
  setSensitivity: (sensitivity) => set({ sensitivity }),
  setUseGpu: (useGpu) => set({ useGpu }),
}));
