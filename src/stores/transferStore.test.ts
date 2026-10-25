import { beforeEach, describe, expect, it } from "vitest";

import { runCameraTransition, useTransferStore, type CameraPhoto } from "./transferStore";

const photo = (handle: number): CameraPhoto => ({
  handle,
  filename: `${handle}.NEF`,
  format: "NEF",
  size: 1024,
  width: 5568,
  height: 3712,
  protected: false,
  can_delete: true,
});

describe("transfer session state", () => {
  beforeEach(() => {
    useTransferStore.setState({
      connectionType: "usb",
      connected: true,
      connecting: false,
      cameraModel: "Z 30",
      connectionDetail: "USB · Windows MTP/PTP",
      cameraPhotos: [photo(1), photo(2), photo(3)],
      connectionError: "old error",
      selectedHandles: [1, 2],
    });
  });

  it("clears the previous camera session when switching USB to Wi-Fi", () => {
    useTransferStore.getState().setSwitchingConnection(true);
    useTransferStore.getState().setConnectionType("wifi-ap");

    const state = useTransferStore.getState();
    expect(state.connectionType).toBe("wifi-ap");
    expect(state.connected).toBe(false);
    expect(state.cameraModel).toBe("");
    expect(state.cameraPhotos).toEqual([]);
    expect(state.selectedHandles).toEqual([]);
    expect(state.connectionError).toBe("");
    expect(state.switchingConnection).toBe(true);

    useTransferStore.getState().setSwitchingConnection(false);
    expect(useTransferStore.getState().switchingConnection).toBe(false);
  });

  it("removes only successfully deleted camera objects", () => {
    useTransferStore.getState().removePhotos([2]);

    const state = useTransferStore.getState();
    expect(state.cameraPhotos.map((item) => item.handle)).toEqual([1, 3]);
    expect(state.selectedHandles).toEqual([1]);
  });

  it("does not end an active connection when its previous error is cleared", () => {
    useTransferStore.getState().setConnecting(true);
    useTransferStore.getState().setConnectionError("");

    expect(useTransferStore.getState().connecting).toBe(true);
    expect(useTransferStore.getState().connectionError).toBe("");

    useTransferStore.getState().setConnectionError("camera unavailable");
    expect(useTransferStore.getState().connecting).toBe(true);
    expect(useTransferStore.getState().connectionError).toBe("camera unavailable");
  });
});

describe("camera transition queue", () => {
  it("does not let a newer native transition overtake an old connect", async () => {
    const order: string[] = [];
    let releaseOld!: () => void;
    const oldGate = new Promise<void>((resolve) => { releaseOld = resolve; });

    const oldConnect = runCameraTransition(async () => {
      order.push("old-start");
      await oldGate;
      order.push("old-end");
    });
    const modeCleanup = runCameraTransition(async () => {
      order.push("cleanup");
    });
    const newConnect = runCameraTransition(async () => {
      order.push("new-connect");
    });

    await Promise.resolve();
    expect(order).toEqual(["old-start"]);
    releaseOld();
    await Promise.all([oldConnect, modeCleanup, newConnect]);
    expect(order).toEqual(["old-start", "old-end", "cleanup", "new-connect"]);
  });

  it("continues after a rejected transition instead of leaving the queue stuck", async () => {
    await expect(runCameraTransition(async () => {
      throw new Error("disconnect failed");
    })).rejects.toThrow("disconnect failed");

    await expect(runCameraTransition(async () => "ready")).resolves.toBe("ready");
  });
});
