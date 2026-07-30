import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { useTransferStore, type CameraPhoto } from "../../stores/transferStore";
import { toast } from "../../stores/toastStore";
import { useTranslation } from "react-i18next";

interface ConnectResult {
  success: boolean;
  message: string;
  camera_model?: string | null;
}

const humanSize = (bytes: number) => bytes >= 1024 ** 2
  ? `${(bytes / 1024 ** 2).toFixed(1)} MB`
  : `${Math.max(1, Math.round(bytes / 1024))} KB`;

const formatName = (format: CameraPhoto["format"]) =>
  typeof format === "string" ? format : Object.values(format)[0] || "RAW";

export default function TransferView() {
  const { t } = useTranslation();
  const store = useTransferStore();
  const {
    connectionType, connected, connecting, cameraModel, connectionDetail, cameraPhotos,
    connectionError, wifiHost, saveDir, selectedHandles, toggleSelect, selectAll,
    deselectAll, setConnected, setConnecting, setCameraInfo, setConnectionError, setSaveDir,
  } = store;
  const [downloading, setDownloading] = useState(false);
  const [thumbs, setThumbs] = useState<Record<number, string>>({});

  useEffect(() => () => Object.values(thumbs).forEach(URL.revokeObjectURL), [thumbs]);

  useEffect(() => {
    if (!connected || connectionType === "usb" || cameraPhotos.length === 0) return;
    let active = true;
    (async () => {
      const next: Record<number, string> = {};
      for (const photo of cameraPhotos.slice(0, 120)) {
        try {
          const bytes = await invoke<number[]>("get_thumbnail", { handle: photo.handle });
          if (!active) break;
          next[photo.handle] = URL.createObjectURL(new Blob([new Uint8Array(bytes)], { type: "image/jpeg" }));
          setThumbs({ ...next });
        } catch { /* Some cameras do not expose PTP thumbnails. */ }
      }
    })();
    return () => { active = false; };
  }, [connected, connectionType, cameraPhotos]);

  const modeHelp = useMemo(() => {
    if (connectionType === "wifi-ap") return "直连 AP：电脑连接相机创建的 Wi‑Fi；该网络显示“无 Internet”是正常现象。";
    if (connectionType === "wifi-sta") return "基础设施 STA：相机与电脑连接同一路由器，并填写相机在局域网中的 IPv4 地址。";
    return "USB：相机的 USB 连接模式须设为 MTP/PTP；请解锁相机并确认存储卡可读取。";
  }, [connectionType]);

  const doConnect = async () => {
    setConnecting(true);
    setConnectionError("");
    try {
      const result = connectionType === "usb"
        ? await invoke<ConnectResult>("connect_usb")
        : await invoke<ConnectResult>("connect_wifi", {
            args: { mode: connectionType === "wifi-sta" ? "sta" : "ap", host: wifiHost.trim(), port: 15740 },
          });
      if (!result.success) throw new Error(result.message);
      const photos = await invoke<CameraPhoto[]>("list_photos");
      const detail = connectionType === "usb" ? "USB · Windows MTP/PTP" : `${connectionType === "wifi-ap" ? "Wi‑Fi 直连 AP" : "Wi‑Fi 基础设施 STA"} · ${wifiHost}:15740`;
      setCameraInfo(result.camera_model || "Nikon 相机", detail, photos);
      setConnected(true);
      toast(`已连接 ${result.camera_model || "Nikon 相机"}，读取到 ${photos.length} 张照片`, "success");
    } catch (error) {
      const raw = error instanceof Error ? error.message : String(error);
      const hint = connectionType === "usb"
        ? `${raw}。请在相机中选择 USB=MTP/PTP，退出菜单/回放占用后重试。`
        : `${raw}。请先用 Nikon Wireless Transmitter Utility 完成“连接到计算机”配对，并确认当前网络模式与地址。`;
      setConnectionError(hint);
      setConnected(false);
      toast(hint, "error");
    } finally {
      setConnecting(false);
    }
  };

  const doDisconnect = async () => {
    await invoke("disconnect").catch(() => undefined);
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
    const targets = selectedHandles.length
      ? cameraPhotos.filter((p) => selectedHandles.includes(p.handle))
      : cameraPhotos;
    setDownloading(true);
    let completed = 0;
    try {
      for (const photo of targets) {
        const destPath = `${directory.replace(/[\\/]$/, "")}\\${photo.filename}`;
        await invoke("download_photo", { handle: photo.handle, destPath });
        completed += 1;
      }
      deselectAll();
      toast(t("transfer.downloadedMsg", { count: completed }), "success");
    } catch (error) {
      toast(`已下载 ${completed}/${targets.length}；${String(error)}`, "error");
    } finally {
      setDownloading(false);
    }
  };

  if (!connected) {
    return (
      <div className="h-full flex items-center justify-center p-8">
        <div className="text-center max-w-xl w-full">
          <div className="mx-auto w-20 h-20 rounded-full bg-[var(--bg-tertiary)] flex items-center justify-center text-4xl mb-5">📷</div>
          <h2 className="text-2xl font-bold mb-2 text-nps-text">连接 Nikon 相机</h2>
          <p className="text-[13px] text-nps-text-dim leading-relaxed mb-4">{modeHelp}</p>
          {connectionType !== "usb" && (
            <label className="block text-left mb-4">
              <span className="block text-[11px] text-nps-text-dim mb-1">相机 IPv4 地址</span>
              <input
                value={wifiHost}
                onChange={(e) => store.setWifiHost(e.target.value)}
                className="w-full rounded border border-nps-border bg-nps-tertiary px-3 py-2 text-sm outline-none focus:border-nps-accent"
                placeholder={connectionType === "wifi-ap" ? "通常为 192.168.1.1" : "例如 192.168.0.108"}
              />
            </label>
          )}
          {connectionError && <div className="mb-4 rounded border border-nps-danger/40 bg-nps-danger/10 p-3 text-left text-xs text-nps-danger leading-relaxed">{connectionError}</div>}
          <button type="button" onClick={doConnect} disabled={connecting}
            className="px-8 py-3 rounded-lg bg-nps-accent text-nps-primary font-semibold text-[15px] hover:bg-nps-accent-hover disabled:opacity-60">
            {connecting ? "正在检测设备并读取存储卡…" : "连接并扫描照片"}
          </button>
        </div>
      </div>
    );
  }

  return (
    <div className="h-full flex flex-col">
      <div className="flex items-center gap-3 px-4 py-2.5 border-b border-nps-border shrink-0">
        <div className="w-2 h-2 rounded-full bg-nps-success animate-pulse" />
        <span className="text-[13px] font-semibold text-nps-text">{cameraModel}</span>
        <span className="text-[11px] text-nps-text-dim">{connectionDetail}</span>
        <span className="text-[12px] text-nps-text-dim">{cameraPhotos.length} 张照片</span>
        {selectedHandles.length > 0 && <span className="text-xs text-nps-accent">已选 {selectedHandles.length}</span>}
        <div className="ml-auto flex gap-2">
          <button onClick={() => selectAll(cameraPhotos.map((p) => p.handle))} className="px-2.5 py-1 rounded border border-nps-border bg-nps-tertiary text-[11px]">全选</button>
          <button onClick={deselectAll} className="px-2.5 py-1 rounded border border-nps-border bg-nps-tertiary text-[11px]">取消选择</button>
          <button onClick={doDownload} disabled={downloading || !cameraPhotos.length} className="px-4 py-1 rounded bg-nps-accent text-nps-primary text-[12px] font-semibold disabled:opacity-60">
            {downloading ? "正在下载…" : "下载"}
          </button>
          <button onClick={doDisconnect} className="px-2.5 py-1 rounded border border-nps-danger/40 text-nps-danger text-[11px]">断开</button>
        </div>
      </div>
      <div className="flex-1 p-4 overflow-y-auto">
        {!cameraPhotos.length ? <div className="h-full flex items-center justify-center text-nps-text-dim text-sm">设备已连接，但存储卡中未发现 NEF/JPEG/TIFF 照片</div> : (
          <div className="thumb-grid">
            {cameraPhotos.map((photo) => {
              const selected = selectedHandles.includes(photo.handle);
              return <button key={photo.handle} onClick={() => toggleSelect(photo.handle)} className={`thumb-item text-left ${selected ? "selected" : ""}`} title={photo.filename}>
                <div className="aspect-[4/3] bg-nps-tertiary flex items-center justify-center overflow-hidden">
                  {thumbs[photo.handle] ? <img src={thumbs[photo.handle]} className="w-full h-full object-cover" /> : <div className="text-center text-nps-text-dim"><div className="text-3xl">📷</div><div className="text-[10px] mt-1">{formatName(photo.format)}</div></div>}
                </div>
                <div className="px-2 py-1.5"><div className="text-xs truncate">{photo.filename}</div><div className="text-[10px] text-nps-text-dim">{photo.width && photo.height ? `${photo.width}×${photo.height} · ` : ""}{humanSize(photo.size)}</div></div>
                <span className={`absolute top-1 left-1 w-5 h-5 rounded grid place-items-center text-[11px] ${selected ? "bg-nps-accent text-black" : "bg-black/40 text-white/50"}`}>{selected ? "✓" : ""}</span>
              </button>;
            })}
          </div>
        )}
      </div>
    </div>
  );
}
