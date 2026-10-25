import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open, save } from "@tauri-apps/plugin-dialog";

export default function NikonRenderPanel() {
  const [sdk, setSdk] = useState("");
  const [mode, setMode] = useState<"as-shot" | "saved-edits">("as-shot");
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState("");
  const chooseSdk = async () => {
    const path = await open({directory: true, title: "选择 Image SDK 的 Library/win/Bin/x64/Release 目录"});
    if (typeof path === "string") { setSdk(path); setMessage(""); }
  };
  const render = async () => {
    if (!sdk || busy) return;
    setBusy(true); setMessage("");
    try {
      const inputPath = await open({multiple: false, title: mode === "saved-edits" ? "选择已嵌入 NX 编辑的 NEF 副本" : "选择原始照片", filters: [{name: "Nikon RAW", extensions: ["nef", "nrw"]}]});
      if (typeof inputPath !== "string") return;
      const stem = inputPath.split(/[\\/]/).pop()?.replace(/\.[^.]+$/, "") ?? "Nikon";
      const outputPath = await save({defaultPath: `${stem}-${mode}.png`, title: "保存新的 sRGB PNG", filters: [{name: "PNG", extensions: ["png"]}]});
      if (!outputPath) return;
      const result = await invoke<{outputPath: string; width: number; height: number; settings: string}>("render_nikon_raw", {sdkDirectory: sdk, inputPath, outputPath, mode});
      setMessage(`已保存 ${result.width} × ${result.height} sRGB PNG：${result.outputPath}。${result.settings === "saved-edits" ? "已读取文件内的适用编辑记录；云创风格一致性仍需对照确认。" : "使用原拍摄设置，未套用网页中的云创配方。"}`);
    } catch (error) { setMessage(String(error)); }
    finally { setBusy(false); }
  };
  return <section className="border-b border-nps-border px-5 py-3 shrink-0">
    <div className="flex flex-wrap items-center gap-2 text-xs">
      <strong>官方显影 · 本机实验</strong>
      <button disabled={busy} onClick={() => {void chooseSdk().catch(error => setMessage(String(error)));}} className="px-3 py-1 rounded bg-nps-tertiary disabled:opacity-40">{sdk ? "更换显影环境" : "选择显影环境"}</button>
      <select aria-label="官方显影模式" disabled={busy} value={mode} onChange={e => {setMode(e.target.value as typeof mode); setMessage("");}} className="bg-nps-primary border border-nps-border rounded p-1">
        <option value="as-shot">原拍摄设置</option><option value="saved-edits">NX 已保存编辑</option>
      </select>
      <button disabled={!sdk || busy} onClick={() => {void render();}} className="px-3 py-1 rounded bg-nps-tertiary disabled:opacity-40">选择 RAW 并显影</button>
    </div>
    <p className="mt-2 text-[11px] text-nps-text-dim">需单独安装并授权 Nikon Image SDK，按项目说明准备显影工具。原片保留，输出新 PNG（8 位、嵌入 sRGB 配置；不复制拍摄 EXIF）。此入口尚不能直接套用云创 NP3。</p>
    {mode === "saved-edits" && <p className="mt-1 text-[11px] text-amber-300">先用 NX Studio 将编辑保存进 NEF 副本；仅有 .nksc 旁车文件无法使用。没有适用编辑时会明确报错。该模式仍待真实编辑照片验证。</p>}
    {busy && <p role="status" className="mt-2 text-xs">正在显影和保存，请稍候…</p>}
    {message && <p role="status" className="mt-2 text-xs break-all">{message}</p>}
  </section>;
}
