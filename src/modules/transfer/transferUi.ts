export type CameraConnectionStage = "idle" | "detect" | "session" | "catalog";

export interface CameraConnectionAttempt<T extends string = string> {
  id: number;
  connectionType: T;
}

/**
 * Owns the lifecycle of one camera connection attempt.  React state is used
 * for presentation, while this synchronous controller rejects double-clicks
 * in the same event turn and prevents a late USB/Wi-Fi result from publishing
 * into a newer transport.
 */
export class CameraConnectionAttemptController<T extends string = string> {
  private version = 0;
  private active: CameraConnectionAttempt<T> | null = null;

  begin(connectionType: T): CameraConnectionAttempt<T> | null {
    if (this.active) return null;
    const attempt = { id: ++this.version, connectionType };
    this.active = attempt;
    return attempt;
  }

  isBusy(): boolean {
    return this.active !== null;
  }

  isCurrent(attempt: CameraConnectionAttempt<T>, connectionType: T): boolean {
    return this.active?.id === attempt.id
      && this.active.connectionType === attempt.connectionType
      && attempt.connectionType === connectionType;
  }

  finish(attempt: CameraConnectionAttempt<T>): boolean {
    if (!this.active || this.active.id !== attempt.id) return false;
    this.active = null;
    return true;
  }

  invalidate(): number {
    this.active = null;
    return ++this.version;
  }

  isVersion(version: number): boolean {
    return this.version === version;
  }
}

type FrameScheduler = (callback: FrameRequestCallback) => number;

/**
 * Let the connection spinner/progress panel commit and reach the screen before
 * invoking a native MTP/PTP command. Two frames are intentional: the first
 * lets React commit and the second starts only after one browser paint.
 */
export function waitForConnectionUiPaint(schedule?: FrameScheduler): Promise<void> {
  const requestFrame = schedule
    ?? (typeof globalThis.requestAnimationFrame === "function"
      ? globalThis.requestAnimationFrame.bind(globalThis)
      : undefined);
  if (!requestFrame) return new Promise((resolve) => setTimeout(resolve, 0));
  return new Promise((resolve) => {
    requestFrame(() => requestFrame(() => resolve()));
  });
}

export interface CameraConnectionStep {
  key: Exclude<CameraConnectionStage, "idle">;
  label: string;
  description: string;
}

const WIFI_STEPS: CameraConnectionStep[] = [
  {
    key: "detect",
    label: "检测相机网络",
    description: "确认当前 Wi-Fi、电脑地址与 Nikon 相机地址。",
  },
  {
    key: "session",
    label: "建立相机会话",
    description: "正在建立 PTP/IP 命令与事件通道，请让相机停留在等待连接画面。",
  },
  {
    key: "catalog",
    label: "读取照片清单",
    description: "正在枚举存储卡中的照片、尺寸及保护状态。",
  },
];

const USB_STEPS: CameraConnectionStep[] = [
  {
    key: "detect",
    label: "查找 USB 相机",
    description: "正在通过 Windows MTP/PTP 查找已开机并解锁的相机。",
  },
  {
    key: "session",
    label: "打开设备存储",
    description: "正在打开相机的便携设备存储与对象列表。",
  },
  {
    key: "catalog",
    label: "读取照片清单",
    description: "正在枚举存储卡中的照片、尺寸及保护状态。",
  },
];

export function connectionSteps(usb: boolean): CameraConnectionStep[] {
  return usb ? USB_STEPS : WIFI_STEPS;
}

export function connectionStageProgress(stage: CameraConnectionStage): number {
  if (stage === "detect") return 18;
  if (stage === "session") return 55;
  if (stage === "catalog") return 86;
  return 0;
}

export function connectionStageIndex(stage: CameraConnectionStage): number {
  if (stage === "detect") return 0;
  if (stage === "session") return 1;
  if (stage === "catalog") return 2;
  return -1;
}

/** Map native PTP/MTP progress without allowing a late completion event to
 * move an already-enumerating catalog backwards to session/detection. */
export function connectionStageFromNativePhase(
  phase: string,
  current: CameraConnectionStage,
): CameraConnectionStage {
  const normalized = phase.toLowerCase();
  if (["scan", "catalog", "storage", "list", "enumerat"].some((token) => normalized.includes(token))) {
    return "catalog";
  }
  if (normalized.includes("complete") && current === "catalog") return "catalog";
  if (["session", "ptp", "handshake", "connect", "transport", "device", "complete"].some((token) => normalized.includes(token))) {
    return "session";
  }
  return "detect";
}

const POWERSHELL_DIAGNOSTIC_LINE = /^(?:at\s+line\s*:|所在位置\s+行\s*[:：]|位于\s+行\s*[:：]|categoryinfo\s*[:：]|fullyqualifiederrorid\s*[:：]|scriptstacktrace\s*[:：]|invocationinfo\s*[:：]|\+|~+|\^+)/i;
const POWERSHELL_INLINE_DIAGNOSTIC = /\s+(?:at\s+line\s*:\s*\d+|所在位置\s+行\s*[:：]\s*\d+|位于\s+行\s*[:：]\s*\d+|\+?\s*categoryinfo\s*[:：]|\+?\s*fullyqualifiederrorid\s*[:：]|scriptstacktrace\s*[:：]|invocationinfo\s*[:：])/i;
const USB_ERROR_PREFIX = /^(?:(?:error|connection failed)\s*:\s*|usb camera scan worker failed\s*:\s*|usb (?:device detection|scan|connect) failed\s*:\s*|windows portable-device error\s*:\s*)/i;

/**
 * PowerShell appends script source, line/character positions and exception
 * metadata to WPD failures. Those blocks are useful in a terminal but make a
 * desktop error unreadable (and repeat the same message in CategoryInfo).
 * Return one stable user-facing cause; the view adds the recovery advice.
 */
export function summarizeUsbConnectionError(value: unknown): string {
  const raw = value instanceof Error ? value.message : String(value ?? "");
  const lines = raw
    .replace(/\u001b\[[0-9;]*m/g, "")
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter(Boolean);

  let summary = lines.find((line) => !POWERSHELL_DIAGNOSTIC_LINE.test(line)) || "";
  const diagnosticStart = summary.search(POWERSHELL_INLINE_DIAGNOSTIC);
  if (diagnosticStart >= 0) summary = summary.slice(0, diagnosticStart);

  // Rust/Tauri and the Windows bridge can each add one contextual prefix.
  // Peel all of them so the user sees the actual WPD/MTP cause only once.
  let previous = "";
  while (summary && summary !== previous) {
    previous = summary;
    summary = summary.replace(USB_ERROR_PREFIX, "").trim();
  }
  summary = summary
    .replace(/\s+\[\],?\s*RuntimeException\b.*$/i, "")
    .replace(/\s+RuntimeException\b.*$/i, "")
    .trim();

  if (/no nikon camera found/i.test(summary)) {
    return "Windows 未检测到可用的 Nikon USB 相机";
  }
  const notReady = summary.match(/MTP device is visible but its storage tree is not ready\s*:\s*(.+)$/i);
  if (notReady) {
    return `Windows 已识别 ${notReady[1].trim()}，但相机存储目录尚未准备好`;
  }
  const disappeared = summary.match(/MTP device disappeared before storage enumeration\s*:\s*(.+)$/i);
  if (disappeared) {
    return `扫描过程中 ${disappeared[1].trim()} 已从 Windows 便携设备中断开`;
  }

  if (!summary) return "Windows 无法读取相机的 MTP/PTP 存储";
  return summary.length > 280 ? `${summary.slice(0, 277).trimEnd()}…` : summary;
}

/**
 * The scroll container includes 16px horizontal padding on either side.  At
 * the maximum slider position the remaining content width becomes one photo
 * column, regardless of window or side-panel width.
 */
export function responsiveThumbnailMax(containerWidth: number): number {
  if (!Number.isFinite(containerWidth) || containerWidth <= 0) return 320;
  return Math.max(320, Math.floor(containerWidth - 32));
}

/**
 * Ask for enough source pixels for the rendered card and current display DPR,
 * while keeping requests in stable buckets so small slider movements do not
 * repeatedly reload every camera thumbnail.
 */
export function adaptiveThumbnailRequestSize(displaySize: number, devicePixelRatio = 1): number {
  const dpr = Math.min(2, Math.max(1, Number.isFinite(devicePixelRatio) ? devicePixelRatio : 1));
  const required = Math.ceil(Math.max(1, displaySize) * dpr * 1.12);
  const buckets = [640, 960, 1280, 1600, 2048] as const;
  return buckets.find((size) => size >= required) ?? buckets[buckets.length - 1];
}

export type CameraPreviewPhase = "first-frame" | "transfer" | "render" | "ready" | "error";

export interface CameraPreviewPresentation {
  phase: CameraPreviewPhase;
  showFirstFrame: boolean;
  showDevelopViewer: boolean;
  showLoadingLayer: boolean;
}

/**
 * Camera object lists can change underneath the detail overlay (disconnect,
 * delete, or a fresh catalog scan). Resolve by the stable PTP handle and fall
 * back to a clamped list position instead of dereferencing a stale index.
 */
export function resolveCameraPreviewHandle(
  handles: number[],
  activeHandle: number | null,
  preferredIndex = 0,
): number | null {
  if (!handles.length) return null;
  if (activeHandle !== null && handles.includes(activeHandle)) return activeHandle;
  const index = Math.min(handles.length - 1, Math.max(0, preferredIndex));
  return handles[index] ?? null;
}

/**
 * Keep the inexpensive camera thumbnail mounted underneath the adaptive
 * renderer until its canvas has actually painted. Preparing the local source
 * path is not enough: ProcessedImage still needs to decode and rasterize its
 * first frame, and swapping at that earlier point produces a black flash.
 */
export function cameraPreviewPresentation({
  hasFirstFrame,
  sourcePrepared,
  developReady,
  loading,
  failed,
}: {
  hasFirstFrame: boolean;
  sourcePrepared: boolean;
  developReady: boolean;
  loading: boolean;
  failed: boolean;
}): CameraPreviewPresentation {
  if (developReady) {
    return { phase: "ready", showFirstFrame: false, showDevelopViewer: true, showLoadingLayer: false };
  }
  if (sourcePrepared) {
    return { phase: "render", showFirstFrame: hasFirstFrame, showDevelopViewer: true, showLoadingLayer: true };
  }
  if (failed) {
    return { phase: "error", showFirstFrame: hasFirstFrame, showDevelopViewer: false, showLoadingLayer: false };
  }
  return {
    phase: loading && hasFirstFrame ? "transfer" : "first-frame",
    showFirstFrame: hasFirstFrame,
    showDevelopViewer: false,
    showLoadingLayer: loading,
  };
}
