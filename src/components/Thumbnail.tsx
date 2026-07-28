import { useEffect, useState } from "react";
import { invoke, convertFileSrc } from "@tauri-apps/api/core";
import type { Photo } from "../stores/libraryStore";

const cache = new Map<string, string>();
const inflight = new Map<string, Promise<string>>();

/* -- Single-call concurrency limiter --
   Used for the develop full-preview and as a per-photo fallback so a failed
   batch never floods the backend. Keeps at most MAX_CONCURRENT individual
   renders in flight. */
const MAX_CONCURRENT = 8;
let active = 0;
const queue: Array<() => void> = [];
function pump(): void {
  while (active < MAX_CONCURRENT && queue.length > 0) {
    const start = queue.shift();
    if (start) start();
  }
}
function limited<T>(fn: () => Promise<T>): Promise<T> {
  return new Promise<T>((resolve, reject) => {
    const start = () => {
      active++;
      fn().then(
        (v) => { active--; pump(); resolve(v); },
        (e) => { active--; pump(); reject(e); }
      );
    };
    if (active < MAX_CONCURRENT) start();
    else queue.push(start);
  });
}

function singleThumb(path: string, size: number): Promise<string> {
  return limited(() => invoke<string>("make_thumbnail", { path, maxSize: size }));
}

/* -- Request coalescing / batching --
   A grid can mount hundreds of thumbnail components in a single render commit
   (e.g. opening a folder of 456 photos), and each calls fetchThumb
   synchronously. Instead of firing hundreds of separate IPC calls, we collect
   every request made within the current tick and flush them as a handful of
   backend-parallel `make_thumbnails_batch` calls (grouped by size, chunked).
   This turns N IPC round-trips into roughly N/64, with the Rust side rendering
   each chunk across all CPU cores. Requests arrive in DOM (visual) order, so
   the first chunk covers the visible photos and they appear first. Works
   identically on import, on app restart from the database, and on filter or
   folder changes -- no race with separately-mounted cards. */
interface PendingRequest {
  path: string;
  size: number;
  key: string;
  resolve: (url: string) => void;
  reject: (err: unknown) => void;
}
const BATCH_CHUNK = 64;
let pending: PendingRequest[] = [];
let flushScheduled = false;
let flushChain: Promise<void> = Promise.resolve();

function scheduleFlush(): void {
  if (flushScheduled) return;
  flushScheduled = true;
  setTimeout(() => {
    flushScheduled = false;
    flushChain = flushChain.then(flushOnce).catch(() => {});
  }, 0);
}

function settle(r: PendingRequest, urlPromise: Promise<string>): void {
  urlPromise.then(
    (url) => { cache.set(r.key, url); inflight.delete(r.key); r.resolve(url); },
    (err) => { inflight.delete(r.key); r.reject(err); }
  );
}

async function flushOnce(): Promise<void> {
  const batch = pending;
  pending = [];
  if (batch.length === 0) return;
  // Group by requested size so each backend call renders uniform thumbnails.
  const bySize = new Map<number, PendingRequest[]>();
  for (const r of batch) {
    const arr = bySize.get(r.size);
    if (arr) arr.push(r);
    else bySize.set(r.size, [r]);
  }
  for (const [size, group] of bySize) {
    for (let i = 0; i < group.length; i += BATCH_CHUNK) {
      const chunk = group.slice(i, i + BATCH_CHUNK);
      const paths = chunk.map((r) => r.path);
      try {
        const res = await invoke<Array<{ path: string; url: string }>>(
          "make_thumbnails_batch",
          { paths, maxSize: size }
        );
        const urlByPath = new Map<string, string>();
        // Backend returns the cached JPEG file path; serve it via the asset
        // protocol so the bytes stream from disk instead of riding IPC as base64.
        for (const e of res) if (e.url) urlByPath.set(e.path, convertFileSrc(e.url));
        for (const r of chunk) {
          const url = urlByPath.get(r.path);
          if (url) {
            cache.set(r.key, url);
            inflight.delete(r.key);
            r.resolve(url);
          } else {
            // Batch produced nothing for this file -> single render fallback.
            settle(r, singleThumb(r.path, size));
          }
        }
      } catch {
        // Whole batch failed -> fall back to single renders (rate-limited).
        for (const r of chunk) settle(r, singleThumb(r.path, size));
      }
    }
  }
}

export function fetchThumb(path: string, size: number): Promise<string> {
  const key = `${path}@${size}`;
  const hit = cache.get(key);
  if (hit) return Promise.resolve(hit);
  const pend = inflight.get(key);
  if (pend) return pend;
  const p = new Promise<string>((resolve, reject) => {
    pending.push({ path, size, key, resolve, reject });
  });
  inflight.set(key, p);
  scheduleFlush();
  return p;
}

/** Fetch a display-sized high-quality preview for the develop viewer. */
export function fetchFullPreview(path: string, maxSize = 4096): Promise<string> {
  const key = `${path}@full:${maxSize}`;
  const hit = cache.get(key);
  if (hit) return Promise.resolve(hit);
  const pend = inflight.get(key);
  if (pend) return pend;
  const p = limited(() => invoke<string>("make_full_preview", { path, maxSize }))
    .then((url) => {
      // Develop previews normally arrive inline so they remain readable by the
      // canvas pixel-processing pipeline. Keep path conversion for backwards
      // compatibility with older backends/cached builds.
      const final = url.startsWith("data:") ? url : convertFileSrc(url);
      cache.set(key, final);
      inflight.delete(key);
      return final;
    })
    .catch((e) => { inflight.delete(key); throw e; });
  inflight.set(key, p);
  return p;
}

/** Fetch a fast low-resolution preview (the camera's embedded JPEG for NEF,
    a downscaled render for raster files). Shown instantly in the develop
    viewer while the full-resolution develop renders in the background. */
export function fetchFastPreview(path: string): Promise<string> {
  const key = `${path}@fast`;
  const hit = cache.get(key);
  if (hit) return Promise.resolve(hit);
  const pend = inflight.get(key);
  if (pend) return pend;
  const p = limited(() => invoke<string>("make_preview_fast", { path }))
    .then((url) => {
      const final = url.startsWith("data:") ? url : convertFileSrc(url);
      cache.set(key, final);
      inflight.delete(key);
      return final;
    })
    .catch((e) => { inflight.delete(key); throw e; });
  inflight.set(key, p);
  return p;
}
/** Warm thumbnails for many photos. Requests coalesce with any concurrently
    mounting grid cards (via the inflight map) and flush as backend-parallel
    batches, so this never duplicates work the cards are already doing. */
export function prefetchThumbs(paths: string[], size: number): void {
  for (const p of paths) {
    fetchThumb(p, size).catch(() => {});
  }
}
function NefPlaceholder({ photo, big }: { photo: Photo; big?: boolean }) {
  return (
    <div className="w-full h-full flex flex-col items-center justify-center gap-1 bg-[var(--thumb-bg)]">
      <div className={big ? "text-4xl" : "text-2xl"}>📷</div>
      <div className={`font-bold tracking-widest text-nps-accent ${big ? "text-lg" : "text-[10px]"}`}>{photo.format}</div>
      {big && <div className="text-[11px] text-nps-text-dim">RAW · {photo.filename}</div>}
    </div>
  );
}

interface ThumbnailProps {
  photo: Photo;
  big?: boolean;
  /** CSS filter string to apply (e.g. from adjustToCssFilter) */
  cssFilter?: string;
  /** Override the rendered max pixel size (develop viewer uses a larger value for sharpness) */
  maxSize?: number;
  /** Use full-resolution preview (develop viewer) */
  fullRes?: boolean;
}

export default function Thumbnail({ photo, big, cssFilter, maxSize, fullRes }: ThumbnailProps) {
  const size = maxSize ?? (big ? 1600 : 360);
  const cacheKey = fullRes ? `${photo.filePath}@full:${size}` : `${photo.filePath}@${size}`;
  const [url, setUrl] = useState<string | null>(cache.get(cacheKey) ?? null);
  const [failed, setFailed] = useState(false);
  // For big images: load small first, then upgrade
  const [smallUrl, setSmallUrl] = useState<string | null>(big ? (cache.get(`${photo.filePath}@360`) ?? null) : null);
  // True once we've fallen back from a failed asset URL to a base64 render, so
  // the onError handler can't loop.
  const [fellBack, setFellBack] = useState(false);

  useEffect(() => {
    setFellBack(false);
    if (!photo.previewable) return;
    const cached = cache.get(cacheKey);
    if (cached) { setUrl(cached); return; }
    let alive = true;
    setFailed(false);
    // For big/fullRes: load small preview first for fast display
    if (big || fullRes) {
      fetchThumb(photo.filePath, 360)
        .then((u) => { if (alive) setSmallUrl(u); })
        .catch(() => {});
    }
    const fetcher = fullRes
      ? fetchFullPreview(photo.filePath, size)
      : fetchThumb(photo.filePath, size);
    fetcher
      .then((u) => alive && setUrl(u))
      .catch(() => alive && setFailed(true));
    return () => { alive = false; };
  }, [photo.filePath, photo.previewable, cacheKey, size, big, fullRes]);

  if (!photo.previewable || failed) {
    return <NefPlaceholder photo={photo} big={big} />;
  }

  const displayUrl = url ?? smallUrl;
  if (!displayUrl) {
    return (
      <div className="w-full h-full flex items-center justify-center bg-[var(--thumb-bg)]">
        <div className="w-5 h-5 border-2 border-nps-accent border-t-transparent rounded-full animate-spin" />
      </div>
    );
  }

  return (
    <img
      src={displayUrl}
      alt={photo.filename}
      loading="lazy"
      draggable={false}
      onError={() => {
        // Asset-protocol URL failed to load (e.g. scope/protocol unavailable):
        // fall back to a base64 render once. content-visibility keeps this
        // bounded to roughly the visible images, never the whole folder.
        if (fellBack) return;
        setFellBack(true);
        singleThumb(photo.filePath, size)
          .then((u) => { cache.set(cacheKey, u); setUrl(u); })
          .catch(() => setFailed(true));
      }}
      className={`block ${big ? "w-full h-full object-contain" : "w-full h-full object-cover"}`}
      style={cssFilter && cssFilter !== "none" ? { filter: cssFilter } : undefined}
    />
  );
}
