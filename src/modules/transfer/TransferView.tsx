import { useState } from "react";
import { useTransferStore } from "../../stores/transferStore";
import { useLibraryStore } from "../../stores/libraryStore";
import { toast } from "../../stores/toastStore";
import Thumbnail from "../../components/Thumbnail";
import { useTranslation } from "react-i18next";

export default function TransferView() {
  const { t } = useTranslation();
  const { connectionType, connected, connecting, selectedHandles, toggleSelect, selectAll, deselectAll, setConnected, setConnecting } =
    useTransferStore();
  const photos = useLibraryStore((s) => s.photos);
  const importFromDialog = useLibraryStore((s) => s.importFromDialog);

  const [downloading, setDownloading] = useState(false);

  // Use imported photos as a stand-in for the camera's card contents.
  const cameraPhotos = photos.map((p, i) => ({ handle: i, photo: p }));

  const doConnect = () => {
    setConnecting(true);
    setTimeout(() => {
      setConnected(true);
      toast(`${t("transfer.connectedMsg")} (${connectionType.toUpperCase()})`, "success");
    }, 900);
  };

  const doDownload = () => {
    const n = selectedHandles.length || cameraPhotos.length;
    setDownloading(true);
    setTimeout(() => {
      setDownloading(false);
      deselectAll();
      toast(t("transfer.downloadedMsg", { count: n }), "success");
    }, 1500);
  };

  // ---- Not connected ----
  if (!connected) {
    return (
      <div className="h-full flex items-center justify-center p-8">
        <div className="text-center max-w-md">
          <div className="mx-auto w-24 h-24 rounded-full bg-[var(--bg-tertiary)] flex items-center justify-center text-5xl mb-6 animate-[float_3s_ease-in-out_infinite]">
            📶
          </div>
          <h2 className="text-2xl font-bold mb-2 text-nps-text">{t("transfer.notConnected")}</h2>
          <p className="text-[14px] text-nps-text-dim mb-2 leading-relaxed">
            {t("transfer.connectDesc")}
          </p>
          <p className="text-[12px] text-nps-accent mb-8">
            {t("transfer.method")}: {connectionType.toUpperCase()}
          </p>
          <div className="flex items-center justify-center gap-3">
            <button
              type="button"
              onClick={doConnect}
              disabled={connecting}
              className="px-8 py-3 rounded-lg bg-nps-accent text-nps-primary font-semibold text-[15px] hover:bg-nps-accent-hover active:scale-[0.98] transition-all shadow-lg disabled:opacity-60"
            >
              {connecting ? t("transfer.connecting") : `🔗 ${t("transfer.connect")}`}
            </button>
            <button
              type="button"
              onClick={async () => {
                const n = await importFromDialog();
                if (n > 0) toast(t("library.importedMsg", { count: n }), "success");
              }}
              className="px-5 py-3 rounded-lg border border-nps-border bg-nps-tertiary text-nps-text text-[14px] hover:bg-nps-hover active:scale-[0.98] transition-all"
            >
              📁 {t("transfer.importInstead")}
            </button>
          </div>
        </div>
      </div>
    );
  }

  // ---- Connected: camera photo grid ----
  return (
    <div className="h-full flex flex-col">
      <div className="flex items-center gap-3 px-4 py-2.5 border-b border-nps-border shrink-0">
        <div className="flex items-center gap-2">
          <div className="w-2 h-2 rounded-full bg-nps-success animate-pulse" />
          <span className="text-[13px] font-semibold text-nps-text">Nikon Z30</span>
          <span className="text-[11px] text-nps-text-dim">· {connectionType.toUpperCase()}</span>
        </div>
        <div className="text-[12px] text-nps-text-dim">
          {cameraPhotos.length} {t("library.photos")}
          {selectedHandles.length > 0 && (
            <span className="ml-2 text-nps-accent">· {selectedHandles.length} {t("transfer.selected")}</span>
          )}
        </div>
        <div className="ml-auto flex items-center gap-2">
          <button
            type="button"
            onClick={() => selectAll(cameraPhotos.map((c) => c.handle))}
            className="px-2.5 py-1 rounded border border-nps-border bg-nps-tertiary text-[11px] hover:bg-nps-hover transition-colors"
          >
            {t("library.selectAll")}
          </button>
          <button
            type="button"
            onClick={deselectAll}
            className="px-2.5 py-1 rounded border border-nps-border bg-nps-tertiary text-[11px] hover:bg-nps-hover transition-colors"
          >
            {t("transfer.deselect")}
          </button>
          <button
            type="button"
            onClick={doDownload}
            disabled={downloading || cameraPhotos.length === 0}
            className="px-4 py-1 rounded bg-nps-accent text-nps-primary text-[12px] font-semibold hover:bg-nps-accent-hover active:scale-[0.97] transition-all disabled:opacity-60"
          >
            {downloading ? t("transfer.downloading") : `⬇️ ${t("transfer.download")}`}
          </button>
          <button
            type="button"
            onClick={() => {
              setConnected(false);
              deselectAll();
              toast(t("transfer.disconnectedMsg"), "info");
            }}
            className="px-2.5 py-1 rounded border border-nps-danger/40 text-nps-danger text-[11px] hover:bg-nps-danger/10 transition-colors"
          >
            {t("transfer.disconnect")}
          </button>
        </div>
      </div>

      <div className="flex-1 p-4 overflow-y-auto">
        {cameraPhotos.length === 0 ? (
          <div className="h-full flex items-center justify-center text-nps-text-dim text-sm">
            {t("transfer.noPhotos")}
          </div>
        ) : (
          <div className="thumb-grid">
            {cameraPhotos.map(({ handle, photo }) => {
              const sel = selectedHandles.includes(handle);
              return (
                <div
                  key={handle}
                  onClick={() => toggleSelect(handle)}
                  className={`thumb-item ${sel ? "selected" : ""}`}
                  title={photo.filename}
                >
                  <Thumbnail photo={photo} />
                  <div className="thumb-label">{photo.filename}</div>
                  <span
                    className={`absolute top-1 left-1 w-5 h-5 rounded flex items-center justify-center text-[10px] font-bold transition-all ${
                      sel
                        ? "bg-nps-accent text-nps-primary"
                        : "bg-black/40 text-white opacity-0"
                    }`}
                    style={{ opacity: sel ? 1 : undefined }}
                  >
                    ✓
                  </span>
                </div>
              );
            })}
          </div>
        )}
      </div>
    </div>
  );
}