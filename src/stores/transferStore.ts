import { create } from "zustand";

// Camera connect/disconnect commands mutate one shared native session. Keep
// those transitions ordered even when a user changes transport while a prior
// (non-cancellable) Tauri invoke is still resolving. The UI state is still
// updated immediately by the callers; only the native lifecycle is queued.
let cameraTransitionTail: Promise<void> = Promise.resolve();

export function runCameraTransition<T>(operation: () => Promise<T>): Promise<T> {
  const result = cameraTransitionTail.then(operation, operation);
  cameraTransitionTail = result.then(() => undefined, () => undefined);
  return result;
}

export type ConnectionType = "wifi-ap" | "wifi-sta" | "usb";
export type Quality = "original" | "8mp" | "2mp";

export interface CameraPhoto {
  handle: number;
  filename: string;
  format: string | Record<string, string>;
  size: number;
  width: number;
  height: number;
  capture_date?: string | null;
  date?: string | null;
  protected?: boolean;
  can_delete?: boolean;
}

interface TransferState {
  connectionType: ConnectionType;
  quality: Quality;
  connected: boolean;
  connecting: boolean;
  switchingConnection: boolean;
  cameraModel: string;
  connectionDetail: string;
  cameraPhotos: CameraPhoto[];
  connectionError: string;
  wifiHost: string;
  saveDir: string;
  selectedHandles: number[];
  setConnectionType: (t: ConnectionType) => void;
  setQuality: (q: Quality) => void;
  setConnected: (v: boolean) => void;
  setConnecting: (v: boolean) => void;
  setSwitchingConnection: (v: boolean) => void;
  setCameraInfo: (model: string, detail: string, photos: CameraPhoto[]) => void;
  setConnectionError: (message: string) => void;
  setWifiHost: (host: string) => void;
  setSaveDir: (path: string) => void;
  toggleSelect: (handle: number) => void;
  selectAll: (handles: number[]) => void;
  setSelectedHandles: (handles: number[]) => void;
  deselectAll: () => void;
  removePhotos: (handles: number[]) => void;
}

export const useTransferStore = create<TransferState>((set) => ({
  connectionType: "wifi-ap",
  quality: "original",
  connected: false,
  connecting: false,
  switchingConnection: false,
  cameraModel: "",
  connectionDetail: "",
  cameraPhotos: [],
  connectionError: "",
  wifiHost: "192.168.1.1",
  saveDir: "",
  selectedHandles: [],
  setConnectionType: (connectionType) => set((state) => state.connectionType === connectionType
    ? state
    : {
        connectionType,
        connected: false,
        connecting: false,
        cameraModel: "",
        connectionDetail: "",
        cameraPhotos: [],
        connectionError: "",
        selectedHandles: [],
      }),
  setQuality: (quality) => set({ quality }),
  setConnected: (connected) => set({ connected, connecting: false, ...(connected ? {} : { cameraPhotos: [], selectedHandles: [] }) }),
  setConnecting: (connecting) => set({ connecting }),
  setSwitchingConnection: (switchingConnection) => set({ switchingConnection }),
  setCameraInfo: (cameraModel, connectionDetail, cameraPhotos) => set({ cameraModel, connectionDetail, cameraPhotos, connectionError: "", selectedHandles: [] }),
  // Error text and connection lifecycle are deliberately independent.  A
  // connect attempt clears the previous error immediately before it starts;
  // coupling that clear to `connecting: false` made the UI hide its progress
  // panel while the native command was still running, and every later click
  // was then rejected by the still-active attempt guard.
  setConnectionError: (connectionError) => set({ connectionError }),
  setWifiHost: (wifiHost) => set({ wifiHost }),
  setSaveDir: (saveDir) => set({ saveDir }),
  toggleSelect: (handle) =>
    set((s) => ({
      selectedHandles: s.selectedHandles.includes(handle)
        ? s.selectedHandles.filter((h) => h !== handle)
        : [...s.selectedHandles, handle],
    })),
  selectAll: (handles) => set({ selectedHandles: handles }),
  setSelectedHandles: (selectedHandles) => set({ selectedHandles }),
  deselectAll: () => set({ selectedHandles: [] }),
  removePhotos: (handles) => set((state) => {
    const removed = new Set(handles);
    return {
      cameraPhotos: state.cameraPhotos.filter((photo) => !removed.has(photo.handle)),
      selectedHandles: state.selectedHandles.filter((handle) => !removed.has(handle)),
    };
  }),
}));
