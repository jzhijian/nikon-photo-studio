import { create } from "zustand";

export type ConnectionType = "wifi-ap" | "wifi-sta" | "usb";
export type Quality = "original" | "8mp" | "2mp";

interface TransferState {
  connectionType: ConnectionType;
  quality: Quality;
  connected: boolean;
  connecting: boolean;
  selectedHandles: number[];
  setConnectionType: (t: ConnectionType) => void;
  setQuality: (q: Quality) => void;
  setConnected: (v: boolean) => void;
  setConnecting: (v: boolean) => void;
  toggleSelect: (handle: number) => void;
  selectAll: (handles: number[]) => void;
  deselectAll: () => void;
}

export const useTransferStore = create<TransferState>((set) => ({
  connectionType: "wifi-ap",
  quality: "original",
  connected: false,
  connecting: false,
  selectedHandles: [],
  setConnectionType: (connectionType) => set({ connectionType }),
  setQuality: (quality) => set({ quality }),
  setConnected: (connected) => set({ connected, connecting: false }),
  setConnecting: (connecting) => set({ connecting }),
  toggleSelect: (handle) =>
    set((s) => ({
      selectedHandles: s.selectedHandles.includes(handle)
        ? s.selectedHandles.filter((h) => h !== handle)
        : [...s.selectedHandles, handle],
    })),
  selectAll: (handles) => set({ selectedHandles: handles }),
  deselectAll: () => set({ selectedHandles: [] }),
}));