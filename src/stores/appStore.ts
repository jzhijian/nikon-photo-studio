import { create } from "zustand";

export type ModuleId = "library" | "organize" | "presets" | "transfer";
export type Theme = "light" | "dark";

interface AppState {
  activeModule: ModuleId;
  language: "zh-CN" | "en-US";
  theme: Theme;
  setModule: (m: ModuleId) => void;
  setLanguage: (lang: "zh-CN" | "en-US") => void;
  setTheme: (t: Theme) => void;
  toggleTheme: () => void;
  leftWidth: number;
  leftCollapsed: boolean;
  setLeftWidth: (w: number) => void;
  toggleLeftCollapsed: () => void;
  exportOpen: boolean;
  openExport: () => void;
  closeExport: () => void;
}

function applyTheme(theme: Theme) {
  document.documentElement.setAttribute("data-theme", theme);
}

// Initialize theme from system preference
const prefersDark = window.matchMedia?.("(prefers-color-scheme: dark)").matches ?? true;
const initialTheme: Theme = prefersDark ? "dark" : "light";
applyTheme(initialTheme);

export const useAppStore = create<AppState>((set, get) => ({
  activeModule: "library",
  language: navigator.language.startsWith("zh") ? "zh-CN" : "en-US",
  theme: initialTheme,
  setModule: (m) => set({ activeModule: m }),
  setLanguage: (lang) => set({ language: lang }),
  setTheme: (t) => {
    applyTheme(t);
    set({ theme: t });
  },
  toggleTheme: () => {
    const next = get().theme === "dark" ? "light" : "dark";
    applyTheme(next);
    set({ theme: next });
  },
  leftWidth: 240,
  leftCollapsed: false,
  setLeftWidth: (leftWidth) => set({ leftWidth: Math.min(420, Math.max(150, leftWidth)), leftCollapsed: false }),
  toggleLeftCollapsed: () => set((s) => ({ leftCollapsed: !s.leftCollapsed })),
  exportOpen: false,
  openExport: () => set({ exportOpen: true }),
  closeExport: () => set({ exportOpen: false }),
}));
