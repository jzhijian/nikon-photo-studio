import { useEffect, useState, useCallback } from "react";
import { useLibraryStore, selectFiltered, adjustToCssFilter } from "../stores/libraryStore";
import Thumbnail from "./Thumbnail";
import { useTranslation } from "react-i18next";

export default function Lightbox() {
  const { t } = useTranslation();
  const previewPath = useLibraryStore((s) => s.previewPath);
  const openPreview = useLibraryStore((s) => s.openPreview);
  const photos = useLibraryStore((s) => s.photos);
  const folder = useLibraryStore((s) => s.folder);
  const filter = useLibraryStore((s) => s.filter);
  const ratingOperator = useLibraryStore((s) => s.ratingOperator);
  const ratingValue = useLibraryStore((s) => s.ratingValue);
  const select = useLibraryStore((s) => s.select);
  const setRating = useLibraryStore((s) => s.setRating);
  const adjust = useLibraryStore((s) => s.adjust);
  const [loaded, setLoaded] = useState(false);

  const list = selectFiltered({ photos, folder, filter, ratingOperator, ratingValue });
  const idx = list.findIndex((p) => p.filePath === previewPath);
  const photo = idx >= 0 ? list[idx] : null;

  useEffect(() => {
    setLoaded(false);
    const timer = setTimeout(() => setLoaded(true), 80);
    return () => clearTimeout(timer);
  }, [previewPath]);

  const goNext = useCallback(() => {
    if (idx < list.length - 1) {
      const n = list[idx + 1];
      openPreview(n.filePath);
      select(n.filePath);
    }
  }, [idx, list, openPreview, select]);

  const goPrev = useCallback(() => {
    if (idx > 0) {
      const n = list[idx - 1];
      openPreview(n.filePath);
      select(n.filePath);
    }
  }, [idx, list, openPreview, select]);

  const close = useCallback(() => {
    openPreview(null);
  }, [openPreview]);

  // Keyboard: ESC close, arrows navigate, 0-5 rating
  useEffect(() => {
    if (!previewPath) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") close();
      else if (e.key === "ArrowRight") goNext();
      else if (e.key === "ArrowLeft") goPrev();
      else if (e.key >= "0" && e.key <= "5" && photo) {
        setRating(photo.filePath, parseInt(e.key));
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [previewPath, photo, close, goNext, goPrev, setRating]);

  if (!photo) return null;

  return (
    <div
      className="fixed inset-0 z-50 flex flex-col bg-black/95 animate-[fade-in_0.15s_ease-out]"
      onClick={close}
    >
      {/* Top bar: filename + close */}
      <div
        className="flex items-center justify-between px-5 py-3 shrink-0"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center gap-3 text-[13px] text-white/90">
          <span className="font-semibold">{photo.filename}</span>
          <span className="text-white/50">
            {photo.format}
            {photo.width > 0 ? ` · ${photo.width}×${photo.height}` : " · RAW"}
            {" · "}
            {(photo.fileSize / 1024 / 1024).toFixed(1)} MB
          </span>
          <span className="text-white/40">
            {idx + 1} / {list.length}
          </span>
        </div>
        <button
          type="button"
          onClick={close}
          className="w-8 h-8 rounded-full bg-white/10 hover:bg-white/25 text-white text-sm flex items-center justify-center transition-colors"
          title={t("lightbox.close") + " (Esc)"}
        >
          ✕
        </button>
      </div>

      {/* Main image area - centered, fills available space */}
      <div
        className="flex-1 flex items-center justify-center relative overflow-hidden px-16"
        onClick={(e) => e.stopPropagation()}
      >
        <div
          className="max-w-full max-h-full flex items-center justify-center"
          style={{ opacity: loaded ? 1 : 0.3, transition: "opacity 0.2s ease-out" }}
        >
          <Thumbnail photo={photo} big cssFilter={adjustToCssFilter(adjust)} />
        </div>

        {/* Nav arrows */}
        {idx > 0 && (
          <button
            type="button"
            onClick={goPrev}
            className="absolute left-4 top-1/2 -translate-y-1/2 w-12 h-12 rounded-full bg-white/10 hover:bg-white/25 text-white text-2xl flex items-center justify-center transition-colors"
            title={t("lightbox.prev") + " (←)"}
          >
            ‹
          </button>
        )}
        {idx < list.length - 1 && (
          <button
            type="button"
            onClick={goNext}
            className="absolute right-4 top-1/2 -translate-y-1/2 w-12 h-12 rounded-full bg-white/10 hover:bg-white/25 text-white text-2xl flex items-center justify-center transition-colors"
            title={t("lightbox.next") + " (→)"}
          >
            ›
          </button>
        )}
      </div>

      {/* Bottom bar: star rating + hints */}
      <div
        className="flex items-center justify-center gap-4 py-4 shrink-0"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center gap-1">
          {[1, 2, 3, 4, 5].map((star) => (
            <button
              key={star}
              type="button"
              onClick={() => setRating(photo.filePath, photo.rating === star ? 0 : star)}
              className={`text-2xl transition-transform hover:scale-125 ${
                star <= photo.rating ? "text-yellow-400" : "text-white/30 hover:text-white/60"
              }`}
              title={`${star} ★ (按数字键 ${star})`}
            >
              ★
            </button>
          ))}
          <span className="ml-3 text-[11px] text-white/40">
            按 0-5 键评分 · ←→ 切换 · Esc 关闭
          </span>
        </div>
      </div>
    </div>
  );
}
