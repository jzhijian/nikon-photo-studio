import { invoke } from "@tauri-apps/api/core";
import i18n from "../i18n";
import { useAppStore } from "../stores/appStore";
import { useLibraryStore, type Photo } from "../stores/libraryStore";
import { useOrganizeStore } from "../stores/organizeStore";
import { usePresetStore } from "../stores/presetStore";
import { useTransferStore } from "../stores/transferStore";

const STATE_KEY = "workspace-v2";
let hydrated = false;
let timer = 0;

function snapshot() {
  const library = useLibraryStore.getState();
  const presets = usePresetStore.getState();
  const organize = useOrganizeStore.getState();
  const app = useAppStore.getState();
  const transfer = useTransferStore.getState();
  return {
    version: 2,
    library: {
      photos: library.photos, folder: library.folder, filter: library.filter,
      ratingOperator: library.ratingOperator, ratingValue: library.ratingValue,
      selectedPath: library.selectedPath, developPath: library.developPath,
      thumbSize: library.thumbSize, adjust: library.adjust,
    },
    presets: {
      presets: presets.presets, group: presets.group, format: presets.format,
      category: presets.category, selectedId: presets.selectedId, appliedId: presets.appliedId,
    },
    organize: {
      threshold: organize.threshold, portraitThreshold: organize.portraitThreshold,
      category: organize.category, sensitivity: organize.sensitivity, useGpu: organize.useGpu,
    },
    app: {
      activeModule: app.activeModule, language: app.language, theme: app.theme,
      leftWidth: app.leftWidth, leftCollapsed: app.leftCollapsed,
    },
    transfer: {
      connectionType: transfer.connectionType, quality: transfer.quality,
      wifiHost: transfer.wifiHost, saveDir: transfer.saveDir,
    },
  };
}

async function persistNow() {
  if (!hydrated) return;
  await invoke("save_app_state", { key: STATE_KEY, valueJson: JSON.stringify(snapshot()) }).catch(console.error);
}

function schedulePersist() {
  if (!hydrated) return;
  window.clearTimeout(timer);
  timer = window.setTimeout(() => void persistNow(), 500);
}

export async function restoreWorkspace(): Promise<() => void> {
  try {
    const raw = await invoke<string | null>("load_app_state", { key: STATE_KEY });
    if (raw) {
      const saved = JSON.parse(raw);
      const savedPhotos = (saved.library?.photos ?? []) as Photo[];
      const existing = await invoke<string[]>("existing_paths", { paths: savedPhotos.map((p) => p.filePath) });
      const existingSet = new Set(existing.map((p) => p.replaceAll("/", "\\").toLocaleLowerCase()));
      const photos = savedPhotos.filter((p) => existingSet.has(p.filePath.replaceAll("/", "\\").toLocaleLowerCase()));
      useLibraryStore.setState({ ...saved.library, photos, importing: false, detecting: false });
      usePresetStore.setState(saved.presets ?? {});
      useOrganizeStore.setState(saved.organize ?? {});
      useTransferStore.setState({ ...(saved.transfer ?? {}), connected: false, connecting: false, selectedHandles: [] });
      if (saved.app) {
        useAppStore.setState(saved.app);
        useAppStore.getState().setTheme(saved.app.theme);
        void i18n.changeLanguage(saved.app.language);
      }
    }
  } catch (error) {
    console.error("Workspace restore failed", error);
  }
  hydrated = true;
  const unsubscribers = [
    useLibraryStore.subscribe(schedulePersist), usePresetStore.subscribe(schedulePersist),
    useOrganizeStore.subscribe(schedulePersist), useAppStore.subscribe(schedulePersist),
    useTransferStore.subscribe(schedulePersist),
  ];
  return () => { unsubscribers.forEach((fn) => fn()); window.clearTimeout(timer); void persistNow(); };
}
