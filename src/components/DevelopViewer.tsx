import { useEffect, useRef, useState, useCallback } from "react";
import ProcessedImage from "./ProcessedImage";
import { isNeutral } from "../lib/imageProcess";
import type { AdjustParams, Photo } from "../stores/libraryStore";

interface Props {
  photo: Photo;
  adjust: AdjustParams;
}

const MIN_ZOOM = 1;
const MAX_ZOOM = 8;
const ZOOM_STEP = 1.15;

/**
 * In-place develop viewer (LR-style): shows the photo centered, supports
 * Ctrl+mouse-wheel zoom, drag-to-pan when zoomed, and double-click to toggle
 * between fit (100%) and a zoomed view.
 *
 * The image is rendered by <ProcessedImage/>, which applies the develop
 * settings at the pixel level (real color science, not a CSS approximation).
 *
 * Before/After comparison: split slider, side-by-side, or hold-Y-to-compare.
 * Keyboard: backslash (\) toggles split compare, Y hold-to-compare.
 */
export default function DevelopViewer({ photo, adjust }: Props) {
  const [zoom, setZoom] = useState(1);
  const [pan, setPan] = useState({ x: 0, y: 0 });
  const containerRef = useRef<HTMLDivElement>(null);
  const dragState = useRef<{ startX: number; startY: number; baseX: number; baseY: number } | null>(null);
  const [dragging, setDragging] = useState(false);

  // Comparison state
  const [compareMode, setCompareMode] = useState<"off" | "split" | "sideBySide">("off");
  const [splitPos, setSplitPos] = useState(50); // percentage
  const [holdCompare, setHoldCompare] = useState(false);
  const splitDragging = useRef(false);

  const hasEdits = !isNeutral(adjust);

  // Reset zoom/pan whenever the photo changes.
  useEffect(() => {
    setZoom(1);
    setPan({ x: 0, y: 0 });
    dragState.current = null;
    setCompareMode("off");
    setHoldCompare(false);
  }, [photo.filePath]);

  // Keyboard shortcuts for comparison
  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      const tag = (e.target as HTMLElement)?.tagName;
      if (tag === "INPUT" || tag === "TEXTAREA") return;
      if (e.key === "\\") {
        e.preventDefault();
        setCompareMode((m) => (m === "off" ? "split" : "off"));
      }
      if (e.key === "y" || e.key === "Y") {
        setHoldCompare(true);
      }
    };
    const onKeyUp = (e: KeyboardEvent) => {
      if (e.key === "y" || e.key === "Y") {
        setHoldCompare(false);
      }
    };
    window.addEventListener("keydown", onKeyDown);
    window.addEventListener("keyup", onKeyUp);
    return () => {
      window.removeEventListener("keydown", onKeyDown);
      window.removeEventListener("keyup", onKeyUp);
    };
  }, []);

  const clampPan = useCallback((x: number, y: number, z: number) => {
    const el = containerRef.current;
    if (!el) return { x, y };
    const r = el.getBoundingClientRect();
    const maxX = ((z - 1) * r.width) / 2 + r.width * 0.25;
    const maxY = ((z - 1) * r.height) / 2 + r.height * 0.25;
    return {
      x: Math.max(-maxX, Math.min(maxX, x)),
      y: Math.max(-maxY, Math.min(maxY, y)),
    };
  }, []);

  // Ctrl+wheel zoom
  useEffect(() => {
    const el = containerRef.current;
    if (!el) return;
    const onWheel = (e: WheelEvent) => {
      if (!e.ctrlKey) return;
      e.preventDefault();
      const factor = e.deltaY < 0 ? ZOOM_STEP : 1 / ZOOM_STEP;
      setZoom((z) => {
        const nz = Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, z * factor));
        if (nz <= 1) setPan({ x: 0, y: 0 });
        else setPan((p) => clampPan(p.x, p.y, nz));
        return nz;
      });
    };
    el.addEventListener("wheel", onWheel, { passive: false });
    return () => el.removeEventListener("wheel", onWheel);
  }, [clampPan]);

  const onMouseDown = (e: React.MouseEvent) => {
    if (zoom <= 1) return;
    e.preventDefault();
    dragState.current = { startX: e.clientX, startY: e.clientY, baseX: pan.x, baseY: pan.y };
    setDragging(true);
  };

  const onMouseMove = (e: React.MouseEvent) => {
    // Handle split divider drag
    if (splitDragging.current && compareMode === "split") {
      const el = containerRef.current;
      if (el) {
        const rect = el.getBoundingClientRect();
        const pct = ((e.clientX - rect.left) / rect.width) * 100;
        setSplitPos(Math.max(5, Math.min(95, pct)));
      }
      return;
    }
    const d = dragState.current;
    if (!d) return;
    setPan(clampPan(d.baseX + (e.clientX - d.startX), d.baseY + (e.clientY - d.startY), zoom));
  };

  const endDrag = () => {
    dragState.current = null;
    setDragging(false);
    splitDragging.current = false;
  };

  const zoomBy = (factor: number) => {
    setZoom((z) => {
      const nz = Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, z * factor));
      if (nz <= 1) setPan({ x: 0, y: 0 });
      else setPan((p) => clampPan(p.x, p.y, nz));
      return nz;
    });
  };

  const toggleZoom = () => {
    if (zoom > 1) {
      setZoom(1);
      setPan({ x: 0, y: 0 });
    } else {
      setZoom(2.5);
    }
  };

  const showOriginal = holdCompare || (compareMode !== "off" && !hasEdits);

  // ── Split comparison view ──
  if (compareMode === "split" && hasEdits) {
    return (
      <div
        ref={containerRef}
        className="w-full h-full flex items-center justify-center overflow-hidden select-none relative"
        onMouseMove={onMouseMove}
        onMouseUp={endDrag}
        onMouseLeave={endDrag}
      >
        <div className="relative w-full h-full flex items-center justify-center">
          {/* Adjusted (full background) */}
          <div className="absolute inset-0 flex items-center justify-center p-8">
            <div className="max-w-full max-h-full w-full h-full flex items-center justify-center">
              <ProcessedImage photo={photo} settings={adjust} />
            </div>
          </div>
          {/* Original (clipped to left of split) */}
          <div
            className="absolute inset-0 flex items-center justify-center p-8 overflow-hidden"
            style={{ clipPath: `inset(0 ${100 - splitPos}% 0 0)` }}
          >
            <div className="max-w-full max-h-full w-full h-full flex items-center justify-center">
              <ProcessedImage photo={photo} settings={adjust} bypass />
            </div>
          </div>
          {/* Split divider */}
          <div
            className="absolute top-0 bottom-0 w-0.5 bg-white/80 cursor-col-resize z-10"
            style={{ left: `${splitPos}%` }}
            onMouseDown={(e) => {
              e.preventDefault();
              e.stopPropagation();
              splitDragging.current = true;
            }}
          >
            <div className="absolute top-1/2 -translate-y-1/2 -translate-x-1/2 w-7 h-7 rounded-full bg-white/90 shadow-lg flex items-center justify-center text-[10px] text-gray-700 font-bold">
              ↔
            </div>
          </div>
          {/* Labels */}
          <div className="absolute top-3 left-3 px-2 py-0.5 rounded bg-black/60 text-[10px] text-white/90 z-10">
            原图
          </div>
          <div className="absolute top-3 right-3 px-2 py-0.5 rounded bg-black/60 text-[10px] text-white/90 z-10">
            修改后
          </div>
        </div>

        <CompareControls
          compareMode={compareMode}
          setCompareMode={setCompareMode}
          zoom={zoom}
          zoomBy={zoomBy}
          toggleZoom={toggleZoom}
        />
      </div>
    );
  }

  // ── Side-by-side comparison view ──
  if (compareMode === "sideBySide" && hasEdits) {
    return (
      <div className="w-full h-full flex overflow-hidden select-none relative">
        <div className="flex-1 flex items-center justify-center p-4 border-r border-nps-border relative">
          <div className="max-w-full max-h-full w-full h-full flex items-center justify-center">
            <ProcessedImage photo={photo} settings={adjust} bypass />
          </div>
          <div className="absolute top-3 left-3 px-2 py-0.5 rounded bg-black/60 text-[10px] text-white/90">
            原图
          </div>
        </div>
        <div className="flex-1 flex items-center justify-center p-4 relative">
          <div className="max-w-full max-h-full w-full h-full flex items-center justify-center">
            <ProcessedImage photo={photo} settings={adjust} />
          </div>
          <div className="absolute top-3 right-3 px-2 py-0.5 rounded bg-black/60 text-[10px] text-white/90">
            修改后
          </div>
        </div>
        <CompareControls
          compareMode={compareMode}
          setCompareMode={setCompareMode}
          zoom={zoom}
          zoomBy={zoomBy}
          toggleZoom={toggleZoom}
        />
      </div>
    );
  }

  // ── Normal view (with hold-to-compare overlay) ──
  return (
    <div
      ref={containerRef}
      className={`w-full h-full flex items-center justify-center overflow-hidden select-none relative ${
        zoom > 1 ? (dragging ? "cursor-grabbing" : "cursor-grab") : "cursor-zoom-in"
      }`}
      onMouseDown={onMouseDown}
      onMouseMove={onMouseMove}
      onMouseUp={endDrag}
      onMouseLeave={endDrag}
      onDoubleClick={toggleZoom}
    >
      <div
        className="w-full h-full flex items-center justify-center"
        style={{
          transform: `translate(${pan.x}px, ${pan.y}px)`,
          transformOrigin: "center center",
          transition: dragging ? "none" : "transform 0.12s ease-out",
        }}
      >
        <ProcessedImage photo={photo} settings={adjust} bypass={showOriginal} zoom={zoom} />
      </div>

      {/* Hold-to-compare indicator */}
      {holdCompare && hasEdits && (
        <div className="absolute top-3 left-1/2 -translate-x-1/2 px-3 py-1 rounded-full bg-black/70 text-[11px] text-white/90 z-10">
          显示原图 (松开 Y 返回)
        </div>
      )}

      {/* Zoom + Compare controls */}
      <CompareControls
        compareMode={compareMode}
        setCompareMode={setCompareMode}
        zoom={zoom}
        zoomBy={zoomBy}
        toggleZoom={toggleZoom}
        hasEdits={hasEdits}
      />
    </div>
  );
}

/** Bottom toolbar with zoom controls and compare mode toggle */
function CompareControls({
  compareMode,
  setCompareMode,
  zoom,
  zoomBy,
  toggleZoom,
  hasEdits,
}: {
  compareMode: "off" | "split" | "sideBySide";
  setCompareMode: (m: "off" | "split" | "sideBySide") => void;
  zoom: number;
  zoomBy: (f: number) => void;
  toggleZoom: () => void;
  hasEdits?: boolean;
}) {
  return (
    <div
      className="absolute bottom-4 left-1/2 -translate-x-1/2 flex items-center gap-1.5 px-3 py-1.5 rounded-full bg-nps-tertiary/90 border border-nps-border text-[11px] text-nps-text-dim backdrop-blur z-20"
      onMouseDown={(e) => e.stopPropagation()}
      onDoubleClick={(e) => e.stopPropagation()}
    >
      <button
        type="button"
        onClick={() => zoomBy(1 / 1.3)}
        className="hover:text-nps-text px-1.5 text-sm leading-none"
        title="缩小"
      >
        −
      </button>
      <button
        type="button"
        onClick={toggleZoom}
        className="hover:text-nps-text font-medium min-w-[46px] text-center"
        title="双击图片切换 100%/适应"
      >
        {Math.round(zoom * 100)}%
      </button>
      <button
        type="button"
        onClick={() => zoomBy(1.3)}
        className="hover:text-nps-text px-1.5 text-sm leading-none"
        title="放大"
      >
        ＋
      </button>

      {hasEdits && (
        <>
          <span className="w-px h-4 bg-nps-border mx-1" />
          <button
            type="button"
            onClick={() => setCompareMode(compareMode === "split" ? "off" : "split")}
            className={`px-2 py-0.5 rounded text-[10px] transition-all ${
              compareMode === "split"
                ? "bg-nps-accent text-white font-medium"
                : "hover:text-nps-text hover:bg-nps-hover"
            }`}
            title="分割对比 (\)"
          >
            ◫ 对比
          </button>
          <button
            type="button"
            onClick={() => setCompareMode(compareMode === "sideBySide" ? "off" : "sideBySide")}
            className={`px-2 py-0.5 rounded text-[10px] transition-all ${
              compareMode === "sideBySide"
                ? "bg-nps-accent text-white font-medium"
                : "hover:text-nps-text hover:bg-nps-hover"
            }`}
            title="并排对比"
          >
            ▥ 并排
          </button>
        </>
      )}

      <span className="text-nps-text-dim/70 ml-1 hidden sm:inline">
        Ctrl+滚轮缩放 · 双击切换 · Y按住看原图
      </span>
    </div>
  );
}