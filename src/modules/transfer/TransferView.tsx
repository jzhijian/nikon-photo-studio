import { startTransition, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import { runCameraTransition, useTransferStore, type CameraPhoto, type ConnectionType } from "../../stores/transferStore";
import { toast } from "../../stores/toastStore";
import { useTranslation } from "react-i18next";
import ContextMenu, { type MenuItem } from "../../components/ContextMenu";
import ConfirmDialog from "../../components/ConfirmDialog";
import DevelopViewer from "../../components/DevelopViewer";
import { DEFAULT_ADJUST, type Photo } from "../../stores/libraryStore";
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
  type CameraConnectionStage,
} from "./transferUi";

interface ConnectResult { success: boolean; message: string; camera_model?: string | null }
interface CameraThumbnail { handle: number; url: string }
interface DeleteCameraResult { deleted: number[]; protected: string[]; failed: string[] }
interface CameraConnectionProgress {
  attempt: number;
  totalAttempts: number;
  phase: string;
  message: string;
}
interface CameraScanProgress { scanned: number; total: number; loaded: number; phase: string }
interface CameraDownloadProgress {
  handle: number;
  filename: string;
  bytesReceived: number;
  totalBytes: number;
  percent: number;
}
interface PreparedCameraPhoto {
  path: string;
  handle: number;
  filename: string;
  size: number;
  format: CameraPhoto["format"];
  width: number;
  height: number;
}
interface WifiNetworkInfo {
  ssid?: string | null;
  localIpv4?: string | null;
  gateway?: string | null;
  looksLikeNikonAp: boolean;
}

const humanSize = (bytes: number) => bytes >= 1024 ** 2
  ? `${(bytes / 1024 ** 2).toFixed(1)} MB`
  : `${Math.max(1, Math.round(bytes / 1024))} KB`;

const humanRate = (bytesPerSecond: number) => bytesPerSecond >= 1024 ** 2
  ? `${(bytesPerSecond / 1024 ** 2).toFixed(1)} MB/s`
  : `${Math.max(1, Math.round(bytesPerSecond / 1024))} KB/s`;

const formatName = (format: CameraPhoto["format"]) =>
  typeof format === "string" ? format : Object.values(format)[0] || "RAW";

const preparedToLibraryPhoto = (prepared: PreparedCameraPhoto): Photo => ({
  filePath: prepared.path,
  filename: prepared.filename,
  format: formatName(prepared.format),
  fileSize: prepared.size,
  width: prepared.width,
  height: prepared.height,
  previewable: true,
  folder: "相机临时预览",
  category: "other",
  classificationConfidence: 0,
  hasPerson: false,
  blurScore: 0,
  marked: false,
  rating: 0,
  selected: false,
});

function TransferPreview({ photos, initialIndex, thumbnails, onClose }: {
  photos: CameraPhoto[];
  initialIndex: number;
  thumbnails: Record<number, string>;
  onClose: () => void;
}) {
  const firstPhoto = photos[Math.min(Math.max(0, initialIndex), Math.max(0, photos.length - 1))];
  const [activeHandle, setActiveHandle] = useState<number | null>(firstPhoto?.handle ?? null);
  const [preparedState, setPreparedState] = useState<{ handle: number; photo: Photo } | null>(null);
  const [firstFrameState, setFirstFrameState] = useState<{ handle: number; url: string } | null>(() => firstPhoto && thumbnails[firstPhoto.handle]
    ? { handle: firstPhoto.handle, url: thumbnails[firstPhoto.handle] }
    : null);
  const [loadState, setLoadState] = useState<{
    handle: number | null;
    status: "loading" | "prepared" | "error";
    error: string;
    progress: CameraDownloadProgress | null;
  }>({ handle: firstPhoto?.handle ?? null, status: "loading", error: "", progress: null });
  const [readyHandle, setReadyHandle] = useState<number | null>(null);
  const resolvedHandle = resolveCameraPreviewHandle(photos.map((item) => item.handle), activeHandle, initialIndex);
  const index = resolvedHandle === null ? -1 : photos.findIndex((item) => item.handle === resolvedHandle);
  const photo = index >= 0 ? photos[index] : null;
  const suppliedThumbnail = photo ? thumbnails[photo.handle] || "" : "";
  const firstFrame = photo
    ? firstFrameState?.handle === photo.handle
      ? firstFrameState.url
      : suppliedThumbnail
    : "";
  const localPhoto = photo && preparedState?.handle === photo.handle ? preparedState.photo : null;
  const currentLoad = photo && loadState.handle === photo.handle ? loadState : null;
  const loading = !currentLoad || currentLoad.status === "loading";
  const loadError = currentLoad?.error || "";
  const previewProgress = currentLoad?.progress || null;
  const developReady = !!photo && readyHandle === photo.handle;
  const presentation = cameraPreviewPresentation({
    hasFirstFrame: !!firstFrame,
    sourcePrepared: !!localPhoto,
    developReady,
    loading,
    failed: !!loadError,
  });
  const thumbnailRef = useRef(thumbnails);
  thumbnailRef.current = thumbnails;

  useEffect(() => {
    if (!photos.length) {
      onClose();
      return;
    }
    if (resolvedHandle !== activeHandle) setActiveHandle(resolvedHandle);
  }, [photos.length, resolvedHandle, activeHandle, onClose]);

  useEffect(() => {
    if (!photo || !suppliedThumbnail) return;
    setFirstFrameState((current) => current?.handle === photo.handle && current.url
      ? current
      : { handle: photo.handle, url: suppliedThumbnail });
  }, [photo?.handle, suppliedThumbnail]);

  useEffect(() => {
    if (!photo) return;
    const handle = photo.handle;
    let active = true;
    let unlisten: (() => void) | undefined;
    setPreparedState(null);
    setReadyHandle(null);
    setFirstFrameState(suppliedThumbnail ? { handle, url: suppliedThumbnail } : null);
    setLoadState({ handle, status: "loading", error: "", progress: null });

    const listenerReady = listen<CameraDownloadProgress>("camera-preview-progress", ({ payload }) => {
      if (!active || payload.handle !== handle) return;
      setLoadState((current) => current.handle === handle ? { ...current, progress: payload } : current);
    }).then((dispose) => {
      if (active) unlisten = dispose;
      else dispose();
    }).catch(() => undefined);

    // Avoid starting a 20-30 MB transfer for every frame while the user is
    // rapidly stepping through the filmstrip. A settled selection starts
    // almost immediately; already cached files return without camera I/O.
    const timer = window.setTimeout(() => {
      void (async () => {
        await listenerReady;
        let haveFirstFrame = !!thumbnailRef.current[handle];
        if (!haveFirstFrame) {
          const rows = await invoke<CameraThumbnail[]>("get_camera_thumbnails", { handles: [handle], size: 1280 }).catch(() => []);
          const row = rows.find((item) => item.handle === handle);
          if (active && row?.url) {
            haveFirstFrame = true;
            setFirstFrameState({ handle, url: row.url });
          }
        }
        if (!active) return;
        try {
          const prepared = await invoke<PreparedCameraPhoto>("prepare_camera_photo", { handle });
          if (!active) return;
          startTransition(() => {
            setPreparedState({ handle, photo: preparedToLibraryPhoto(prepared) });
            setLoadState((current) => current.handle === handle ? { ...current, status: "prepared" } : current);
          });
        } catch (error) {
          if (!active) return;
          const message = `无法读取高清原片：${String(error)}`;
          setLoadState((current) => current.handle === handle ? { ...current, status: "error", error: message } : current);
          if (!haveFirstFrame) toast(message, "error");
        }
      })();
    }, 160);
    return () => {
      active = false;
      window.clearTimeout(timer);
      unlisten?.();
    };
  }, [photo?.handle]);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
      if (event.key === "ArrowLeft" && index > 0) setActiveHandle(photos[index - 1]?.handle ?? activeHandle);
      if (event.key === "ArrowRight" && index >= 0 && index < photos.length - 1) setActiveHandle(photos[index + 1]?.handle ?? activeHandle);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [activeHandle, index, photos, onClose]);

  if (!photo) return (
    <div className="fixed inset-0 z-[90] flex items-center justify-center bg-black/95 text-white" onClick={onClose}>
      <div className="rounded-lg border border-white/10 bg-white/5 p-6 text-center" onClick={(event) => event.stopPropagation()}>
        <div className="text-sm">照片清单已经更新，当前照片不再可用</div>
        <button onClick={onClose} className="mt-4 rounded bg-nps-accent px-4 py-2 text-xs font-semibold text-black">返回传输</button>
      </div>
    </div>
  );

  return (
    <div className="fixed inset-0 z-[90] flex flex-col bg-[#090909]" onClick={onClose}>
      <div className="flex h-14 shrink-0 items-center gap-3 px-5 text-white" onClick={(event) => event.stopPropagation()}>
        <button onClick={onClose} className="rounded border border-white/15 bg-white/10 px-3 py-1.5 text-xs hover:bg-white/20">← 返回传输</button>
        <span className="font-semibold">{photo.filename}</span>
        <span className="text-xs text-white/50">{formatName(photo.format)} · {photo.width && photo.height ? `${photo.width}×${photo.height} · ` : ""}{humanSize(photo.size)}</span>
        {localPhoto && <span className="rounded bg-nps-accent/15 px-2 py-0.5 text-[10px] text-nps-accent">{developReady ? "自适应高清 2048–8192px" : "原片已缓存 · 正在解析高清"}</span>}
        <span className="text-xs text-white/40">{index + 1} / {photos.length}</span>
        <button onClick={onClose} className="ml-auto grid h-8 w-8 place-items-center rounded-full bg-white/10 text-xl hover:bg-white/20">×</button>
      </div>
      <div className="relative flex flex-1 items-center justify-center overflow-hidden bg-[radial-gradient(circle_at_center,rgba(255,255,255,0.045),transparent_62%)]" onClick={(event) => event.stopPropagation()}>
        {firstFrame && <img src={firstFrame} alt={photo.filename} draggable={false} className={`absolute inset-0 h-full w-full select-none object-contain transition-opacity duration-200 ${presentation.showFirstFrame ? "opacity-95" : "opacity-0"}`} />}
        {!firstFrame && !localPhoto && <div className="absolute inset-6 grid place-items-center rounded-lg border border-white/[0.04] bg-white/[0.015]"><div className="flex flex-col items-center gap-3 text-center text-sm text-white/55"><span className="h-9 w-9 animate-spin rounded-full border-2 border-white/15 border-t-nps-accent" /><span>正在优先读取这张照片的内嵌预览</span><span className="text-[11px] text-white/35">预览可用后会立即显示，不需要等待原文件传输完成</span></div></div>}
        {presentation.showDevelopViewer && localPhoto && <div className={`absolute inset-0 transition-opacity duration-200 ${developReady || !firstFrame ? "opacity-100" : "pointer-events-none opacity-0"}`}>
          <DevelopViewer key={localPhoto.filePath} photo={localPhoto} adjust={DEFAULT_ADJUST} onReady={() => setReadyHandle(photo.handle)} />
        </div>}
        {presentation.showLoadingLayer && <div className="absolute right-5 top-4 w-80 rounded border border-white/10 bg-black/75 px-3 py-2 text-xs text-white/75 shadow-xl backdrop-blur">
          <div className="flex items-center gap-2"><span className="h-3 w-3 animate-spin rounded-full border border-white/20 border-t-nps-accent" /><span>{localPhoto ? "原片已就绪，正在绘制高清画面…" : firstFrame ? "缩略图已显示，正在读取完整原片…" : "正在准备首帧与完整原片…"}</span><span className="ml-auto text-nps-accent">{localPhoto ? "渲染中" : previewProgress ? `${previewProgress.percent.toFixed(0)}%` : "准备中"}</span></div>
          <div className="mt-2 h-1 overflow-hidden rounded bg-white/10"><div className="h-full rounded bg-nps-accent transition-[width]" style={{ width: `${previewProgress?.percent || 2}%` }} /></div>
          <div className="mt-1.5 text-[10px] leading-relaxed text-white/45">{previewProgress && !localPhoto ? `${humanSize(previewProgress.bytesReceived)} / ${humanSize(previewProgress.totalBytes)} · ` : ""}{localPhoto ? "缩略图会保留到 DevelopViewer 首帧真正绘制完成，因此不会切换成空白画布。" : "完成后会按窗口、DPI 与缩放倍率自动升级清晰度；同一原片不会重复传输。"}</div>
        </div>}
        {!!loadError && <div className="absolute right-5 top-4 max-w-sm rounded border border-nps-danger/40 bg-black/80 p-3 text-xs leading-relaxed text-nps-danger">{loadError}<div className="mt-1 text-[10px] text-white/45">仍可查看相机提供的内嵌缩略图；请确认相机未进入菜单、回放或休眠状态。</div></div>}
        {index > 0 && <button onClick={() => setActiveHandle(photos[index - 1]?.handle ?? activeHandle)} className="absolute left-4 top-1/2 grid h-12 w-12 -translate-y-1/2 place-items-center rounded-full bg-white/10 text-2xl text-white hover:bg-white/25">‹</button>}
        {index < photos.length - 1 && <button onClick={() => setActiveHandle(photos[index + 1]?.handle ?? activeHandle)} className="absolute right-4 top-1/2 grid h-12 w-12 -translate-y-1/2 place-items-center rounded-full bg-white/10 text-2xl text-white hover:bg-white/25">›</button>}
      </div>
      <div className="h-20 shrink-0 border-t border-white/10 bg-black/80 px-4 py-2" onClick={(event) => event.stopPropagation()}>
        <div className="mx-auto flex max-w-4xl gap-1 overflow-x-auto">
          {photos.slice(Math.max(0, index - 12), Math.min(photos.length, index + 13)).map((item) => {
            const itemIndex = photos.indexOf(item);
            return <button key={item.handle} onClick={() => setActiveHandle(item.handle)} title={item.filename} className={`h-10 w-14 shrink-0 overflow-hidden rounded border ${itemIndex === index ? "border-nps-accent" : "border-white/10"}`}>
              {thumbnails[item.handle] ? <img src={thumbnails[item.handle]} alt="" className="h-full w-full object-contain" /> : <span className="grid h-full place-items-center text-[9px] text-white/40">{formatName(item.format)}</span>}
            </button>;
          })}
        </div>
      </div>
    </div>
  );
}

export default function TransferView() {
  const { t } = useTranslation();
  const store = useTransferStore();
  const {
    connectionType, connected, connecting, switchingConnection, cameraModel, connectionDetail, cameraPhotos,
    connectionError, wifiHost, saveDir, selectedHandles, toggleSelect, selectAll,
    setSelectedHandles, deselectAll, removePhotos, setConnected, setConnecting, setCameraInfo, setConnectionError, setSaveDir,
  } = store;
  const [downloading, setDownloading] = useState(false);
  const [cancellingConnection, setCancellingConnection] = useState(false);
  const [downloadProgress, setDownloadProgress] = useState<{ completed: number; total: number; current: string } | null>(null);
  const [nativeDownloadProgress, setNativeDownloadProgress] = useState<CameraDownloadProgress | null>(null);
  const [downloadBytesPerSecond, setDownloadBytesPerSecond] = useState(0);
  const [deleting, setDeleting] = useState(false);
  const [connectionStage, setConnectionStage] = useState<CameraConnectionStage>("idle");
  const [nativeConnectionProgress, setNativeConnectionProgress] = useState<CameraConnectionProgress | null>(null);
  const [scanProgress, setScanProgress] = useState<CameraScanProgress | null>(null);
  const [thumbs, setThumbs] = useState<Record<number, string>>({});
  const [thumbSize, setThumbSize] = useState(190);
  const [thumbLimit, setThumbLimit] = useState(40);
  const [loadingThumbs, setLoadingThumbs] = useState(false);
  const [thumbStatus, setThumbStatus] = useState<Record<number, "loading" | "ready" | "unavailable">>({});
  const [thumbRevision, setThumbRevision] = useState(0);
  const [previewIndex, setPreviewIndex] = useState<number | null>(null);
  const [contextMenu, setContextMenu] = useState<{ x: number; y: number; index: number } | null>(null);
  const [confirmDelete, setConfirmDelete] = useState<number[] | null>(null);
  const [wifiInfo, setWifiInfo] = useState<WifiNetworkInfo | null>(null);
  const [gridViewport, setGridViewport] = useState({ width: 0, height: 0 });
  const gridViewportRef = useRef<HTMLDivElement>(null);
  const loadedThumbSizes = useRef(new Map<number, number>());
  const pendingThumbs = useRef(new Set<number>());
  const thumbnailSession = useRef(0);
  const autoConnectTriggered = useRef(false);
  const lastSelectedIndex = useRef<number | null>(null);
  const connectionAttempts = useRef(new CameraConnectionAttemptController<ConnectionType>());
  const downloadRateRef = useRef<{ handle: number; bytes: number; at: number } | null>(null);

  const selectedSet = useMemo(() => new Set(selectedHandles), [selectedHandles]);
  const visiblePhotos = useMemo(() => cameraPhotos.slice(0, thumbLimit), [cameraPhotos, thumbLimit]);
  const maxThumbSize = useMemo(() => responsiveThumbnailMax(gridViewport.width), [gridViewport.width]);
  const singleColumn = thumbSize >= maxThumbSize - 5;
  const connectionStepList = useMemo(() => connectionSteps(connectionType === "usb"), [connectionType]);
  const activeConnectionStep = connectionStageIndex(connectionStage);
  const visibleThumbTotal = visiblePhotos.length;
  const readyThumbs = useMemo(() => visiblePhotos.filter((photo) => thumbStatus[photo.handle] === "ready").length, [visiblePhotos, thumbStatus]);
  const unavailableThumbs = useMemo(() => visiblePhotos.filter((photo) => thumbStatus[photo.handle] === "unavailable").length, [visiblePhotos, thumbStatus]);
  const resolvedThumbs = readyThumbs + unavailableThumbs;
  const catalogRatio = scanProgress && scanProgress.total > 0
    ? Math.min(1, Math.max(scanProgress.scanned, scanProgress.loaded) / scanProgress.total)
    : 0;
  const connectionProgressPercent = connectionStage === "catalog" && catalogRatio > 0
    ? Math.round(86 + catalogRatio * 13)
    : connectionStageProgress(connectionStage);
  // Stage is kept as a second source of presentation truth.  Even if another
  // store action accidentally touches `connecting`, an active attempt never
  // collapses back to the apparently idle button while native I/O continues.
  const connectionBusy = connecting || connectionStage !== "idle";

  const thumbnailPageSizeFor = (size: number) => {
    if (!gridViewport.width || !gridViewport.height) return 12;
    const contentWidth = Math.max(1, gridViewport.width - 32);
    const columns = size >= maxThumbSize - 5
      ? 1
      : Math.max(1, Math.floor((contentWidth + 8) / (size + 8)));
    const cardWidth = (contentWidth - Math.max(0, columns - 1) * 8) / columns;
    const cardHeight = cardWidth * 2 / 3 + 48;
    const rows = Math.max(2, Math.ceil(gridViewport.height / Math.max(1, cardHeight)) + 2);
    return Math.min(40, Math.max(4, columns * rows));
  };

  const updateThumbSize = (next: number) => {
    const clamped = Math.min(maxThumbSize, Math.max(120, next));
    setThumbSize(clamped);
    setThumbLimit(Math.min(cameraPhotos.length, thumbnailPageSizeFor(clamped)));
    if (gridViewportRef.current) gridViewportRef.current.scrollTop = 0;
  };

  const refreshNetwork = async () => {
    const info = await invoke<WifiNetworkInfo>("get_wifi_network_info").catch(() => null);
    if (!info) return null;
    setWifiInfo(info);
    if (info.looksLikeNikonAp && info.gateway) store.setWifiHost(info.gateway);
    return info;
  };

  useEffect(() => {
    if (connected || connectionType === "usb") return;
    void refreshNetwork();
    const timer = window.setInterval(() => { void refreshNetwork(); }, 2000);
    return () => window.clearInterval(timer);
  }, [connected, connectionType]);

  useEffect(() => {
    let disposed = false;
    const cleanups: Array<() => void> = [];
    const register = async () => {
      const unlistenConnection = await listen<CameraConnectionProgress>("camera-connection-progress", ({ payload }) => {
        if (!connectionAttempts.current.isBusy()) return;
        setNativeConnectionProgress(payload);
        setConnectionStage((current) => connectionStageFromNativePhase(payload.phase, current));
      });
      const unlistenScan = await listen<CameraScanProgress>("camera-scan-progress", ({ payload }) => {
        setScanProgress(payload);
        if (connectionAttempts.current.isBusy()) setConnectionStage("catalog");
      });
      const unlistenDownload = await listen<CameraDownloadProgress>("camera-download-progress", ({ payload }) => {
        const now = performance.now();
        const previous = downloadRateRef.current;
        if (previous && previous.handle === payload.handle && now > previous.at && payload.bytesReceived >= previous.bytes) {
          const seconds = (now - previous.at) / 1000;
          if (seconds >= 0.12) setDownloadBytesPerSecond((payload.bytesReceived - previous.bytes) / seconds);
        }
        downloadRateRef.current = { handle: payload.handle, bytes: payload.bytesReceived, at: now };
        setNativeDownloadProgress(payload);
      });
      if (disposed) {
        unlistenConnection();
        unlistenScan();
        unlistenDownload();
      } else {
        cleanups.push(unlistenConnection, unlistenScan, unlistenDownload);
      }
    };
    void register();
    return () => {
      disposed = true;
      cleanups.forEach((cleanup) => cleanup());
    };
  }, []);

  useEffect(() => {
    if (!connected) return;
    const node = gridViewportRef.current;
    if (!node) return;
    const measure = () => setGridViewport((current) => {
      const next = { width: node.clientWidth, height: node.clientHeight };
      return current.width === next.width && current.height === next.height ? current : next;
    });
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(node);
    return () => observer.disconnect();
  }, [connected]);

  useEffect(() => {
    setThumbSize((current) => Math.min(current, maxThumbSize));
  }, [maxThumbSize]);

  useEffect(() => {
    if (!connected || !cameraPhotos.length || !gridViewport.width || !gridViewport.height) return;
    setThumbLimit((current) => Math.min(cameraPhotos.length, Math.max(current, thumbnailPageSizeFor(thumbSize))));
  }, [connected, cameraPhotos.length, gridViewport.width, gridViewport.height, thumbSize]);

  useEffect(() => {
    connectionAttempts.current.invalidate();
    setConnecting(false);
    setCancellingConnection(false);
    autoConnectTriggered.current = false;
    setConnectionStage("idle");
    setNativeConnectionProgress(null);
    setScanProgress(null);
    setContextMenu(null);
    setPreviewIndex(null);
    lastSelectedIndex.current = null;
  }, [connectionType]);

  useEffect(() => {
    if (!wifiInfo?.looksLikeNikonAp) autoConnectTriggered.current = false;
  }, [wifiInfo?.looksLikeNikonAp]);

  useEffect(() => {
    setThumbs({});
    setThumbLimit(12);
    setThumbStatus({});
    loadedThumbSizes.current.clear();
    pendingThumbs.current.clear();
    thumbnailSession.current += 1;
    setLoadingThumbs(false);
  }, [connected, cameraModel, connectionDetail]);

  // A user opening one photo has higher priority than background card work.
  // Let the current one-object GetThumb finish, then cancel the remaining
  // queue so the full-detail request can acquire the single PTP/IP session.
  // Closing the viewer resumes only the still-missing visible thumbnails.
  useEffect(() => {
    if (previewIndex !== null) {
      thumbnailSession.current += 1;
      pendingThumbs.current.clear();
      setLoadingThumbs(false);
      return;
    }
    if (connected && cameraPhotos.length) setThumbRevision((value) => value + 1);
  }, [previewIndex, connected, cameraPhotos.length]);

  useEffect(() => {
    if (!connected || cameraPhotos.length === 0) return;
    // PTP/IP GetThumb returns the camera's fixed embedded JPEG, so changing
    // the grid size must not download the same Wi-Fi object again. USB can ask
    // Windows for a larger rendition when the card grows.
    const requestSize = connectionType === "usb"
      ? adaptiveThumbnailRequestSize(thumbSize, window.devicePixelRatio || 1)
      : 1280;
    const targets = cameraPhotos.slice(0, thumbLimit).filter((photo) =>
      (connectionType === "usb"
        ? (loadedThumbSizes.current.get(photo.handle) || 0) < requestSize
        : !loadedThumbSizes.current.has(photo.handle))
      && !pendingThumbs.current.has(photo.handle));
    if (!targets.length) return;
    const session = thumbnailSession.current;
    targets.forEach((photo) => pendingThumbs.current.add(photo.handle));
    setThumbStatus((current) => ({
      ...current,
      ...Object.fromEntries(targets.map((photo) => [photo.handle, "loading"])),
    }));
    setLoadingThumbs(true);
    (async () => {
      const batchSize = connectionType === "usb" ? 8 : 1;
      for (let offset = 0; offset < targets.length; offset += batchSize) {
        const batch = targets.slice(offset, offset + batchSize);
        let rows: CameraThumbnail[] = [];
        try {
          rows = await invoke<CameraThumbnail[]>("get_camera_thumbnails", { handles: batch.map((photo) => photo.handle), size: requestSize });
          const returned = new Set(rows.map((row) => row.handle));
          const missing = batch.filter((photo) => !returned.has(photo.handle));
          // A transient Windows thumbnail-provider miss is common immediately
          // after MTP enumeration. Retry only missing USB objects once; a
          // PTP/IP miss should advance to the next card instead of doubling
          // the serial Wi-Fi wait.
          if (connectionType === "usb" && missing.length) {
            const retried = await invoke<CameraThumbnail[]>("get_camera_thumbnails", { handles: missing.map((photo) => photo.handle), size: requestSize }).catch(() => []);
            rows = [...rows, ...retried];
          }
          if (session !== thumbnailSession.current) return;
          const ready = new Set(rows.map((row) => row.handle));
          rows.forEach((row) => loadedThumbSizes.current.set(row.handle, requestSize));
          batch.filter((photo) => !ready.has(photo.handle)).forEach((photo) => loadedThumbSizes.current.set(photo.handle, requestSize));
          setThumbs((current) => ({ ...current, ...Object.fromEntries(rows.map((row) => [row.handle, row.url])) }));
          setThumbStatus((current) => ({
            ...current,
            ...Object.fromEntries(batch.map((photo) => [photo.handle, ready.has(photo.handle) ? "ready" : "unavailable"])),
          }));
        } catch {
          if (session === thumbnailSession.current) {
            batch.forEach((photo) => loadedThumbSizes.current.set(photo.handle, requestSize));
            setThumbStatus((current) => ({
              ...current,
              ...Object.fromEntries(batch.map((photo) => [photo.handle, "unavailable"])),
            }));
          }
        } finally {
          batch.forEach((photo) => pendingThumbs.current.delete(photo.handle));
        }
      }
      if (session === thumbnailSession.current) {
        setLoadingThumbs(pendingThumbs.current.size > 0);
        setThumbRevision((value) => value + 1);
      }
    })();
  }, [connected, connectionType, cameraPhotos, thumbLimit, thumbSize, thumbRevision]);

  const modeHelp = useMemo(() => {
    if (connectionType === "wifi-ap") return "Wi‑Fi 直连（AP）：电脑连接相机创建的 NIKON_… 网络；请让相机停留在“连接到计算机 → Wi‑Fi 连接 → 建立连接”的等待画面。显示“无 Internet”是正常的。";
    if (connectionType === "wifi-sta") return "Wi‑Fi 基础设施（STA）：相机和电脑连接同一路由器；先通过 Nikon Wireless Transmitter Utility 完成电脑配对，再填写相机在局域网中的 IPv4 地址。";
    return "USB（MTP/PTP）：相机 USB 模式设为 MTP/PTP，保持相机开机并解锁。缩略图由 Windows 便携设备缩略图资源读取。";
  }, [connectionType]);

  const doConnect = async () => {
    const requestedType = connectionType;
    if (cancellingConnection || switchingConnection) return;
    const attempt = connectionAttempts.current.begin(requestedType);
    if (!attempt) return;

    // Clear presentation state first, then make the lifecycle busy. Error text
    // no longer owns `connecting`; only this attempt may finish the lifecycle.
    setConnectionError("");
    setConnectionStage("detect");
    setNativeConnectionProgress(null);
    setScanProgress(null);
    setConnecting(true);

    const isCurrent = () => connectionAttempts.current.isCurrent(
      attempt,
      useTransferStore.getState().connectionType,
    );
    try {
      // Tauri commands run off-thread, but yielding across an actual paint
      // boundary guarantees that slow native discovery can never make the
      // button look as though it ignored the click.
      await waitForConnectionUiPaint();
      if (!isCurrent()) return;

      const payload = await runCameraTransition(async () => {
        // A cancelled attempt may have spent time waiting behind an older
        // transition. Never let that stale callback disconnect a newer camera
        // session when it eventually reaches the head of the queue.
        if (!isCurrent()) return null;
        // Always release a previous backend session before opening a new one.
        // The transition queue also ensures that a late, old connect cannot
        // publish its native session after a newer mode has already connected.
        await invoke("disconnect").catch(() => undefined);
        if (!isCurrent()) return null;

        const latest = requestedType === "usb" ? wifiInfo : await refreshNetwork();
        if (!isCurrent()) return null;

        const effectiveType: ConnectionType = requestedType;
        const host = effectiveType === "wifi-ap" ? (latest?.gateway || wifiHost || "192.168.1.1") : wifiHost.trim();
        setConnectionStage("session");
        const result = effectiveType === "usb"
          ? await invoke<ConnectResult>("connect_usb")
          : await invoke<ConnectResult>("connect_wifi", { args: { mode: effectiveType === "wifi-sta" ? "sta" : "ap", host, port: 15740 } });
        if (!isCurrent()) return null;
        if (!result.success) throw new Error(result.message);

        setConnectionStage("catalog");
        const photos = await invoke<CameraPhoto[]>("list_photos");
        if (!isCurrent()) return null;
        return { result, photos, effectiveType, host };
      });
      if (!payload) return;

      const { result, photos, effectiveType, host } = payload;
      const detail = effectiveType === "usb" ? "USB · Windows MTP/PTP" : `${effectiveType === "wifi-ap" ? "Wi‑Fi 直连 AP" : "Wi‑Fi 基础设施 STA"} · ${host}:15740`;
      setCameraInfo(result.camera_model || "Nikon 相机", detail, photos);
      setConnected(true);
      toast(`已连接 ${result.camera_model || "Nikon 相机"}，读取到 ${photos.length} 张照片`, "success");
    } catch (error) {
      if (!isCurrent()) return;
      await invoke("disconnect").catch(() => undefined);
      if (!isCurrent()) return;
      const raw = error instanceof Error ? error.message : String(error);
      const hint = requestedType === "usb"
        ? `${summarizeUsbConnectionError(raw)}。请保持相机开机并解锁，确认 USB 模式为 MTP/PTP；关闭 NX Studio、“照片”等可能占用相机的软件，重新插拔数据线后再扫描。`
        : wifiInfo?.looksLikeNikonAp
          ? `${raw}。电脑已连接 ${wifiInfo.ssid || "Nikon 相机网络"}，但相机没有开放 PTP/IP 服务。请在相机中进入“连接到计算机 → Wi‑Fi 连接 → 建立连接”并停留在等待连接画面；仅打开 Wi‑Fi 或“连接至智能设备/SnapBridge”不能进行电脑图传。`
          : `${raw}。STA 模式需要相机与电脑在同一路由器，并先用 Nikon Wireless Transmitter Utility 完成“连接到计算机”配对；请填写相机实际 IPv4，不能使用 AP 默认地址。`;
      setConnectionError(hint);
      setConnected(false);
    } finally {
      if (connectionAttempts.current.finish(attempt)) {
        setConnecting(false);
        setConnectionStage("idle");
        setNativeConnectionProgress(null);
      }
    }
  };

  const cancelConnect = () => {
    if (!connectionAttempts.current.isBusy() || cancellingConnection) return;
    const cancelledVersion = connectionAttempts.current.invalidate();
    autoConnectTriggered.current = true;
    setCancellingConnection(true);
    setConnecting(false);
    setConnectionStage("idle");
    setNativeConnectionProgress(null);
    setScanProgress(null);
    setConnectionError("");

    // Invalidate the UI generation first, then interrupt the native session
    // immediately. The backend closes both PTP/IP sockets, so this must not be
    // queued behind the connect/list operation that the user is cancelling.
    void invoke("disconnect").catch(() => undefined).finally(() => {
      if (!connectionAttempts.current.isVersion(cancelledVersion)) return;
      setCancellingConnection(false);
      setConnectionError("本次连接已取消。检查相机连接页面或网络后，可以重新连接并扫描照片。");
    });
  };

  useEffect(() => {
    if (connected || connectionBusy || switchingConnection || connectionType !== "wifi-ap" || !wifiInfo?.looksLikeNikonAp || autoConnectTriggered.current) return;
    autoConnectTriggered.current = true;
    void doConnect();
  }, [connected, connectionBusy, switchingConnection, connectionType, wifiInfo?.looksLikeNikonAp]);

  const doDisconnect = async () => {
    connectionAttempts.current.invalidate();
    await invoke("disconnect").catch(() => undefined);
    setConnecting(false);
    setConnected(false);
    setThumbs({});
    toast(t("transfer.disconnectedMsg"), "info");
  };

  const doDownload = async () => {
    let directory = saveDir;
    if (!directory) {
      const picked = await open({ directory: true, multiple: false });
      if (!picked || typeof picked !== "string") return;
      directory = picked;
      setSaveDir(directory);
    }
    const targets = selectedHandles.length ? cameraPhotos.filter((photo) => selectedHandles.includes(photo.handle)) : cameraPhotos;
    if (!targets.length) return;
    setDownloading(true);
    setNativeDownloadProgress(null);
    setDownloadBytesPerSecond(0);
    downloadRateRef.current = null;
    setDownloadProgress({ completed: 0, total: targets.length, current: targets[0].filename });
    let completed = 0;
    try {
      for (const photo of targets) {
        setDownloadProgress({ completed, total: targets.length, current: photo.filename });
        await invoke("download_photo", { handle: photo.handle, destPath: `${directory.replace(/[\\/]$/, "")}\\${photo.filename}` });
        completed += 1;
        setDownloadProgress({ completed, total: targets.length, current: photo.filename });
      }
      deselectAll();
      toast(t("transfer.downloadedMsg", { count: completed }), "success");
    } catch (error) {
      toast(`已下载 ${completed}/${targets.length}：${String(error)}`, "error");
    } finally {
      setDownloading(false);
      setDownloadProgress(null);
      setNativeDownloadProgress(null);
      setDownloadBytesPerSecond(0);
      downloadRateRef.current = null;
    }
  };

  const handlePhotoClick = (event: React.MouseEvent, index: number, handle: number) => {
    const ordered = cameraPhotos.map((photo) => photo.handle);
    if (event.shiftKey && lastSelectedIndex.current !== null) {
      const start = Math.min(lastSelectedIndex.current, index);
      const end = Math.max(lastSelectedIndex.current, index);
      const range = ordered.slice(start, end + 1);
      setSelectedHandles(event.ctrlKey || event.metaKey
        ? Array.from(new Set([...selectedHandles, ...range]))
        : range);
      return;
    }
    if (event.ctrlKey || event.metaKey) toggleSelect(handle);
    else setSelectedHandles([handle]);
    lastSelectedIndex.current = index;
  };

  const requestDelete = (handles: number[]) => {
    if (!handles.length || downloading || deleting) return;
    setConfirmDelete(handles);
  };

  const doDelete = async () => {
    const handles = confirmDelete || [];
    setConfirmDelete(null);
    if (!handles.length) return;
    const candidates = cameraPhotos.filter((photo) => handles.includes(photo.handle));
    const blocked = candidates.filter((photo) => photo.protected || photo.can_delete === false);
    const deletable = candidates.filter((photo) => !photo.protected && photo.can_delete !== false);
    if (!deletable.length) {
      toast("所选照片均已保护或相机禁止删除", "warn");
      return;
    }
    setDeleting(true);
    try {
      const result = await invoke<DeleteCameraResult>("delete_camera_photos", { handles: deletable.map((photo) => photo.handle) });
      if (result.deleted.length) {
        removePhotos(result.deleted);
        lastSelectedIndex.current = null;
        setThumbs((current) => {
          const next = { ...current };
          result.deleted.forEach((handle) => delete next[handle]);
          return next;
        });
        setPreviewIndex(null);
      }
      const protectedCount = blocked.length + result.protected.length;
      if (result.failed.length) {
        toast(`已永久删除 ${result.deleted.length} 张；${result.failed.length} 张失败${protectedCount ? `，${protectedCount} 张受保护` : ""}`, "warn");
      } else {
        toast(`已永久删除 ${result.deleted.length} 张${protectedCount ? `，跳过 ${protectedCount} 张受保护照片` : ""}`, "success");
      }
    } catch (error) {
      toast(`相机删除失败：${String(error)}`, "error");
    } finally {
      setDeleting(false);
    }
  };

  const contextMenuItems = (): MenuItem[] => {
    if (!contextMenu) return [];
    const photo = cameraPhotos[contextMenu.index];
    if (!photo) return [];
    const targets = selectedSet.has(photo.handle) && selectedHandles.length > 1 ? selectedHandles : [photo.handle];
    const blocked = photo.protected || photo.can_delete === false;
    return [
      { label: "打开大图", icon: "🔍", onClick: () => setPreviewIndex(contextMenu.index) },
      { label: "", divider: true, onClick: () => undefined },
      {
        label: blocked && targets.length === 1 ? "已保护，无法删除" : `从相机永久删除${targets.length > 1 ? `（${targets.length} 张）` : ""}`,
        icon: blocked && targets.length === 1 ? "🔒" : "🗑️",
        danger: !blocked || targets.length > 1,
        onClick: () => blocked && targets.length === 1 ? toast("这张照片已受保护，相机不允许删除", "warn") : requestDelete(targets),
      },
    ];
  };

  useEffect(() => {
    if (!connected) return;
    const onKey = (event: KeyboardEvent) => {
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "a") {
        event.preventDefault();
        selectAll(cameraPhotos.map((photo) => photo.handle));
      } else if (event.key === "Delete" && selectedHandles.length) {
        event.preventDefault();
        requestDelete(selectedHandles);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [connected, cameraPhotos, selectedHandles, downloading, deleting]);

  if (!connected) return (
    <div className="flex h-full items-center justify-center p-8">
      <div className="w-full max-w-2xl text-center">
        <div className={`relative mx-auto mb-5 flex h-20 w-20 items-center justify-center rounded-full bg-nps-tertiary text-4xl ${connectionBusy ? "animate-pulse" : ""}`}>
          📷
          {connectionBusy && <span className="absolute inset-0 animate-ping rounded-full border border-nps-accent/50" />}
        </div>
        <h2 className="mb-2 text-2xl font-bold">连接 Nikon 相机</h2>
        <p className="mb-4 text-[13px] leading-relaxed text-nps-text-dim">{modeHelp}</p>
        {connectionType !== "usb" && wifiInfo && <div className={`mb-4 rounded border p-3 text-left text-xs ${wifiInfo.looksLikeNikonAp ? "border-nps-success/40 bg-nps-success/10" : "border-nps-border bg-nps-tertiary"}`}>
          <div className="font-semibold">当前 Wi‑Fi：{wifiInfo.ssid || "未识别"}</div>
          <div className="mt-1 text-nps-text-dim">电脑 {wifiInfo.localIpv4 || "无 IPv4"} · 网关/相机候选 {wifiInfo.gateway || "未发现"}</div>
          {wifiInfo.looksLikeNikonAp && <div className="mt-1 text-nps-success">{connectionBusy ? "已识别 Nikon 相机热点，正在自动建立相机会话并扫描存储卡。" : "已识别 Nikon 相机热点，可以建立相机会话并读取存储卡。"}</div>}
        </div>}
        {connectionType !== "usb" && <label className="mb-4 block text-left">
          <span className="mb-1 block text-[11px] text-nps-text-dim">{connectionType === "wifi-ap" ? "相机地址（自动使用当前 Wi‑Fi 网关）" : "相机在路由器中的 IPv4 地址"}</span>
          <input value={wifiHost} onChange={(event) => store.setWifiHost(event.target.value)} disabled={connectionType === "wifi-ap" && !!wifiInfo?.gateway} className="w-full rounded border border-nps-border bg-nps-tertiary px-3 py-2 text-sm outline-none focus:border-nps-accent disabled:opacity-60" placeholder={connectionType === "wifi-ap" ? "通常为 192.168.1.1" : "例如 192.168.0.108"} />
        </label>}
        {connectionError && <div className="mb-4 rounded border border-nps-danger/40 bg-nps-danger/10 p-3 text-left text-xs leading-relaxed text-nps-danger">{connectionError}</div>}
        {(connectionBusy || cancellingConnection) && <div className="mb-4 rounded-lg border border-nps-accent/30 bg-nps-accent/10 p-4 text-left">
          <div className="mb-3 flex items-start gap-3">
            <span className="mt-0.5 h-5 w-5 shrink-0 animate-spin rounded-full border-2 border-nps-accent/20 border-t-nps-accent" />
            <div className="min-w-0">
              <div className="text-sm font-semibold text-nps-accent">{cancellingConnection ? "正在取消本次连接" : connectionStepList[Math.max(0, activeConnectionStep)]?.label || "正在准备连接"}</div>
              <div className="mt-1 text-xs leading-relaxed text-nps-text-dim">{cancellingConnection ? "正在中止当前相机命令并关闭 PTP/IP 会话，完成后即可重新连接。" : nativeConnectionProgress?.message || connectionStepList[Math.max(0, activeConnectionStep)]?.description}</div>
              {!cancellingConnection && nativeConnectionProgress && nativeConnectionProgress.totalAttempts > 1 && <div className="mt-1 text-[10px] text-nps-text-dim">连接尝试 {nativeConnectionProgress.attempt}/{nativeConnectionProgress.totalAttempts} · {nativeConnectionProgress.phase}</div>}
              {!cancellingConnection && connectionStage === "catalog" && scanProgress && <div className="mt-1 text-[10px] text-nps-text-dim">已扫描 {scanProgress.scanned}{scanProgress.total > 0 ? `/${scanProgress.total}` : ""} 个对象 · 已识别 {scanProgress.loaded} 张照片</div>}
            </div>
            <span className="ml-auto text-xs font-semibold text-nps-accent">{cancellingConnection ? "停止中" : `${connectionProgressPercent}%`}</span>
          </div>
          <div className="mb-4 h-1.5 overflow-hidden rounded-full bg-black/25"><div className="relative h-full rounded-full bg-nps-accent transition-[width] duration-300" style={{ width: `${cancellingConnection ? 100 : connectionProgressPercent}%` }}><span className="absolute inset-0 animate-pulse bg-white/20" /></div></div>
          <div className={`grid grid-cols-3 gap-2 ${cancellingConnection ? "opacity-45" : ""}`}>
            {connectionStepList.map((step, index) => {
              const done = index < activeConnectionStep;
              const active = index === activeConnectionStep;
              return <div key={step.key} className={`rounded border px-2.5 py-2 ${active ? "border-nps-accent/60 bg-nps-accent/10" : done ? "border-nps-success/30 bg-nps-success/5" : "border-nps-border/70 bg-black/10"}`}>
                <div className={`text-[11px] font-medium ${active ? "text-nps-accent" : done ? "text-nps-success" : "text-nps-text-dim"}`}>{done ? "✓" : index + 1} · {step.label}</div>
                <div className="mt-1 hidden text-[9px] leading-relaxed text-nps-text-dim sm:block">{step.description}</div>
              </div>;
            })}
          </div>
          <div className="mt-3 text-[10px] leading-relaxed text-nps-text-dim">{cancellingConnection ? "取消不会删除或修改相机中的任何照片。" : "连接与清单完成后会先显示照片卡片，再逐张解析缩略图；无需等待全部缩略图完成即可选择、下载或双击查看。"}</div>
        </div>}
        <div className="flex justify-center gap-2">
          {connectionType !== "usb" && <button onClick={() => void refreshNetwork()} disabled={connectionBusy || cancellingConnection} className="rounded border border-nps-border bg-nps-tertiary px-4 py-3 text-sm hover:bg-nps-hover disabled:opacity-50">重新检测网络</button>}
          {connectionBusy
            ? <button onClick={cancelConnect} disabled={cancellingConnection} className="rounded-lg border border-nps-danger/50 bg-nps-danger/10 px-8 py-3 text-[15px] font-semibold text-nps-danger hover:bg-nps-danger/20 disabled:opacity-60">取消连接</button>
            : <button onClick={doConnect} disabled={cancellingConnection || switchingConnection} className="rounded-lg bg-nps-accent px-8 py-3 text-[15px] font-semibold text-nps-primary hover:bg-nps-accent-hover disabled:opacity-60">{cancellingConnection ? "正在释放相机会话…" : switchingConnection ? "正在切换连接…" : "连接并扫描照片"}</button>}
        </div>
      </div>
    </div>
  );

  return (
    <div className="flex h-full flex-col">
      <div className="shrink-0 border-b border-nps-border">
        <div className="flex min-h-12 flex-wrap items-center gap-3 px-4 py-2.5">
          <div className="h-2 w-2 animate-pulse rounded-full bg-nps-success" />
          <span className="text-[13px] font-semibold">{cameraModel}</span><span className="text-[11px] text-nps-text-dim">{connectionDetail}</span><span className="text-xs text-nps-text-dim">{cameraPhotos.length} 张照片</span>
          {selectedHandles.length > 0 && <span className="text-xs text-nps-accent">已选 {selectedHandles.length}</span>}
          <div className="ml-auto flex items-center gap-2">
            <button onClick={() => updateThumbSize(thumbSize - 20)} aria-label="缩小照片卡片" className="h-7 w-7 rounded border border-nps-border bg-nps-tertiary">−</button>
            <input aria-label="照片卡片尺寸" type="range" min={120} max={maxThumbSize} step={10} value={Math.min(thumbSize, maxThumbSize)} onChange={(event) => updateThumbSize(Number(event.target.value))} className="w-28 accent-[var(--accent)]" />
            <button onClick={() => updateThumbSize(thumbSize + 20)} aria-label="放大照片卡片" className="h-7 w-7 rounded border border-nps-border bg-nps-tertiary">＋</button>
            <span className="mr-2 text-[10px] text-nps-text-dim">{singleColumn ? "单张适配" : `缩略图 ${thumbSize}px`} · 已解析 {readyThumbs}/{visibleThumbTotal}{loadingThumbs ? " · 载入中" : ""}</span>
            <button onClick={() => selectAll(cameraPhotos.map((photo) => photo.handle))} disabled={downloading || deleting} className="rounded border border-nps-border bg-nps-tertiary px-2.5 py-1 text-[11px] disabled:opacity-50">全选</button>
            <button onClick={() => { deselectAll(); lastSelectedIndex.current = null; }} disabled={downloading || deleting} className="rounded border border-nps-border bg-nps-tertiary px-2.5 py-1 text-[11px] disabled:opacity-50">取消选择</button>
            <button onClick={() => requestDelete(selectedHandles)} disabled={downloading || deleting || !selectedHandles.length} className="rounded border border-nps-danger/40 px-2.5 py-1 text-[11px] text-nps-danger disabled:opacity-40">{deleting ? "正在删除…" : "删除"}</button>
            <button onClick={doDownload} disabled={downloading || deleting || !cameraPhotos.length} className="rounded bg-nps-accent px-4 py-1 text-xs font-semibold text-nps-primary disabled:opacity-60">{downloading && downloadProgress ? `${downloadProgress.completed}/${downloadProgress.total}` : selectedHandles.length ? `下载原文件（${selectedHandles.length}）` : "下载全部原文件"}</button>
            <button onClick={doDisconnect} disabled={downloading || deleting} className="rounded border border-nps-danger/40 px-2.5 py-1 text-[11px] text-nps-danger disabled:opacity-40">断开</button>
          </div>
        </div>
        {downloadProgress && <div className="flex items-center gap-3 border-t border-nps-border/60 px-4 py-1.5 text-[11px] text-nps-text-dim">
          <span className="shrink-0">正在完整复制 {downloadProgress.completed}/{downloadProgress.total}</span>
          <div className="h-1.5 flex-1 overflow-hidden rounded-full bg-nps-tertiary"><div className="h-full rounded-full bg-nps-accent transition-all" style={{ width: `${downloadProgress.total ? ((downloadProgress.completed + (nativeDownloadProgress?.percent || 0) / 100) / downloadProgress.total) * 100 : 0}%` }} /></div>
          <span className="max-w-56 truncate">{downloadProgress.current}</span>
          {nativeDownloadProgress && <span className="shrink-0 text-[10px] text-nps-text-dim">{humanSize(nativeDownloadProgress.bytesReceived)} / {humanSize(nativeDownloadProgress.totalBytes)} · {nativeDownloadProgress.percent.toFixed(0)}%{downloadBytesPerSecond > 0 ? ` · ${humanRate(downloadBytesPerSecond)}` : ""}</span>}
        </div>}
        {!!cameraPhotos.length && <div className="flex items-center gap-3 border-t border-nps-border/60 px-4 py-1.5 text-[10px] text-nps-text-dim">
          {(loadingThumbs || resolvedThumbs < visibleThumbTotal) && <span className="h-3 w-3 shrink-0 animate-spin rounded-full border border-nps-text-dim/30 border-t-nps-accent" />}
          <span className="shrink-0">{resolvedThumbs < visibleThumbTotal ? `正在逐张解析相机缩略图 ${resolvedThumbs}/${visibleThumbTotal}` : `当前 ${visibleThumbTotal} 张缩略图处理完成`}</span>
          <div className="h-1 flex min-w-20 flex-1 overflow-hidden rounded-full bg-nps-tertiary"><div className="h-full rounded-full bg-nps-accent transition-all" style={{ width: `${visibleThumbTotal ? (resolvedThumbs / visibleThumbTotal) * 100 : 0}%` }} /></div>
          <span className="max-w-[32rem] truncate">{connectionType === "usb" ? "Windows 正在解析便携设备预览；滚动时继续加载下一批。" : "Wi‑Fi 每读取一张立即显示；双击可单独读取更清晰预览，无需等待全部完成。"}{unavailableThumbs ? ` · ${unavailableThumbs} 张未提供内嵌缩略图` : ""}</span>
        </div>}
      </div>
      <div ref={gridViewportRef} className="flex-1 overflow-y-auto p-4" onScroll={(event) => { const node = event.currentTarget; if (node.scrollHeight - node.scrollTop - node.clientHeight < 700) setThumbLimit((value) => Math.min(cameraPhotos.length, value + thumbnailPageSizeFor(thumbSize))); }}>
        {!cameraPhotos.length ? <div className="flex h-full items-center justify-center text-sm text-nps-text-dim">设备已连接，但存储卡中未发现 NEF/JPEG/TIFF 照片</div> : (
          <div className="grid gap-2" style={{ gridTemplateColumns: singleColumn ? "minmax(0, 1fr)" : `repeat(auto-fill, minmax(${thumbSize}px, 1fr))` }}>
            {visiblePhotos.map((photo, index) => {
              const selected = selectedSet.has(photo.handle);
              const protectedPhoto = photo.protected || photo.can_delete === false;
              const status = thumbStatus[photo.handle];
              return <button key={photo.handle} onClick={(event) => handlePhotoClick(event, index, photo.handle)} onDoubleClick={() => setPreviewIndex(index)} onContextMenu={(event) => {
                event.preventDefault();
                if (!selectedSet.has(photo.handle)) setSelectedHandles([photo.handle]);
                lastSelectedIndex.current = index;
                setContextMenu({ x: event.clientX, y: event.clientY, index });
              }} title={`${photo.filename} · 双击查看详情${protectedPhoto ? " · 已保护" : ""}`} className={`group relative overflow-hidden rounded-md border-2 text-left transition hover:-translate-y-0.5 hover:border-nps-accent-hover ${selected ? "border-nps-accent" : "border-transparent bg-nps-tertiary"}`}>
                <div className="relative aspect-[3/2] overflow-hidden bg-nps-tertiary">
                  {thumbs[photo.handle] ? <img src={thumbs[photo.handle]} alt={photo.filename} loading="lazy" className="h-full w-full bg-black/20 object-contain" /> : status === "unavailable" ? <div className="flex h-full items-center justify-center text-center text-nps-text-dim"><div><div className="text-3xl">📷</div><div className="mt-1 text-[10px]">未找到内嵌预览 · 双击尝试清晰大图</div></div></div> : <div className="absolute inset-0 animate-pulse overflow-hidden bg-nps-hover"><div className="absolute inset-x-0 top-0 h-2/3 bg-gradient-to-br from-white/5 via-white/10 to-transparent" /><div className="absolute bottom-5 left-1/2 -translate-x-1/2 whitespace-nowrap text-[10px] text-nps-text-dim">{status === "loading" ? "正在从相机解析 JPEG 预览…" : `等待读取 ${formatName(photo.format)} 缩略图…`}</div></div>}
                </div>
                <div className="px-2 py-1.5"><div className="truncate text-xs">{photo.filename}</div><div className="text-[10px] text-nps-text-dim">{photo.width && photo.height ? `${photo.width}×${photo.height} · ` : ""}{humanSize(photo.size)}</div></div>
                <span className={`absolute left-1 top-1 grid h-5 w-5 place-items-center rounded text-[11px] ${selected ? "bg-nps-accent text-black" : "bg-black/40 text-white/60"}`}>{selected ? "✓" : ""}</span>
                <span className="absolute right-1 top-1 rounded bg-black/60 px-1.5 py-0.5 text-[9px] text-white">{protectedPhoto ? "🔒 已保护" : "双击查看"}</span>
              </button>;
            })}
          </div>
        )}
      </div>
      {previewIndex !== null && <TransferPreview photos={cameraPhotos} initialIndex={previewIndex} thumbnails={thumbs} onClose={() => setPreviewIndex(null)} />}
      {contextMenu && <ContextMenu x={contextMenu.x} y={contextMenu.y} items={contextMenuItems()} onClose={() => setContextMenu(null)} />}
      {confirmDelete && <ConfirmDialog
        title="从相机永久删除"
        message={`确定永久删除所选 ${confirmDelete.length} 张照片吗？相机存储不会进入 Windows 回收站；已保护或设备禁止删除的照片会自动跳过。`}
        confirmLabel="永久删除"
        cancelLabel="取消"
        danger
        onConfirm={() => { void doDelete(); }}
        onCancel={() => setConfirmDelete(null)}
      />}
    </div>
  );
}
