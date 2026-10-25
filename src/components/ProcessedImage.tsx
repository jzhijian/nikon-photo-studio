import { useCallback, useEffect, useRef, useState } from "react";
import { fetchFullPreview, fetchFastPreview } from "./Thumbnail";
import { useTranslation } from "react-i18next";
import { processImageData, isNeutral } from "../lib/imageProcess";
import type { AdjustParams, Photo } from "../stores/libraryStore";

/**
 * Hard cap on the processing buffer's long side (px). This matches the RAW
 * backend preview and preserves detail on high-DPI displays / moderate zoom.
 */
const MAX_BUF = 8192;
/** Minimum buffer dimension so tiny panels still look decent. */
const MIN_BUF = 96;
/** Stable preview sizes keep the disk/memory cache bounded while zooming. */
const PREVIEW_BUCKETS = [2048, 3072, 4096, 6144, 8192] as const;

function previewBucket(required: number): number {
  return PREVIEW_BUCKETS.find((size) => size >= required) ?? PREVIEW_BUCKETS[PREVIEW_BUCKETS.length - 1];
}

interface Props {
  photo: Photo;
  /** Develop settings to apply. Ignored when `bypass` is true. */
  settings: AdjustParams;
  /** Render the untouched original (used for before/after compare). */
  bypass?: boolean;
  /** Current zoom factor of the viewer (used to keep zoomed-in views sharp). */
  zoom?: number;
  className?: string;
  /** Called after the first canvas frame has actually been painted. */
  onReady?: () => void;
}

/**
 * Renders the photo onto a canvas with the develop engine applied at the pixel
 * level.
 *
 * Sharpness strategy: the processing buffer is sized to match the actual
 * on-screen display area multiplied by `devicePixelRatio` (and the current
 * zoom), capped at the source's native resolution and `MAX_BUF`. This yields a
 * ~1:1 device-pixel mapping so the centered image is as sharp as possible
 * instead of being a fixed, possibly too-small, downscaled preview.
 *
 * Reprocessing is coalesced to one pass per animation frame; re-rasterization
 * (on resize / zoom) is debounced.
 */
export default function ProcessedImage({
  photo,
  settings,
  bypass = false,
  zoom = 1,
  className,
  onReady,
}: Props) {
  const { i18n } = useTranslation();
  const rootRef = useRef<HTMLDivElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const imgRef = useRef<HTMLImageElement | null>(null);
  const sourcePathRef = useRef("");
  const previewRequestRef = useRef({ path: "", size: PREVIEW_BUCKETS[0] as number });
  const baseDataRef = useRef<ImageData | null>(null);
  const bufDimsRef = useRef<{ w: number; h: number }>({ w: 0, h: 0 });
  const displayRef = useRef<{ w: number; h: number }>({ w: 0, h: 0 });
  const rafRef = useRef<number>(0);
  const readyCallbackRafRef = useRef<number>(0);
  const readyNotifiedRef = useRef(false);
  const onReadyRef = useRef(onReady);
  onReadyRef.current = onReady;
  const [status, setStatus] = useState<"loading" | "ready" | "failed">("loading");
  const [renderingFull, setRenderingFull] = useState(false);
  const [loadError, setLoadError] = useState("");
  const [displaySize, setDisplaySize] = useState({ w: 0, h: 0 });

  // Request only the pixels the current display can actually show. The small
  // headroom prevents fractional layout/DPI rounding from causing upscaling.
  // Raster images are capped at their native dimensions; zoom cannot invent
  // detail beyond the source. RAW dimensions may be unknown during import, in
  // which case the backend's 8192px safety limit is used.
  const displayLong = Math.max(displaySize.w, displaySize.h, 1);
  const nativeLong = Math.max(photo.width, photo.height);
  // NEF import dimensions describe the camera's small embedded JPEG, not the
  // sensor image. Never cap RAW zoom requests to those placeholder dimensions.
  const isRaw = photo.format.toUpperCase() === "NEF";
  const requiredLong = Math.min(
    MAX_BUF,
    Math.ceil(displayLong * (window.devicePixelRatio || 1) * Math.max(1, zoom) * 1.05)
  );
  const candidatePreviewSize = previewBucket(
    !isRaw && nativeLong > 0 ? Math.min(requiredLong, nativeLong) : requiredLong
  );
  // Keep the selected size monotonic for the current photo: shrinking the
  // window or zooming out must not replace a sharp source with a smaller one.
  if (previewRequestRef.current.path !== photo.filePath) {
    previewRequestRef.current = { path: photo.filePath, size: candidatePreviewSize };
  } else if (candidatePreviewSize > previewRequestRef.current.size) {
    previewRequestRef.current.size = candidatePreviewSize;
  }
  const fullPreviewSize = previewRequestRef.current.size;

  // Latest-value refs so the heavy callbacks stay referentially stable.
  const settingsRef = useRef(settings);
  settingsRef.current = settings;
  const bypassRef = useRef(bypass);
  bypassRef.current = bypass;
  const zoomRef = useRef(zoom);
  zoomRef.current = zoom;

  /**
   * Compute the contain-fit display size (CSS px) at zoom 1 from the source
   * native dimensions and the current on-screen box. Returns null until both
   * are known.
   */
  const computeFit = useCallback((): {
    cssW: number;
    cssH: number;
    aw: number;
    ah: number;
  } | null => {
    const img = imgRef.current;
    if (!img || !img.naturalWidth) return null;
    const aw = img.naturalWidth;
    const ah = img.naturalHeight;
    let { w: W, h: H } = displayRef.current;
    // The first layout can fire (from img.onload) before the ResizeObserver
    // has run; measure the box directly in that case so we never size against
    // a 0x0 area (which would produce a tiny, blurry buffer).
    if ((W === 0 || H === 0) && rootRef.current) {
      const r = rootRef.current.getBoundingClientRect();
      W = r.width;
      H = r.height;
      if (W > 0 && H > 0) displayRef.current = { w: W, h: H };
    }
    if (W === 0 || H === 0) return null;
    const fit = Math.min(W / aw, H / ah);
    return { cssW: aw * fit, cssH: ah * fit, aw, ah };
  }, []);

  /**
   * Set the canvas CSS size to the contain-fit size times zoom (centered by
   * the root flexbox). Cheap - called immediately on zoom/resize for responsive
   * feedback, independently of the heavier re-rasterization. Because the canvas
   * backing store is sized to this times devicePixelRatio, the backing maps
   * roughly 1:1 to device pixels at every zoom level. Setting the CSS size
   * explicitly (instead of object-contain on a full-size canvas) means the
   * browser never resamples a fixed buffer, so the centered image stays as
   * sharp as possible - including when zoomed in.
   */
  const applyLayout = useCallback(() => {
    const fit = computeFit();
    const cv = canvasRef.current;
    if (!fit || !cv) return;
    const z = Math.max(1, zoomRef.current);
    cv.style.width = `${fit.cssW * z}px`;
    cv.style.height = `${fit.cssH * z}px`;
  }, [computeFit]);

  /** Compute the backing-store dimensions: display size x zoom x DPR, capped. */
  const computeBuf = useCallback((): { w: number; h: number } | null => {
    const fit = computeFit();
    if (!fit) return null;
    const dpr = window.devicePixelRatio || 1;
    const z = Math.max(1, zoomRef.current);
    let bw = Math.round(fit.cssW * z * dpr);
    let bh = Math.round(fit.cssH * z * dpr);
    // Never upscale beyond the source native resolution.
    bw = Math.min(bw, fit.aw);
    bh = Math.min(bh, fit.ah);
    // Cap the long side for performance / memory.
    const long = Math.max(bw, bh);
    if (long > MAX_BUF) {
      const s = MAX_BUF / long;
      bw = Math.round(bw * s);
      bh = Math.round(bh * s);
    }
    return { w: Math.max(bw, MIN_BUF), h: Math.max(bh, MIN_BUF) };
  }, [computeFit]);

  /** Rasterize the source image into the base buffer at the computed size. */
  const rasterize = useCallback((): boolean => {
    const img = imgRef.current;
    // Lay out the canvas (CSS size) first; this also measures the display area
    // on first run so we never size the buffer against a 0x0 box.
    applyLayout();
    const dims = computeBuf();
    if (!img || !dims) return false;
    const off = document.createElement("canvas");
    off.width = dims.w;
    off.height = dims.h;
    const octx = off.getContext("2d", { willReadFrequently: true });
    if (!octx) {
      setStatus("failed");
      setLoadError("无法创建图像画布");
      return false;
    }
    octx.imageSmoothingEnabled = true;
    octx.imageSmoothingQuality = "high";
    octx.drawImage(img, 0, 0, dims.w, dims.h);
    try {
      baseDataRef.current = octx.getImageData(0, 0, dims.w, dims.h);
      bufDimsRef.current = dims;
      const cv = canvasRef.current;
      if (cv) {
        cv.width = dims.w;
        cv.height = dims.h;
      }
      setStatus("ready");
      return true;
    } catch (error) {
      // Most commonly caused by a cross-origin asset:// image tainting the
      // canvas. The Rust preview commands now return same-origin data URLs,
      // but retaining a useful error makes old/stale packaged backends clear.
      setLoadError(error instanceof Error ? error.message : String(error));
      setStatus("failed");
      return false;
    }
  }, [applyLayout, computeBuf]);

  const notifyPainted = useCallback(() => {
    if (readyNotifiedRef.current) return;
    readyNotifiedRef.current = true;
    cancelAnimationFrame(readyCallbackRafRef.current);
    // Wait one more frame so the freshly painted canvas is presented before
    // a progressive parent fades out its thumbnail placeholder.
    readyCallbackRafRef.current = requestAnimationFrame(() => onReadyRef.current?.());
  }, []);

  /** Run the develop engine on the base buffer and paint the result. */
  const process = useCallback(() => {
    cancelAnimationFrame(rafRef.current);
    rafRef.current = requestAnimationFrame(() => {
      const base = baseDataRef.current;
      const cv = canvasRef.current;
      if (!base || !cv) return;
      const ctx = cv.getContext("2d", { willReadFrequently: true });
      if (!ctx) return;
      if (bypassRef.current || isNeutral(settingsRef.current)) {
        ctx.putImageData(base, 0, 0);
        notifyPainted();
        return;
      }
      const working = new ImageData(
        new Uint8ClampedArray(base.data),
        base.width,
        base.height
      );
      processImageData(working, settingsRef.current);
      ctx.putImageData(working, 0, 0);
      notifyPainted();
    });
  }, [notifyPainted]);

  /** Re-rasterize only if the target buffer size changed meaningfully. */
  const maybeRerasterize = useCallback(() => {
    applyLayout();
    const dims = computeBuf();
    const cur = bufDimsRef.current;
    if (!dims || !imgRef.current) return;
    const changed =
      cur.w === 0 ||
      Math.abs(dims.w - cur.w) > cur.w * 0.03 ||
      Math.abs(dims.h - cur.h) > cur.h * 0.03;
    if (changed) {
      rasterize();
      process();
    }
  }, [applyLayout, computeBuf, rasterize, process]);

  // Load + rasterize the source whenever the photo changes.
  //
  // Progressive loading (Lightroom-style): show a fast low-resolution preview
  // (the camera's embedded JPEG for NEF) almost instantly, then develop the
  // full-resolution image in the background and swap it in when ready. The
  // user sees the photo in well under a second instead of waiting several
  // seconds for the full RAW demosaic; the image then pops to full sharpness.
  // A small indicator shows while the high-resolution pass is still rendering.
  useEffect(() => {
    let alive = true;
    const samePhoto = sourcePathRef.current === photo.filePath;
    const isUpgrade = samePhoto && imgRef.current !== null;
    let havePreview = isUpgrade;
    let gotFull = false;
    if (!isUpgrade) {
      sourcePathRef.current = photo.filePath;
      readyNotifiedRef.current = false;
      cancelAnimationFrame(readyCallbackRafRef.current);
      imgRef.current = null;
      setStatus("loading");
      setRenderingFull(false);
      setLoadError("");
      baseDataRef.current = null;
      bufDimsRef.current = { w: 0, h: 0 };
    }

    if (!photo.previewable) {
      setStatus("failed");
      return;
    }

    const loadInto = (url: string, isFull: boolean) => {
      const img = new Image();
      // Allows canvas pixel access when an older backend still returns a
      // Tauri asset-protocol URL. Must be assigned before src.
      img.crossOrigin = "anonymous";
      img.onload = () => {
        if (!alive) return;
        // Never let a late-arriving fast preview downgrade an image that has
        // already been replaced by the full-resolution develop.
        if (!isFull && gotFull) return;
        imgRef.current = img;
        if (!rasterize()) return;
        process();
        havePreview = true;
        if (isFull) {
          gotFull = true;
          setRenderingFull(false);
        }
        setStatus("ready");
      };
      img.onerror = () => {
        if (alive && !havePreview) {
          setLoadError(isFull ? "高清预览加载失败" : "NEF 内嵌预览加载失败");
          setStatus("failed");
        }
      };
      img.src = url;
    };

    // Stage 1: instant fast preview (embedded JPEG for NEF).
    if (!isUpgrade) {
      fetchFastPreview(photo.filePath)
        .then((url) => {
          if (!alive) return;
          if (!gotFull) setRenderingFull(true);
          loadInto(url, false);
        })
        .catch(() => {
          // No fast preview available; the full-resolution pass is the fallback.
        });
    } else {
      setRenderingFull(true);
    }

    // Stage 2: full-resolution develop (background). Swap in when ready.
    fetchFullPreview(photo.filePath, fullPreviewSize)
      .then((url) => {
        if (alive) loadInto(url, true);
      })
      .catch(() => {
        if (!alive) return;
        setRenderingFull(false);
        if (!havePreview) setStatus("failed");
      });

    return () => {
      alive = false;
      cancelAnimationFrame(readyCallbackRafRef.current);
    };
  }, [photo.filePath, photo.previewable, fullPreviewSize, rasterize, process]);

  // Track the display area; re-rasterize (debounced) when it resizes.
  useEffect(() => {
    const el = rootRef.current;
    if (!el) return;
    const rect = el.getBoundingClientRect();
    displayRef.current = { w: rect.width, h: rect.height };
    setDisplaySize({ w: rect.width, h: rect.height });
    let t = 0;
    const ro = new ResizeObserver((entries) => {
      const cr = entries[0].contentRect;
      displayRef.current = { w: cr.width, h: cr.height };
      setDisplaySize((prev) =>
        Math.abs(prev.w - cr.width) < 1 && Math.abs(prev.h - cr.height) < 1
          ? prev
          : { w: cr.width, h: cr.height }
      );
      applyLayout();
      window.clearTimeout(t);
      t = window.setTimeout(() => maybeRerasterize(), 160);
    });
    ro.observe(el);
    return () => {
      ro.disconnect();
      window.clearTimeout(t);
    };
  }, [applyLayout, maybeRerasterize]);

  // Re-rasterize (debounced) when zoom changes so zoomed-in views stay sharp.
  useEffect(() => {
    if (!imgRef.current) return;
    applyLayout();
    const t = window.setTimeout(() => maybeRerasterize(), 180);
    return () => window.clearTimeout(t);
  }, [zoom, applyLayout, maybeRerasterize]);

  // Re-process whenever the develop settings / bypass change.
  useEffect(() => {
    if (status !== "ready") return;
    process();
  }, [settings, bypass, status, process]);

  if (status === "failed") {
    return (
      <div className="w-full h-full flex flex-col items-center justify-center gap-1 bg-[var(--thumb-bg)]">
        <div className="text-4xl">📷</div>
        <div className="font-bold tracking-widest text-nps-accent text-lg">{photo.format}</div>
        <div className="text-[11px] text-nps-text-dim">{photo.filename}</div>
        {loadError && <div className="max-w-md px-4 text-center text-[10px] text-red-400/80">{loadError}</div>}
      </div>
    );
  }

  return (
    <div ref={rootRef} className="w-full h-full flex items-center justify-center relative">
      {status === "loading" && (
        <div className="absolute inset-0 flex items-center justify-center">
          <div className="w-6 h-6 border-2 border-nps-accent border-t-transparent rounded-full animate-spin" />
        </div>
      )}
      {renderingFull && status === "ready" && (
        <div className="absolute top-3 left-3 flex items-center gap-1.5 px-2 py-1 rounded bg-black/60 text-[10px] text-white/85 z-10 pointer-events-none">
          <div className="w-3 h-3 border border-white/70 border-t-transparent rounded-full animate-spin" />
          {i18n.language.startsWith("zh") ? "\u6B63\u5728\u6E32\u67D3\u9AD8\u6E05\u2026" : "Rendering high-res\u2026"}
        </div>
      )}
      <canvas
        ref={canvasRef}
        className={`block ${className ?? ""}`}
        style={{
          display: status === "ready" ? "block" : "none",
          transition: "width 0.1s ease-out, height 0.1s ease-out",
        }}
      />
    </div>
  );
}
