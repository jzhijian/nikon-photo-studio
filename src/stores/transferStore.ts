import { create } from "zustand";

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
}

interface TransferState {
  connectionType: ConnectionType;
  quality: Quality;
  connected: boolean;
  connecting: boolean;
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
  setCameraInfo: (model: string, detail: string, photos: CameraPhoto[]) => void;
  setConnectionError: (message: string) => void;
  setWifiHost: (host: string) => void;
  setSaveDir: (path: string) => void;
  toggleSelect: (handle: number) => void;
  selectAll: (handles: number[]) => void;
  deselectAll: () => void;
}

export const useTransferStore = create<TransferState>((set) => ({
  connectionType: "wifi-ap",
  quality: "original",
  connected: false,
  connecting: false,
  cameraModel: "",
  connectionDetail: "",
  cameraPhotos: [],
  connectionError: "",
  wifiHost: "192.168.1.1",
  saveDir: "",
  selectedHandles: [],
  setConnectionType: (connectionType) => set({ connectionType }),
  setQuality: (quality) => set({ quality }),
  setConnected: (connected) => set({ connected, connecting: false, ...(connected ? {} : { cameraPhotos: [], selectedHandles: [] }) }),
  setConnecting: (connecting) => set({ connecting }),
  setCameraInfo: (cameraModel, connectionDetail, cameraPhotos) => set({ cameraModel, connectionDetail, cameraPhotos, connectionError: "" }),
  setConnectionError: (connectionError) => set({ connectionError, connecting: false }),
  setWifiHost: (wifiHost) => set({ wifiHost }),
  setSaveDir: (saveDir) => set({ saveDir }),
  toggleSelect: (handle) =>
    set((s) => ({
      selectedHandles: s.selectedHandles.includes(handle)
        ? s.selectedHandles.filter((h) => h !== handle)
        : [...s.selectedHandles, handle],
    })),
  selectAll: (handles) => set({ selectedHandles: handles }),
  deselectAll: () => set({ selectedHandles: [] }),
}));
