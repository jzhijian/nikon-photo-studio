import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { useAppStore } from "../stores/appStore";
import { useLibraryStore } from "../stores/libraryStore";
import { toast } from "../stores/toastStore";

type ExportFormat = "jpeg" | "png" | "tiff";
type ResizeMode = "original" | "long-edge" | "dimensions";

interface ExportResult {
  outputPath: string;
  width: number;
  height: number;
  bitDepth: number;
  format: string;
  fileSize: number;
}

function parentDirectory(path: string) {
  const split = Math.max(path.lastIndexOf("\\"), path.lastIndexOf("/"));
  return split >= 0 ? path.slice(0, split) : ".";
}

function NumberField({ value, min, max, onChange, disabled = false }: {
  value: number; min: number; max: number; disabled?: boolean; onChange: (value: number) => void;
}) {
  return (
    <input
      type="number"
      value={value}
      min={min}
      max={max}
      disabled={disabled}
      onChange={(event) => onChange(Math.min(max, Math.max(min, Number(event.target.value) || min)))}
      className="w-full rounded border border-nps-border bg-nps-primary px-2 py-1.5 text-xs outline-none focus:border-nps-accent disabled:opacity-40"
    />
  );
}

export default function ExportDialog() {
  const isOpen = useAppStore((s) => s.exportOpen);
  const close = useAppStore((s) => s.closeExport);
  const language = useAppStore((s) => s.language);
  const photos = useLibraryStore((s) => s.photos);
  const selectedPath = useLibraryStore((s) => s.selectedPath);
  const developPath = useLibraryStore((s) => s.developPath);
  const adjust = useLibraryStore((s) => s.adjust);
  const photo = photos.find((item) => item.filePath === (developPath || selectedPath)) ?? null;
  const zh = language === "zh-CN";

  const [format, setFormat] = useState<ExportFormat>("jpeg");
  const [bitDepth, setBitDepth] = useState<8 | 16>(8);
  const [quality, setQuality] = useState(92);
  const [resizeMode, setResizeMode] = useState<ResizeMode>("original");
  const [width, setWidth] = useState(6000);
  const [height, setHeight] = useState(4000);
  const [longEdge, setLongEdge] = useState(6000);
  const [noUpscale, setNoUpscale] = useState(true);
  const [dpi, setDpi] = useState(300);
  const [outputDir, setOutputDir] = useState("");
  const [suffix, setSuffix] = useState("-export");
  const [overwrite, setOverwrite] = useState(false);
  const [exporting, setExporting] = useState(false);

  useEffect(() => {
    if (!isOpen || !photo) return;
    setOutputDir((current) => current || parentDirectory(photo.filePath));
    if (photo.width > 0 && photo.height > 0) {
      setWidth(photo.width);
      setHeight(photo.height);
      setLongEdge(Math.max(photo.width, photo.height));
    }
  }, [isOpen, photo?.filePath]);

  useEffect(() => {
    if (format === "jpeg") setBitDepth(8);
  }, [format]);

  useEffect(() => {
    if (!isOpen) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape" && !exporting) close();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [isOpen, exporting, close]);

  const estimated = useMemo(() => {
    if (!photo || photo.width <= 0 || photo.height <= 0) return { width, height };
    const sourceW = photo.width;
    const sourceH = photo.height;
    let scale = 1;
    if (resizeMode === "long-edge") scale = longEdge / Math.max(sourceW, sourceH);
    if (resizeMode === "dimensions") scale = Math.min(width / sourceW, height / sourceH);
    if (resizeMode === "original" || (noUpscale && scale > 1)) scale = 1;
    return { width: Math.max(1, Math.round(sourceW * scale)), height: Math.max(1, Math.round(sourceH * scale)) };
  }, [photo, resizeMode, width, height, longEdge, noUpscale]);

  if (!isOpen || !photo) return null;

  const chooseDirectory = async () => {
    const chosen = await open({ directory: true, multiple: false, defaultPath: outputDir });
    if (typeof chosen === "string") setOutputDir(chosen);
  };

  const runExport = async () => {
    if (!outputDir) return;
    setExporting(true);
    try {
      const result = await invoke<ExportResult>("export_photo", {
        photoPath: photo.filePath,
        adjust,
        options: { format, bitDepth, quality, resizeMode, width, height, longEdge, noUpscale, dpi, outputDir, suffix, overwrite },
      });
      const mb = (result.fileSize / 1024 / 1024).toFixed(1);
      toast(zh
        ? `导出完成：${result.width}×${result.height} · ${result.bitDepth} 位 · ${mb} MB`
        : `Exported: ${result.width}×${result.height} · ${result.bitDepth}-bit · ${mb} MB`, "success");
      close();
    } catch (error) {
      toast(`${zh ? "导出失败" : "Export failed"}: ${String(error)}`, "error");
    } finally {
      setExporting(false);
    }
  };

  const label = (cn: string, en: string) => zh ? cn : en;
  return (
    <div className="fixed inset-0 z-[100] flex items-center justify-center bg-black/70 p-6" onMouseDown={(e) => { if (e.target === e.currentTarget && !exporting) close(); }}>
      <div className="w-full max-w-3xl max-h-[92vh] overflow-y-auto rounded-xl border border-nps-border bg-nps-secondary shadow-2xl">
        <div className="sticky top-0 z-10 flex items-center border-b border-nps-border bg-nps-secondary px-5 py-3">
          <div>
            <h2 className="text-base font-semibold">{label("导出照片", "Export photo")}</h2>
            <p className="mt-0.5 max-w-[600px] truncate text-[11px] text-nps-text-dim">{photo.filename}</p>
          </div>
          <button onClick={close} disabled={exporting} className="ml-auto rounded px-2 py-1 text-lg text-nps-text-dim hover:bg-nps-hover">×</button>
        </div>

        <div className="grid gap-5 p-5 md:grid-cols-2">
          <section className="space-y-3 rounded-lg border border-nps-border bg-nps-primary/40 p-4">
            <h3 className="text-xs font-semibold text-nps-accent">{label("文件设置", "File settings")}</h3>
            <div>
              <div className="mb-1.5 text-[11px] text-nps-text-dim">{label("图像格式", "Image format")}</div>
              <div className="grid grid-cols-3 gap-2">
                {(["jpeg", "png", "tiff"] as ExportFormat[]).map((value) => (
                  <button key={value} onClick={() => setFormat(value)} className={`rounded border py-2 text-xs font-semibold uppercase ${format === value ? "border-nps-accent bg-nps-accent text-nps-primary" : "border-nps-border bg-nps-tertiary hover:bg-nps-hover"}`}>{value}</button>
                ))}
              </div>
            </div>
            {format === "jpeg" && <div>
              <div className="mb-1 flex justify-between text-[11px]"><span className="text-nps-text-dim">{label("JPEG 品质", "JPEG quality")}</span><span>{quality}</span></div>
              <input type="range" min={1} max={100} value={quality} onChange={(e) => setQuality(Number(e.target.value))} className="w-full accent-[var(--color-accent)]" />
            </div>}
            <div>
              <div className="mb-1.5 text-[11px] text-nps-text-dim">{label("位深度", "Bit depth")}</div>
              <div className="grid grid-cols-2 gap-2">
                {[8, 16].map((value) => <button key={value} disabled={format === "jpeg" && value === 16} onClick={() => setBitDepth(value as 8 | 16)} className={`rounded border py-1.5 text-xs disabled:cursor-not-allowed disabled:opacity-35 ${bitDepth === value ? "border-nps-accent text-nps-accent" : "border-nps-border bg-nps-tertiary"}`}>{value} bit</button>)}
              </div>
              <p className="mt-1.5 text-[10px] leading-4 text-nps-text-dim">{format === "jpeg" ? label("JPEG 标准为 8 位；需要更高色阶请选 16 位 PNG/TIFF。", "JPEG is 8-bit; choose 16-bit PNG/TIFF for higher tonal precision.") : label("16 位更适合后续精修，文件也会更大。", "16-bit is better for further editing and creates larger files.")}</p>
            </div>
          </section>

          <section className="space-y-3 rounded-lg border border-nps-border bg-nps-primary/40 p-4">
            <h3 className="text-xs font-semibold text-nps-accent">{label("调整图像大小", "Image sizing")}</h3>
            <div className="grid grid-cols-3 gap-1.5">
              {([ ["original", "原始尺寸", "Original"], ["long-edge", "最长边", "Long edge"], ["dimensions", "限定尺寸", "Fit dimensions"] ] as const).map(([value, cn, en]) => (
                <button key={value} onClick={() => setResizeMode(value)} className={`rounded border px-1 py-2 text-[11px] ${resizeMode === value ? "border-nps-accent bg-nps-accent/15 text-nps-accent" : "border-nps-border bg-nps-tertiary"}`}>{label(cn, en)}</button>
              ))}
            </div>
            {resizeMode === "long-edge" && <label className="block"><span className="mb-1 block text-[11px] text-nps-text-dim">{label("最长边（像素）", "Long edge (pixels)")}</span><NumberField value={longEdge} min={1} max={32768} onChange={setLongEdge} /></label>}
            {resizeMode === "dimensions" && <div className="grid grid-cols-2 gap-2">
              <label><span className="mb-1 block text-[11px] text-nps-text-dim">{label("最大宽度", "Max width")}</span><NumberField value={width} min={1} max={32768} onChange={setWidth} /></label>
              <label><span className="mb-1 block text-[11px] text-nps-text-dim">{label("最大高度", "Max height")}</span><NumberField value={height} min={1} max={32768} onChange={setHeight} /></label>
            </div>}
            <label className="flex items-center gap-2 text-xs"><input type="checkbox" checked={noUpscale} onChange={(e) => setNoUpscale(e.target.checked)} className="accent-[var(--color-accent)]" />{label("不放大原图", "Do not enlarge")}</label>
            <div className="grid grid-cols-2 items-end gap-3">
              <label><span className="mb-1 block text-[11px] text-nps-text-dim">{label("打印 DPI（尺寸参考）", "Print DPI (size reference)")}</span><NumberField value={dpi} min={72} max={1200} onChange={setDpi} /></label>
              <div className="rounded border border-nps-border bg-nps-tertiary px-2 py-1.5 text-center text-xs">{estimated.width} × {estimated.height} px</div>
            </div>
            <p className="text-[10px] leading-4 text-nps-text-dim">{label("屏幕清晰度由像素尺寸决定；DPI 主要用于打印尺寸标记。缩小采用 Lanczos 高质量重采样。", "On-screen detail is determined by pixel dimensions; DPI mainly describes print size. Downsizing uses high-quality Lanczos resampling.")}</p>
          </section>

          <section className="space-y-3 rounded-lg border border-nps-border bg-nps-primary/40 p-4 md:col-span-2">
            <h3 className="text-xs font-semibold text-nps-accent">{label("导出位置与文件名", "Destination and filename")}</h3>
            <div className="flex gap-2"><input value={outputDir} onChange={(e) => setOutputDir(e.target.value)} className="min-w-0 flex-1 rounded border border-nps-border bg-nps-primary px-2 py-1.5 text-xs outline-none focus:border-nps-accent" /><button onClick={chooseDirectory} className="rounded border border-nps-border bg-nps-tertiary px-3 text-xs hover:bg-nps-hover">{label("选择…", "Choose…")}</button></div>
            <div className="grid gap-3 sm:grid-cols-2">
              <label><span className="mb-1 block text-[11px] text-nps-text-dim">{label("文件名后缀", "Filename suffix")}</span><input value={suffix} maxLength={80} onChange={(e) => setSuffix(e.target.value)} className="w-full rounded border border-nps-border bg-nps-primary px-2 py-1.5 text-xs outline-none focus:border-nps-accent" /></label>
              <label className="flex items-end gap-2 pb-1.5 text-xs"><input type="checkbox" checked={overwrite} onChange={(e) => setOverwrite(e.target.checked)} className="accent-[var(--color-accent)]" />{label("覆盖同名文件（否则自动编号）", "Overwrite existing file (otherwise auto-number)")}</label>
            </div>
          </section>
        </div>

        <div className="sticky bottom-0 flex items-center border-t border-nps-border bg-nps-secondary px-5 py-3">
          <span className="text-[10px] text-nps-text-dim">{label("RAW 将从全尺寸传感器数据渲染，并应用当前调节。", "RAW is rendered from full-resolution sensor data with current adjustments.")}</span>
          <div className="ml-auto flex gap-2"><button onClick={close} disabled={exporting} className="rounded border border-nps-border bg-nps-tertiary px-4 py-2 text-xs hover:bg-nps-hover disabled:opacity-50">{label("取消", "Cancel")}</button><button onClick={runExport} disabled={exporting || !outputDir} className="min-w-28 rounded bg-nps-accent px-4 py-2 text-xs font-semibold text-nps-primary hover:bg-nps-accent-hover disabled:opacity-50">{exporting ? label("正在导出…", "Exporting…") : label("导出", "Export")}</button></div>
        </div>
      </div>
    </div>
  );
}
