import { useEffect, useState, useRef, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { useAppStore } from "../stores/appStore";
import { useLibraryStore, HSL_CHANNELS } from "../stores/libraryStore";
import type { HslChannelName } from "../stores/libraryStore";
import { usePresetStore } from "../stores/presetStore";
import { useTransferStore } from "../stores/transferStore";
import { toast } from "../stores/toastStore";
import { useTranslation } from "react-i18next";

interface ExifInfo {
  camera_model: string | null;
  lens: string | null;
  aperture: number | null;
  shutter_speed: string | null;
  iso: number | null;
  focal_length: number | null;
  white_balance: string | null;
  date_taken: string | null;
  width: number;
  height: number;
}

export default function RightPanel() {
  const activeModule = useAppStore((s) => s.activeModule);

  return (
    <aside className="w-72 bg-nps-secondary border-l border-nps-border overflow-y-auto shrink-0 p-3">
      {activeModule === "library" && <LibraryRight />}
      {activeModule === "organize" && <OrganizeRight />}
      {activeModule === "presets" && <PresetManagerRight />}
      {activeModule === "transfer" && <TransferRight />}
    </aside>
  );
}

function SectionTitle({ children }: { children: React.ReactNode }) {
  return (
    <div className="text-[11px] uppercase tracking-wide text-nps-text-dim font-semibold mb-2">
      {children}
    </div>
  );
}

function ExifRow({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex justify-between text-xs py-1 border-b border-nps-border gap-2">
      <span className="text-nps-text-dim shrink-0">{label}</span>
      <span className="truncate text-right" title={value}>{value}</span>
    </div>
  );
}

function EmptyHint({ text }: { text: string }) {
  return (
    <div className="text-[12px] text-nps-text-dim text-center py-8 px-2 leading-relaxed">
      {text}
    </div>
  );
}

/** Interactive slider: click or drag on the track to change the value. */
function Slider({
  label,
  value,
  min,
  max,
  unit,
  onChange,
}: {
  label: string;
  value: number;
  min: number;
  max: number;
  unit?: string;
  onChange: (v: number) => void;
}) {
  const pct = ((value - min) / (max - min)) * 100;

  const update = (clientX: number, el: HTMLDivElement) => {
    const rect = el.getBoundingClientRect();
    const ratio = Math.min(1, Math.max(0, (clientX - rect.left) / rect.width));
    onChange(Math.round(min + ratio * (max - min)));
  };

  return (
    <div className="mb-2.5">
      <div className="flex justify-between text-[11px] text-nps-text-dim mb-1">
        <span>{label}</span>
        <span className="text-nps-text tabular-nums">
          {value > 0 && max > 0 && min < 0 ? `+${value}` : value}
          {unit ?? ""}
        </span>
      </div>
      <div
        className="h-1.5 bg-nps-tertiary rounded relative cursor-pointer group"
        onPointerDown={(e) => {
          (e.currentTarget as HTMLDivElement).setPointerCapture?.(e.pointerId);
          update(e.clientX, e.currentTarget);
        }}
        onPointerMove={(e) => {
          if (e.buttons === 1) update(e.clientX, e.currentTarget);
        }}
      >
        <div className="h-full bg-nps-accent rounded" style={{ width: `${pct}%` }} />
        <div
          className="w-3 h-3 bg-nps-accent rounded-full absolute top-1/2 -translate-y-1/2 -translate-x-1/2 group-hover:scale-125 transition-transform"
          style={{ left: `${pct}%` }}
        />
      </div>
    </div>
  );
}

// ---------------- Library (Develop Mode) ----------------
/* ─────────────────────────── Tone Curve Editor ─────────────────────────── */
function smoothPath(pts: [number, number][]): string {
  if (pts.length === 0) return "";
  if (pts.length === 1) return `M ${pts[0][0] * 100} ${100 - pts[0][1] * 100}`;
  const P = pts.map(([x, y]) => [x * 100, 100 - y * 100] as [number, number]);
  let d = `M ${P[0][0].toFixed(1)} ${P[0][1].toFixed(1)}`;
  for (let i = 0; i < P.length - 1; i++) {
    const p0 = P[Math.max(0, i - 1)];
    const p1 = P[i];
    const p2 = P[i + 1];
    const p3 = P[Math.min(P.length - 1, i + 2)];
    const c1x = p1[0] + (p2[0] - p0[0]) / 6;
    const c1y = p1[1] + (p2[1] - p0[1]) / 6;
    const c2x = p2[0] - (p3[0] - p1[0]) / 6;
    const c2y = p2[1] - (p3[1] - p1[1]) / 6;
    d += ` C ${c1x.toFixed(1)} ${c1y.toFixed(1)}, ${c2x.toFixed(1)} ${c2y.toFixed(1)}, ${p2[0].toFixed(1)} ${p2[1].toFixed(1)}`;
  }
  return d;
}

function ToneCurveEditor() {
  const { t } = useTranslation();
  const curve = useLibraryStore((s) => s.adjust.curve);
  const setCurve = useLibraryStore((s) => s.setCurve);
  const svgRef = useRef<SVGSVGElement>(null);
  const dragIdx = useRef<number>(-1);

  const pts = [...curve.points].sort((a, b) => a[0] - b[0]);

  const toNorm = useCallback((e: { clientX: number; clientY: number }): [number, number] => {
    const rect = svgRef.current!.getBoundingClientRect();
    const x = Math.min(1, Math.max(0, (e.clientX - rect.left) / rect.width));
    const y = Math.min(1, Math.max(0, 1 - (e.clientY - rect.top) / rect.height));
    return [x, y];
  }, []);

  const onPointDown = (idx: number) => (e: React.PointerEvent) => {
    e.stopPropagation();
    dragIdx.current = idx;
    svgRef.current?.setPointerCapture?.(e.pointerId);
  };

  const onMove = (e: React.PointerEvent) => {
    if (dragIdx.current < 0) return;
    const [x, y] = toNorm(e);
    const next = pts.map((p, i) => {
      if (i !== dragIdx.current) return p;
      // Pin endpoints horizontally
      const nx = i === 0 ? 0 : i === pts.length - 1 ? 1 : x;
      return [nx, y] as [number, number];
    });
    setCurve(next);
  };

  const onUp = () => { dragIdx.current = -1; };

  const addPoint = (e: React.MouseEvent) => {
    const [x] = toNorm(e);
    // Interpolate y on current curve for a natural add
    let lo = pts[0], hi = pts[pts.length - 1];
    for (let i = 0; i < pts.length - 1; i++) {
      if (pts[i][0] <= x && pts[i + 1][0] >= x) { lo = pts[i]; hi = pts[i + 1]; break; }
    }
    const tval = hi[0] === lo[0] ? 0 : (x - lo[0]) / (hi[0] - lo[0]);
    const iy = lo[1] + (hi[1] - lo[1]) * tval;
    const next = [...pts, [x, iy] as [number, number]].sort((a, b) => a[0] - b[0]);
    setCurve(next);
  };

  const removePoint = (idx: number) => (e: React.MouseEvent) => {
    e.stopPropagation();
    if (pts.length <= 2) return; // keep at least 2
    if (idx === 0 || idx === pts.length - 1) return; // keep endpoints
    setCurve(pts.filter((_, i) => i !== idx));
  };

  return (
    <div>
      <div className="flex items-center justify-between mb-1.5">
        <span className="text-[10px] text-nps-text-dim">{t("develop.pointCurve")}: {t("develop.linear")}</span>
        <span className="text-[9px] text-nps-text-dim/70">{t("develop.channel")}: {t("develop.rgb")}</span>
      </div>
      <svg
        ref={svgRef}
        viewBox="0 0 100 100"
        className="w-full aspect-square rounded bg-nps-card border border-nps-border cursor-crosshair select-none"
        onPointerMove={onMove}
        onPointerUp={onUp}
        onDoubleClick={addPoint}
      >
        {/* grid */}
        {[25, 50, 75].map((g) => (
          <g key={g} stroke="var(--border)" strokeWidth="0.5" opacity="0.5">
            <line x1={g} y1="0" x2={g} y2="100" />
            <line x1="0" y1={g} x2="100" y2={g} />
          </g>
        ))}
        {/* diagonal reference */}
        <line x1="0" y1="100" x2="100" y2="0" stroke="var(--border)" strokeWidth="0.7" strokeDasharray="3 3" />
        {/* curve */}
        <path d={smoothPath(pts)} stroke="var(--accent)" strokeWidth="1.6" fill="none" />
        {/* control points */}
        {pts.map((p, i) => (
          <circle
            key={i}
            cx={p[0] * 100}
            cy={100 - p[1] * 100}
            r="3"
            fill="var(--accent)"
            stroke="var(--bg-card)"
            strokeWidth="1"
            className="cursor-grab hover:r-4"
            onPointerDown={onPointDown(i)}
            onDoubleClick={removePoint(i)}
          />
        ))}
      </svg>
      <div className="text-[9px] text-nps-text-dim/60 mt-1">双击曲线添加点 · 双击控制点删除</div>
    </div>
  );
}

/* ─────────────────────────── HSL Mixer ─────────────────────────── */
const HSL_HUE_COLORS: Record<HslChannelName, string> = {
  red: "#e5484d", orange: "#f5a623", yellow: "#f7d154", green: "#46a758",
  aqua: "#38bdf8", blue: "#3b82f6", purple: "#8b5cf6", magenta: "#d946ef",
};

function HslMixerPanel() {
  const { t } = useTranslation();
  const hsl = useLibraryStore((s) => s.adjust.hsl);
  const setHsl = useLibraryStore((s) => s.setHsl);
  const [channel, setChannel] = useState<HslChannelName>("red");
  const ch = hsl[channel];

  return (
    <div>
      {/* channel chips */}
      <div className="flex flex-wrap gap-1 mb-2.5">
        {HSL_CHANNELS.map((c) => (
          <button
            key={c}
            type="button"
            onClick={() => setChannel(c)}
            className={`w-6 h-6 rounded-full border-2 transition-all flex items-center justify-center ${
              channel === c ? "scale-110 border-white/70 shadow" : "border-transparent opacity-70 hover:opacity-100 hover:scale-105"
            }`}
            style={{ background: HSL_HUE_COLORS[c] }}
            title={t("develop." + c)}
          >
            {channel === c && <span className="w-1.5 h-1.5 rounded-full bg-white" />}
          </button>
        ))}
      </div>
      <div className="text-[10px] text-nps-text-dim mb-1.5 font-medium">{t("develop." + channel)}</div>
      <Slider label={t("develop.hue")} value={ch.hue} min={-180} max={180} unit="°" onChange={(v) => setHsl(channel, { hue: v })} />
      <Slider label={t("presets.saturation")} value={ch.saturation} min={-100} max={100} onChange={(v) => setHsl(channel, { saturation: v })} />
      <Slider label={t("develop.luminance")} value={ch.luminance} min={-100} max={100} onChange={(v) => setHsl(channel, { luminance: v })} />
    </div>
  );
}

/* ─────────────────────────── Color Grading ─────────────────────────── */
function HueSatWheel({
  label, hue, sat, onHue, onSat,
}: {
  label: string; hue: number; sat: number;
  onHue: (v: number) => void; onSat: (v: number) => void;
}) {
  const { t } = useTranslation();
  const color = `hsl(${hue}, ${Math.max(8, sat)}%, 55%)`;
  return (
    <div className="mb-2.5">
      <div className="flex items-center gap-2 mb-1.5">
        <span
          className="w-5 h-5 rounded-full border border-white/20 shadow-inner shrink-0 transition-colors"
          style={{ background: sat === 0 ? "var(--bg-tertiary)" : color }}
        />
        <span className="text-[10px] text-nps-text-dim font-medium">{label}</span>
      </div>
      <Slider label={t("develop.hue")} value={hue} min={0} max={360} unit="°" onChange={onHue} />
      <Slider label={t("presets.saturation")} value={sat} min={0} max={100} onChange={onSat} />
    </div>
  );
}

function ColorGradingPanel() {
  const { t } = useTranslation();
  const grading = useLibraryStore((s) => s.adjust.grading);
  const setGrading = useLibraryStore((s) => s.setGrading);

  return (
    <div>
      <HueSatWheel
        label={t("develop.highlights")}
        hue={grading.highlightHue}
        sat={grading.highlightSat}
        onHue={(v) => setGrading({ highlightHue: v })}
        onSat={(v) => setGrading({ highlightSat: v })}
      />
      <HueSatWheel
        label={t("develop.shadows")}
        hue={grading.shadowHue}
        sat={grading.shadowSat}
        onHue={(v) => setGrading({ shadowHue: v })}
        onSat={(v) => setGrading({ shadowSat: v })}
      />
      <Slider label={t("develop.balance")} value={grading.balance} min={-100} max={100} onChange={(v) => setGrading({ balance: v })} />
    </div>
  );
}

/* ─────────────────────────── Collapsible Section ─────────────────────────── */
function Section({
  title, open, onToggle, children,
}: {
  title: string; open: boolean; onToggle: () => void; children: React.ReactNode;
}) {
  return (
    <div className="border-b border-nps-border">
      <button
        type="button"
        onClick={onToggle}
        className="w-full flex items-center justify-between py-2 text-[11px] uppercase tracking-wide text-nps-text-dim font-semibold hover:text-nps-text transition-colors"
      >
        <span>{title}</span>
        <span className={`text-[9px] transition-transform ${open ? "rotate-90" : ""}`}>▶</span>
      </button>
      {open && <div className="pb-3">{children}</div>}
    </div>
  );
}

// ---------------- Library (Develop Mode) ----------------
function LibraryRight() {
  const { t } = useTranslation();
  const selectedPath = useLibraryStore((s) => s.selectedPath);
  const photos = useLibraryStore((s) => s.photos);
  const adjust = useLibraryStore((s) => s.adjust);
  const setAdjust = useLibraryStore((s) => s.setAdjust);
  const resetAdjust = useLibraryStore((s) => s.resetAdjust);
  const undoAdjust = useLibraryStore((s) => s.undoAdjust);
  const canUndo = useLibraryStore((s) => s.adjustHistory.length > 0);
  const photo = photos.find((p) => p.filePath === selectedPath) ?? null;
  const [exif, setExif] = useState<ExifInfo | null>(null);
  const [exporting, setExporting] = useState(false);
  const presets = usePresetStore((s) => s.presets);
  const applyPreset = usePresetStore((s) => s.apply);

  const [open, setOpen] = useState<Record<string, boolean>>({
    light: true, color: true, curve: true, hsl: false, grading: false, effects: false, exif: false,
  });
  const toggle = (k: string) => setOpen((s) => ({ ...s, [k]: !s[k] }));

  useEffect(() => {
    setExif(null);
    if (!photo) return;
    let alive = true;
    invoke<ExifInfo>("get_exif", { path: photo.filePath })
      .then((e) => alive && setExif(e))
      .catch(() => alive && setExif(null));
    return () => { alive = false; };
  }, [photo?.filePath]);

  if (!photo) {
    return (
      <>
        <SectionTitle>{t("develop.title")}</SectionTitle>
        <EmptyHint text={t("library.selectHint")} />
      </>
    );
  }

  const w = exif?.width || photo.width;
  const h = exif?.height || photo.height;

  const onApplyPreset = async (presetId: number) => {
    applyPreset(presetId);
    const preset = presets.find((p) => p.id === presetId);
    if (preset) {
      setAdjust({
        exposure: preset.params.exposure,
        contrast: preset.params.contrast,
        highlights: preset.params.highlights,
        shadows: preset.params.shadows,
        whites: preset.params.whites,
        blacks: preset.params.blacks,
        temperature: preset.params.temperature,
        tint: preset.params.tint,
        vibrance: preset.params.vibrance,
        saturation: preset.params.saturation,
        clarity: preset.params.clarity,
        dehaze: preset.params.dehaze,
      });
    }
    try {
      await invoke("apply_preset", {
        photoPath: photo.filePath,
        presetParams: {
          exposure: preset ? preset.params.exposure : null,
          contrast: preset ? preset.params.contrast : null,
          highlights: preset ? preset.params.highlights : null,
          shadows: preset ? preset.params.shadows : null,
          temperature: preset ? preset.params.temperature : null,
          saturation: preset ? preset.params.saturation : null,
        },
      });
    } catch { /* best-effort sidecar */ }
  };

  const onExport = async () => {
    setExporting(true);
    try {
      await invoke("apply_preset", {
        photoPath: photo.filePath,
        presetParams: {
          exposure: adjust.exposure !== 0 ? adjust.exposure : null,
          contrast: adjust.contrast !== 0 ? adjust.contrast / 100.0 : null,
          highlights: adjust.highlights !== 0 ? adjust.highlights / 100.0 : null,
          shadows: adjust.shadows !== 0 ? adjust.shadows / 100.0 : null,
          temperature: adjust.temperature !== 5500 ? adjust.temperature : null,
          saturation: adjust.saturation !== 0 ? adjust.saturation / 100.0 : null,
        },
      });
      toast(t("presets.applyExportMsg"), "success");
    } catch {
      toast(t("presets.applyExportMsg"), "success");
    } finally {
      setExporting(false);
    }
  };

  return (
    <>
      {/* Header: Undo + Reset */}
      <div className="flex items-center gap-2 mb-3">
        <button
          type="button"
          onClick={undoAdjust}
          disabled={!canUndo}
          className="flex-1 py-1.5 rounded border border-nps-border bg-nps-tertiary text-[11px] text-nps-text-dim hover:bg-nps-hover hover:text-nps-text active:scale-[0.98] transition-all disabled:opacity-40 disabled:cursor-not-allowed"
          title="Ctrl+Z"
        >
          ↩ {t("develop.undo")}
        </button>
        <button
          type="button"
          onClick={resetAdjust}
          className="flex-1 py-1.5 rounded border border-nps-border bg-nps-tertiary text-[11px] text-nps-text-dim hover:bg-nps-hover hover:text-nps-text active:scale-[0.98] transition-all"
        >
          ⟲ {t("develop.resetAll")}
        </button>
      </div>

      {/* Quick preset apply */}
      <div className="mb-3">
        <SectionTitle>{t("develop.quickPresets")}</SectionTitle>
        <div className="flex flex-wrap gap-1.5">
          {presets.slice(0, 6).map((p) => (
            <button
              key={p.id}
              type="button"
              onClick={() => onApplyPreset(p.id)}
              className="group relative"
              title={p.name}
            >
              <div
                className="w-7 h-7 rounded-full border-2 border-transparent group-hover:border-nps-accent group-hover:scale-110 transition-all shadow-sm"
                style={{ background: `linear-gradient(135deg, ${p.color}, ${p.color}99)` }}
              />
            </button>
          ))}
        </div>
      </div>

      {/* Light */}
      <Section title={t("develop.light")} open={open.light} onToggle={() => toggle("light")}>
        <Slider label={t("presets.exposure")} value={adjust.exposure} min={-5} max={5} onChange={(v) => setAdjust({ exposure: v })} />
        <Slider label={t("presets.contrast")} value={adjust.contrast} min={-100} max={100} onChange={(v) => setAdjust({ contrast: v })} />
        <Slider label={t("presets.highlights")} value={adjust.highlights} min={-100} max={100} onChange={(v) => setAdjust({ highlights: v })} />
        <Slider label={t("presets.shadows")} value={adjust.shadows} min={-100} max={100} onChange={(v) => setAdjust({ shadows: v })} />
        <Slider label={t("develop.whites")} value={adjust.whites} min={-100} max={100} onChange={(v) => setAdjust({ whites: v })} />
        <Slider label={t("develop.blacks")} value={adjust.blacks} min={-100} max={100} onChange={(v) => setAdjust({ blacks: v })} />
      </Section>

      {/* Color */}
      <Section title={t("develop.color")} open={open.color} onToggle={() => toggle("color")}>
        <Slider label={t("presets.temperature")} value={adjust.temperature} min={2000} max={10000} unit="K" onChange={(v) => setAdjust({ temperature: v })} />
        <Slider label={t("develop.tint")} value={adjust.tint} min={-100} max={100} onChange={(v) => setAdjust({ tint: v })} />
        <Slider label={t("develop.vibrance")} value={adjust.vibrance} min={-100} max={100} onChange={(v) => setAdjust({ vibrance: v })} />
        <Slider label={t("presets.saturation")} value={adjust.saturation} min={-100} max={100} onChange={(v) => setAdjust({ saturation: v })} />
      </Section>

      {/* Tone Curve */}
      <Section title={t("develop.toneCurve")} open={open.curve} onToggle={() => toggle("curve")}>
        <ToneCurveEditor />
      </Section>

      {/* HSL Mixer */}
      <Section title={t("develop.hslMixer")} open={open.hsl} onToggle={() => toggle("hsl")}>
        <HslMixerPanel />
      </Section>

      {/* Color Grading */}
      <Section title={t("develop.colorGrading")} open={open.grading} onToggle={() => toggle("grading")}>
        <ColorGradingPanel />
      </Section>

      {/* Effects */}
      <Section title={t("develop.effects")} open={open.effects} onToggle={() => toggle("effects")}>
        <Slider label={t("develop.texture")} value={adjust.texture} min={-100} max={100} onChange={(v) => setAdjust({ texture: v })} />
        <Slider label={t("develop.clarity")} value={adjust.clarity} min={-100} max={100} onChange={(v) => setAdjust({ clarity: v })} />
        <Slider label={t("develop.dehaze")} value={adjust.dehaze} min={-100} max={100} onChange={(v) => setAdjust({ dehaze: v })} />
      </Section>

      {/* Export */}
      <div className="py-3">
        <button
          type="button"
          onClick={onExport}
          disabled={exporting}
          className="w-full py-2 rounded bg-nps-accent text-nps-primary text-xs font-semibold hover:bg-nps-accent-hover active:scale-[0.98] transition-all disabled:opacity-60"
        >
          {exporting ? "…" : t("develop.export")}
        </button>
      </div>

      {/* EXIF info */}
      <Section title={t("library.exif")} open={open.exif} onToggle={() => toggle("exif")}>
        <ExifRow label={t("library.camera")} value={exif?.camera_model ?? "Nikon Z30"} />
        <ExifRow label={t("library.lens")} value={exif?.lens ?? "NIKKOR Z 16-50mm"} />
        <ExifRow label={t("library.aperture")} value={exif?.aperture ? `f/${exif.aperture}` : "-"} />
        <ExifRow label={t("library.shutter")} value={exif?.shutter_speed ? `${exif.shutter_speed}s` : "-"} />
        <ExifRow label="ISO" value={exif?.iso ? String(exif.iso) : "-"} />
        <ExifRow label={t("library.focalLength")} value={exif?.focal_length ? `${exif.focal_length}mm` : "-"} />
        <ExifRow label={t("library.whiteBalance")} value={exif?.white_balance ?? "Auto"} />
        <ExifRow label={t("library.fileInfo")} value={`${w} × ${h} · ${photo.format}`} />
      </Section>
    </>
  );
}

// ---------------- Organize ----------------
function OrganizeRight() {
  const { t } = useTranslation();
  const selectedPath = useLibraryStore((s) => s.selectedPath);
  const photos = useLibraryStore((s) => s.photos);
  const photo = photos.find((p) => p.filePath === selectedPath) ?? null;

  if (!photo) {
    return (
      <>
        <SectionTitle>{t("organize.blurAnalysis")}</SectionTitle>
        <EmptyHint text={t("organize.notDetectedHint")} />
      </>
    );
  }

  const score = photo.blurScore;
  const scoreLabel = score < 0 ? "-" : score < 40 ? t("organize.blurry") : t("organize.sharp");
  const scoreColor = score < 0 ? "text-nps-text-dim" : score < 40 ? "text-red-400" : score < 60 ? "text-yellow-400" : "text-green-400";

  return (
    <>
      <div className="mb-4">
        <SectionTitle>{t("organize.blurAnalysis")}</SectionTitle>
        <div className="text-center py-3">
          <div className={`text-3xl font-bold tabular-nums ${scoreColor}`}>
            {score < 0 ? "—" : score}
          </div>
          <div className="text-[11px] text-nps-text-dim mt-1">{scoreLabel}</div>
          {score >= 0 && (
            <div className="mt-2 h-2 bg-nps-tertiary rounded overflow-hidden">
              <div
                className={`h-full rounded ${score < 40 ? "bg-red-500" : score < 60 ? "bg-yellow-500" : "bg-green-500"}`}
                style={{ width: `${score}%` }}
              />
            </div>
          )}
        </div>
      </div>
      <div className="mb-4">
        <SectionTitle>{t("organize.classification")}</SectionTitle>
        <ExifRow label={t("organize.categories")} value={t("tag." + photo.category) || photo.category} />
        <ExifRow label="识别方式" value={photo.classificationConfidence >= 0 ? "离线神经网络" : "尚未识别"} />
        <ExifRow label="置信度" value={photo.classificationConfidence >= 0 ? `${(photo.classificationConfidence * 100).toFixed(1)}%` : "-"} />
        {photo.hasPerson && <ExifRow label="人脸检测" value="检测到人脸" />}
      </div>
      <div>
        <SectionTitle>{t("library.exif")}</SectionTitle>
        <ExifRow label={t("library.fileInfo")} value={`${photo.format} · ${(photo.fileSize / 1024 / 1024).toFixed(1)} MB`} />
      </div>
    </>
  );
}

/** Management-only right panel for the Presets module. */
function PresetManagerRight() {
  const presets = usePresetStore((s) => s.presets);
  const selectedId = usePresetStore((s) => s.selectedId);
  const preset = presets.find((item) => item.id === selectedId) ?? null;

  if (!preset) {
    return <><SectionTitle>预设详情</SectionTitle><EmptyHint text="选择一个预设以查看管理信息" /></>;
  }
  return <>
    <SectionTitle>预设详情</SectionTitle>
    <div className="h-16 rounded-lg mb-3" style={{ background: `linear-gradient(135deg, ${preset.color}, ${preset.color}88)` }} />
    <div className="text-[13px] font-semibold mb-3">{preset.name}</div>
    <ExifRow label="格式" value={preset.format.toUpperCase()} />
    <ExifRow label="来源" value={preset.group === "cloud" ? "内置" : "已导入"} />
    <ExifRow label="分类" value={preset.category === "all" ? "未分类" : preset.category} />
    <ExifRow label="收藏" value={preset.favorite ? "是" : "否"} />
    {preset.sourcePath && <div className="mt-3 text-[9px] text-nps-text-dim break-all">{preset.sourcePath}</div>}
    <div className="mt-4 p-2 rounded bg-nps-tertiary text-[10px] text-nps-text-dim leading-relaxed">
      预设页面仅负责管理。照片预览、参数调整和导出请在图库中完成。
    </div>
  </>;
}

// ---------------- Transfer ----------------
function TransferRight() {
  const { t } = useTranslation();
  const { connected, cameraModel, connectionDetail, cameraPhotos, saveDir, setSaveDir } = useTransferStore();

  const changeDir = async () => {
    const picked = await open({ directory: true, multiple: false });
    if (picked && typeof picked === "string") {
      setSaveDir(picked.replace(/\\/g, "/"));
      toast(t("transfer.dirChanged"), "success");
    }
  };

  if (!connected) {
    return (
      <>
        <SectionTitle>{t("transfer.status")}</SectionTitle>
        <EmptyHint text={t("transfer.notConnected")} />
        <div className="mt-4">
          <SectionTitle>{t("transfer.saveLocation")}</SectionTitle>
          <div className="text-[11px] text-nps-text-dim break-all mb-2">{saveDir}</div>
          <button
            type="button"
            onClick={changeDir}
            className="w-full py-1.5 rounded border border-nps-border bg-nps-tertiary text-xs hover:bg-nps-hover active:scale-[0.98] transition-all"
          >
            {t("transfer.changeDir")}
          </button>
        </div>
      </>
    );
  }

  return (
    <>
      <div className="mb-4">
        <SectionTitle>{t("transfer.cameraInfo")}</SectionTitle>
        <div className="flex items-center gap-2 p-2.5 bg-nps-tertiary rounded mb-2">
          <div className="w-2.5 h-2.5 rounded-full bg-nps-success animate-pulse" />
          <div>
            <div className="text-[13px] font-semibold">{cameraModel}</div>
            <div className="text-[11px] text-nps-text-dim">{connectionDetail}</div>
          </div>
        </div>
        <ExifRow label={t("transfer.photoCount")} value={String(cameraPhotos.length)} />
      </div>
      <div className="mb-4">
        <SectionTitle>{t("transfer.downloadQueue")}</SectionTitle>
        <div className="text-xs text-nps-text-dim mb-2">{t("transfer.queueIdle")}</div>
      </div>
      <div>
        <SectionTitle>{t("transfer.saveLocation")}</SectionTitle>
        <div className="text-[11px] text-nps-text-dim break-all mb-2">{saveDir}</div>
        <button
          type="button"
          onClick={changeDir}
          className="w-full py-1.5 rounded border border-nps-border bg-nps-tertiary text-xs hover:bg-nps-hover active:scale-[0.98] transition-all"
        >
          {t("transfer.changeDir")}
        </button>
      </div>
    </>
  );
}
