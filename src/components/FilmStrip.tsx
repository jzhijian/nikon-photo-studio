import { useEffect, useRef } from "react";
import { useLibraryStore, selectFiltered } from "../stores/libraryStore";
import Thumbnail from "./Thumbnail";
import { useAppStore } from "../stores/appStore";
import { useOrganizeStore } from "../stores/organizeStore";

export default function FilmStrip() {
  const photos = useLibraryStore((s) => s.photos);
  const folder = useLibraryStore((s) => s.folder);
  const filter = useLibraryStore((s) => s.filter);
  const ratingOperator = useLibraryStore((s) => s.ratingOperator);
  const ratingValue = useLibraryStore((s) => s.ratingValue);
  const selectedPath = useLibraryStore((s) => s.selectedPath);
  const select = useLibraryStore((s) => s.select);
  const openPreview = useLibraryStore((s) => s.openPreview);
  const setRating = useLibraryStore((s) => s.setRating);
  const developPath = useLibraryStore((s) => s.developPath);
  const setDevelopPath = useLibraryStore((s) => s.setDevelopPath);
  const activeModule = useAppStore((s) => s.activeModule);
  const organizeCategory = useOrganizeStore((s) => s.category);
  const organizeThreshold = useOrganizeStore((s) => s.threshold);

  const scrollRef = useRef<HTMLDivElement>(null);

  const list = activeModule === "organize"
    ? photos.filter((photo) =>
        organizeCategory === "blurry"
          ? photo.blurScore >= 0 && photo.blurScore < organizeThreshold
          : photo.category === organizeCategory
      )
    : selectFiltered({ photos, folder, filter, ratingOperator, ratingValue });

  // Vertical mouse-wheel over the strip => horizontal scroll (easier browsing).
  useEffect(() => {
    const el = scrollRef.current;
    if (!el) return;
    const onWheel = (e: WheelEvent) => {
      const canScrollH = el.scrollWidth - el.clientWidth > 1;
      if (!canScrollH) return;
      // Let native horizontal input (trackpad two-finger / shift+wheel) through.
      if (Math.abs(e.deltaX) > Math.abs(e.deltaY)) return;
      e.preventDefault();
      // Normalise the delta to pixels: mouse wheels report "line" deltas
      // (deltaMode === 1) which would otherwise crawl. One notch ~= one
      // thumbnail (~64px) so wheel browsing/positioning feels natural.
      const unit = e.deltaMode === 1 ? 64 : e.deltaMode === 2 ? el.clientWidth : 1;
      el.scrollLeft += e.deltaY * unit;
    };
    el.addEventListener("wheel", onWheel, { passive: false });
    return () => el.removeEventListener("wheel", onWheel);
  }, [list.length]);

  // Keep the active thumbnail in view (auto-positioning).
  useEffect(() => {
    const el = scrollRef.current;
    const active = selectedPath ?? developPath;
    if (!el || !active) return;
    let target: HTMLElement | undefined;
    for (const child of Array.from(el.children)) {
      if ((child as HTMLElement).dataset.path === active) {
        target = child as HTMLElement;
        break;
      }
    }
    if (!target) return;
    const elRect = el.getBoundingClientRect();
    const tRect = target.getBoundingClientRect();
    if (tRect.left < elRect.left || tRect.right > elRect.right) {
      target.scrollIntoView({ inline: "center", block: "nearest", behavior: "smooth" });
    }
  }, [selectedPath, developPath, list.length]);

  if (list.length === 0) {
    return (
      <div className="h-[72px] bg-nps-secondary border-t border-nps-border flex items-center justify-center shrink-0">
        <span className="text-[11px] text-nps-text-dim">—</span>
      </div>
    );
  }

  return (
    <div
      ref={scrollRef}
      className="filmstrip-scroll h-[72px] bg-nps-secondary border-t border-nps-border flex items-center px-3 gap-1.5 overflow-x-auto shrink-0"
    >
      {list.map((p) => (
        <div
          key={p.filePath}
          data-path={p.filePath}
          onClick={() => { if (developPath) setDevelopPath(p.filePath); else select(p.filePath); }}
          onDoubleClick={() => {
            if (developPath) { setDevelopPath(p.filePath); return; }
            select(p.filePath);
            openPreview(p.filePath);
          }}
          className={`filmstrip-thumb group relative ${selectedPath === p.filePath ? "selected" : ""}`}
          title={`${p.filename} (双击放大)`}
        >
          <Thumbnail photo={p} />
          {/* Rating stars - clickable */}
          <div
            className="absolute bottom-0 left-0 right-0 flex justify-center gap-[1px] bg-black/60 opacity-0 group-hover:opacity-100 transition-opacity"
            onClick={(e) => e.stopPropagation()}
          >
            {[1, 2, 3, 4, 5].map((star) => (
              <button
                key={star}
                type="button"
                onClick={() => setRating(p.filePath, p.rating === star ? 0 : star)}
                className={`text-[7px] leading-tight transition-colors ${
                  star <= p.rating ? "text-yellow-400" : "text-white/40 hover:text-white/70"
                }`}
              >
                ★
              </button>
            ))}
          </div>
          {/* Persistent rating indicator */}
          {p.rating > 0 && (
            <span className="absolute bottom-0 left-0 right-0 text-center text-[7px] text-yellow-400 leading-none bg-black/50 group-hover:opacity-0 transition-opacity pointer-events-none">
              {"★".repeat(p.rating)}
            </span>
          )}
        </div>
      ))}
    </div>
  );
}
