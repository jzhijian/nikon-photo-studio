import { describe, expect, it } from "vitest";

import {
  adaptiveThumbnailRequestSize,
  CameraConnectionAttemptController,
  cameraPreviewPresentation,
  connectionStageFromNativePhase,
  connectionStageIndex,
  connectionStageProgress,
  connectionSteps,
  resolveCameraPreviewHandle,
  responsiveThumbnailMax,
  summarizeUsbConnectionError,
  waitForConnectionUiPaint,
} from "./transferUi";

describe("transfer connection progress", () => {
  it("keeps the three stages ordered and gives USB/Wi-Fi specific guidance", () => {
    expect(connectionSteps(false).map((step) => step.key)).toEqual(["detect", "session", "catalog"]);
    expect(connectionSteps(false)[1].description).toContain("PTP/IP");
    expect(connectionSteps(true)[0].description).toContain("MTP/PTP");
    expect(connectionStageIndex("detect")).toBeLessThan(connectionStageIndex("catalog"));
    expect(connectionStageProgress("session")).toBeGreaterThan(connectionStageProgress("detect"));
  });

  it("keeps USB enumeration and completion in the catalog stage", () => {
    expect(connectionStageFromNativePhase("usb-detect", "idle")).toBe("detect");
    expect(connectionStageFromNativePhase("usb-storage", "session")).toBe("catalog");
    expect(connectionStageFromNativePhase("usb-enumerated", "catalog")).toBe("catalog");
    expect(connectionStageFromNativePhase("complete", "catalog")).toBe("catalog");
    expect(connectionStageFromNativePhase("complete", "detect")).toBe("session");
  });

  it("keeps exactly one current attempt and invalidates stale transport results", () => {
    const controller = new CameraConnectionAttemptController<"wifi-ap" | "usb">();
    const wifi = controller.begin("wifi-ap");
    expect(wifi).not.toBeNull();
    expect(controller.isBusy()).toBe(true);
    expect(controller.begin("wifi-ap")).toBeNull();
    expect(controller.isCurrent(wifi!, "wifi-ap")).toBe(true);

    const cancelledVersion = controller.invalidate();
    expect(controller.isCurrent(wifi!, "wifi-ap")).toBe(false);
    expect(controller.isVersion(cancelledVersion)).toBe(true);

    const usb = controller.begin("usb");
    expect(usb).not.toBeNull();
    expect(controller.finish(wifi!)).toBe(false);
    expect(controller.isCurrent(usb!, "usb")).toBe(true);
    expect(controller.finish(usb!)).toBe(true);
    expect(controller.isBusy()).toBe(false);
  });

  it("waits through a real paint boundary before native connection work", async () => {
    const frames: FrameRequestCallback[] = [];
    const painted = waitForConnectionUiPaint((callback) => {
      frames.push(callback);
      return frames.length;
    });

    expect(frames).toHaveLength(1);
    frames.shift()!(0);
    expect(frames).toHaveLength(1);
    let resolved = false;
    void painted.then(() => { resolved = true; });
    await Promise.resolve();
    expect(resolved).toBe(false);
    frames.shift()!(16);
    await painted;
    expect(resolved).toBe(true);
  });
});

describe("camera detail progressive presentation", () => {
  it("keeps the thumbnail visible while the original downloads and the develop canvas renders", () => {
    expect(cameraPreviewPresentation({
      hasFirstFrame: true,
      sourcePrepared: false,
      developReady: false,
      loading: true,
      failed: false,
    })).toMatchObject({ phase: "transfer", showFirstFrame: true, showDevelopViewer: false });

    expect(cameraPreviewPresentation({
      hasFirstFrame: true,
      sourcePrepared: true,
      developReady: false,
      loading: false,
      failed: false,
    })).toMatchObject({ phase: "render", showFirstFrame: true, showDevelopViewer: true, showLoadingLayer: true });
  });

  it("removes the first frame only after DevelopViewer reports a painted canvas", () => {
    expect(cameraPreviewPresentation({
      hasFirstFrame: true,
      sourcePrepared: true,
      developReady: true,
      loading: false,
      failed: false,
    })).toEqual({ phase: "ready", showFirstFrame: false, showDevelopViewer: true, showLoadingLayer: false });
  });

  it("recovers from an invalid handle after the camera catalog changes", () => {
    expect(resolveCameraPreviewHandle([31, 42, 57], 42, 0)).toBe(42);
    expect(resolveCameraPreviewHandle([31, 57], 42, 8)).toBe(57);
    expect(resolveCameraPreviewHandle([], 42, 0)).toBeNull();
  });
});

describe("USB connection error presentation", () => {
  it("keeps the primary WPD cause and removes PowerShell script diagnostics", () => {
    const raw = [
      "USB scan failed: Windows portable-device error: Unable to enumerate Z 30\\可移动存储\\DCIM: 设备尚未准备好",
      "所在位置 行:71 字符: 9",
      "+       throw \"Unable to enumerate $context\"",
      "+       ~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~",
      "    + CategoryInfo          : OperationStopped: (Unable to enum...:String) [], RuntimeException",
      "    + FullyQualifiedErrorId : Unable to enumerate Z 30",
    ].join("\r\n");

    const message = summarizeUsbConnectionError(raw);
    expect(message).toBe("Unable to enumerate Z 30\\可移动存储\\DCIM: 设备尚未准备好");
    expect(message).not.toMatch(/CategoryInfo|RuntimeException|所在位置|行:71/);
  });

  it("recognizes common device states and does not repeat nested prefixes", () => {
    expect(summarizeUsbConnectionError(
      "USB camera scan worker failed: USB scan failed: Windows portable-device error: No Nikon camera found. Set the camera USB mode to MTP/PTP.",
    )).toBe("Windows 未检测到可用的 Nikon USB 相机");

    expect(summarizeUsbConnectionError(
      "USB connect failed: Windows portable-device error: MTP device is visible but its storage tree is not ready: Z 30 At line: 64 char: 3 + throw + CategoryInfo : OperationStopped: RuntimeException",
    )).toBe("Windows 已识别 Z 30，但相机存储目录尚未准备好");
  });

  it("uses a concise fallback when PowerShell returned diagnostics only", () => {
    expect(summarizeUsbConnectionError(
      "At line:12 char:4\n+ throw\n+ CategoryInfo : OperationStopped: (:) [], RuntimeException\n+ FullyQualifiedErrorId : failure",
    )).toBe("Windows 无法读取相机的 MTP/PTP 存储");
  });
});

describe("responsive transfer thumbnails", () => {
  it("uses the photo viewport width for a one-column maximum", () => {
    expect(responsiveThumbnailMax(1280)).toBe(1248);
    expect(responsiveThumbnailMax(300)).toBe(320);
    expect(responsiveThumbnailMax(0)).toBe(320);
  });

  it("requests stable DPR-aware sizes without exceeding the preview ceiling", () => {
    expect(adaptiveThumbnailRequestSize(190, 1)).toBe(640);
    expect(adaptiveThumbnailRequestSize(500, 2)).toBe(1280);
    expect(adaptiveThumbnailRequestSize(1200, 2)).toBe(2048);
    expect(adaptiveThumbnailRequestSize(4000, 3)).toBe(2048);
  });
});
