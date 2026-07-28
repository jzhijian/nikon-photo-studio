import type { Photo } from "../stores/libraryStore";
import { useLibraryStore } from "../stores/libraryStore";
import Thumbnail from "./Thumbnail";

interface Props {
  photo: Photo;
  /** Ordered paths of the current visible list (for Shift+click range select). */
  orderedPaths?: string[];
  /** Called on right-click with the mouse event. */
  onContextMenu?: (e: React.MouseEvent, photo: Photo) => void;
  /** Called on double-click. If not provided, falls back to openPreview. */
  onDoubleClick?: (photo: Photo) => void;
}

export default function PhotoCard({ photo, orderedPaths, onContextMenu, onDoubleClick }: Props) {
  const selectedPath = useLibraryStore((s) => s.selectedPath);
  const openPreview = useLibraryStore((s) => s.openPreview);
  const toggleMark = useLibraryStore((s) => s.toggleMark);
  const setRating = useLibraryStore((s) => s.setRating);
  const toggleSelected = useLibraryStore((s) => s.toggleSelected);
  const clickSelect = useLibraryStore((s) => s.clickSelect);

  const selected = selectedPath === photo.filePath;

  const handleClick = (e: React.MouseEvent) => {
    if (orderedPaths) {
      clickSelect(photo.filePath, e.shiftKey, e.ctrlKey || e.metaKey, orderedPaths);
    } else {
      if (e.ctrlKey || e.metaKey) toggleSelected(photo.filePath);
      else useLibraryStore.getState().select(photo.filePath);
    }
  };

  const handleDoubleClick = () => {
    if (onDoubleClick) {
      onDoubleClick(photo);
    } else {
      useLibraryStore.getState().select(photo.filePath);
      openPreview(photo.filePath);
    }
  };

  return (
    <div
      onClick={handleClick}
      onDoubleClick={handleDoubleClick}
      onContextMenu={(e) => {
        e.preventDefault();
        onContextMenu?.(e, photo);
      }}
      className={`thumb-item group ${selected ? "selected" : ""} ${photo.selected ? "ring-2 ring-nps-accent" : ""}`}
      title={`${photo.filename} · ${photo.format} · ${(photo.fileSize / 1024 / 1024).toFixed(1)} MB · 双击打开`}
    >
      <Thumbnail photo={photo} />

      <div className="thumb-label">{photo.filename}</div>

      <span
        className={`thumb-badge ${
          photo.format === "NEF"
            ? "bg-nps-accent/25 text-nps-accent"
            : "bg-nps-success/20 text-nps-success"
        }`}
      >
        {photo.format}
      </span>

      {/* Selection checkbox (top-left) */}
      <button
        type="button"
        onClick={(e) => {
          e.stopPropagation();
          toggleSelected(photo.filePath);
        }}
        className={`absolute top-1 left-1 w-5 h-5 rounded border flex items-center justify-center text-[11px] transition-all ${
          photo.selected
            ? "bg-nps-accent border-nps-accent text-nps-primary opacity-100"
            : "border-white/60 bg-black/30 text-transparent opacity-0 group-hover:opacity-100"
        }`}
        title="选择 (Ctrl+点击)"
      >
        ✓
      </button>

      {/* Mark star (top-right) */}
      <button
        type="button"
        onClick={(e) => {
          e.stopPropagation();
          toggleMark(photo.filePath);
        }}
        className={`absolute top-1 right-1 w-6 h-6 rounded flex items-center justify-center text-[13px] transition-all ${
          photo.marked
            ? "bg-nps-accent text-nps-primary opacity-100"
            : "bg-black/40 text-white opacity-0 group-hover:opacity-100"
        }`}
        title="标记 / 取消标记"
      >
        {photo.marked ? "★" : "☆"}
      </button>

      {/* Star rating (bottom overlay) */}
      <div
        className="absolute bottom-6 left-0 right-0 flex justify-center gap-[1px] opacity-0 group-hover:opacity-100 transition-opacity"
        onClick={(e) => e.stopPropagation()}
      >
        {[1, 2, 3, 4, 5].map((star) => (
          <button
            key={star}
            type="button"
            onClick={() => setRating(photo.filePath, photo.rating === star ? 0 : star)}
            className={`text-[11px] leading-none transition-transform hover:scale-125 ${
              star <= photo.rating ? "text-yellow-400" : "text-white/50"
            }`}
          >
            ★
          </button>
        ))}
      </div>

      {/* Show rating stars persistently if rated */}
      {photo.rating > 0 && (
        <div className="absolute bottom-6 left-0 right-0 flex justify-center gap-[1px] pointer-events-none group-hover:opacity-0 transition-opacity">
          {[1, 2, 3, 4, 5].map((star) => (
            <span key={star} className={`text-[10px] leading-none ${star <= photo.rating ? "text-yellow-400" : "text-white/30"}`}>
              ★
            </span>
          ))}
        </div>
      )}

      {/* Blur score badge */}
      {photo.blurScore >= 0 && (
        <span
          className={`absolute bottom-1 right-1 text-[9px] px-1 py-0.5 rounded font-mono ${
            photo.blurScore < 40
              ? "bg-red-500/80 text-white"
              : photo.blurScore < 60
              ? "bg-yellow-500/80 text-black"
              : "bg-green-500/80 text-white"
          }`}
        >
          {photo.blurScore}
        </span>
      )}
    </div>
  );
}