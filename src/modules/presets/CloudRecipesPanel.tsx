import { useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open, save } from "@tauri-apps/plugin-dialog";
import { assessCloudRecipe, normalizeCloudRecipe, parseRecipeId, type CloudRecipe, type CloudRegion } from "./cloudRecipe";

interface NikonFileReport {
  displayName: string | null; fileSize: number; version: string;
  hasCustomCurve: boolean; hasAllFlexibleColorBlocks: boolean;
  unknownTags: number[]; renderingVerified: boolean;
}

export default function CloudRecipesPanel() {
  const [catalog, setCatalog] = useState<{id: string; name: string}[]>([]);
  const [region, setRegion] = useState<CloudRegion>("china");
  const [input, setInput] = useState("");
  const [recipe, setRecipe] = useState<CloudRecipe | null>(null);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState("");
  const [usbReport, setUsbReport] = useState<{devices: {name: string; available?: boolean; capabilities?: {id: number; canGet: boolean; canSet: boolean}[]}[]; reason: string; sdkLoaded: boolean} | null>(null);
  const [sdkDirectory, setSdkDirectory] = useState<string | null>(null);
  const [fileReport, setFileReport] = useState<NikonFileReport | null>(null);
  const requestId = useRef(0);
  const assessment = recipe ? assessCloudRecipe(recipe) : null;

  const probeUsb = async () => {
    setBusy(true); setMessage(""); setUsbReport(null);
    try { setUsbReport(await invoke("probe_preset_usb")); }
    catch (error) { setMessage(String(error)); }
    finally { setBusy(false); }
  };

  const probeSdk = async () => {
    setBusy(true); setMessage(""); setUsbReport(null);
    try {
      const directory = sdkDirectory ?? await open({directory: true, multiple: false, title: "选择 Nikon SDK 的 Module/Win/BinaryFile 文件夹"});
      if (typeof directory !== "string") return;
      const report = await invoke<typeof usbReport>("probe_nikon_sdk", {sdkDirectory: directory});
      setSdkDirectory(directory); setUsbReport(report);
    } catch (error) { setSdkDirectory(null); setMessage(String(error)); }
    finally { setBusy(false); }
  };

  const inspectFile = async () => {
    setBusy(true); setMessage(""); setFileReport(null);
    try {
      const path = await open({multiple: false, filters: [{name: "Nikon Picture Control", extensions: ["np3"]}]});
      if (typeof path !== "string") return;
      setFileReport(await invoke("inspect_nikon_file", {path}));
    } catch (error) { setMessage(String(error)); }
    finally { setBusy(false); }
  };

  const loadCatalog = async () => {
    setBusy(true); setMessage("");
    try { setCatalog(await invoke("list_cloud_recipes", {region})); }
    catch (error) { setMessage(String(error)); }
    finally { setBusy(false); }
  };
  const inspect = async (value: string) => {
    const current = ++requestId.current;
    setBusy(true); setRecipe(null); setMessage("");
    try {
      const id = parseRecipeId(value, region);
      const result = normalizeCloudRecipe(await invoke("get_cloud_recipe", {recipeId: id, region}), region);
      if (current === requestId.current) setRecipe(result);
    } catch (error) { if (current === requestId.current) setMessage(String(error)); }
    finally { if (current === requestId.current) setBusy(false); }
  };
  const download = async () => {
    if (!recipe) return;
    setBusy(true); setMessage("");
    try {
      const path = await save({defaultPath: `${recipe.id}.nikon-recipe.json`, filters: [{name: "云创公开配方资料", extensions: ["json"]}]});
      if (!path) return;
      const result = await invoke<string>("download_cloud_recipe", {recipeId: recipe.id, outputPath: path, region});
      setMessage(`已保存公开参数资料：${result}。这不是可导入相机的 NP3 文件。`);
    } catch (error) { setMessage(String(error)); }
    finally { setBusy(false); }
  };
  return <section className="border-b border-nps-border px-5 py-3 bg-nps-secondary/40 shrink-0">
    <div className="flex flex-wrap gap-2 items-center">
      <span className="text-xs font-semibold">尼康云创 · 实验功能</span>
      <select aria-label="云创站点" disabled={busy} value={region} onChange={e => {
        setRegion(e.target.value as CloudRegion); setCatalog([]); setRecipe(null); setInput(""); setMessage("");
      }} className="bg-nps-primary border border-nps-border rounded p-1 text-xs">
        <option value="china">中国站</option><option value="international">国际站</option>
      </select>
      <button disabled={busy} onClick={loadCatalog} className="px-3 py-1 rounded bg-nps-tertiary text-xs disabled:opacity-40">读取公开风格</button>
      <button disabled={busy} onClick={probeUsb} className="px-3 py-1 rounded bg-nps-tertiary text-xs disabled:opacity-40">检测 USB 相机</button>
      <button disabled={busy} onClick={probeSdk} className="px-3 py-1 rounded bg-nps-tertiary text-xs disabled:opacity-40">检测 SDK 能力</button>
      <button disabled={busy} onClick={inspectFile} className="px-3 py-1 rounded bg-nps-tertiary text-xs disabled:opacity-40">检查 NP3 文件</button>
      <input aria-label="云创配方链接或 ID" value={input} onChange={e => setInput(e.target.value)} placeholder="粘贴云创公开风格详情链接或 ID" className="flex-1 min-w-48 px-2 py-1 bg-nps-primary border border-nps-border rounded text-xs" />
      <button disabled={busy || !input.trim()} onClick={() => inspect(input)} className="px-3 py-1 bg-nps-tertiary rounded text-xs disabled:opacity-40">查看参数</button>
    </div>
    <p className="text-[11px] text-nps-text-dim mt-2">公开风格可直接读取，无需账号密码。完整优化校准文件与 USB 写入能力仍需单独验证。</p>
    {catalog.length > 0 && <select aria-label="公开云创风格" disabled={busy} value={recipe?.id ?? ""} onChange={e => {setInput(e.target.value); void inspect(e.target.value);}} className="mt-2 w-full bg-nps-primary border border-nps-border rounded p-1 text-xs">
      <option value="" disabled>选择风格（{catalog.length} 个）</option>
      {catalog.map(item => <option key={item.id} value={item.id}>{item.name}</option>)}
    </select>}
    {busy && <p role="status" className="text-xs mt-2">正在处理…</p>}
    {message && <p role="status" className="text-xs mt-2 break-all">{message}</p>}
    {fileReport && <div role="status" className="mt-2 text-xs border border-nps-border rounded p-2">
      <strong>{fileReport.displayName || "Nikon 预设"}</strong> · {fileReport.fileSize} 字节 · 格式 {fileReport.version}
      <p>{fileReport.hasCustomCurve ? "包含自定义曲线" : "未发现自定义曲线块"}；{fileReport.hasAllFlexibleColorBlocks ? "Flexible Color 数据块齐全" : "未发现齐全的 Flexible Color 数据块"}。
        {fileReport.unknownTags.length > 0 && `包含 ${fileReport.unknownTags.length} 个待识别字段。`}</p>
      <p className="text-amber-300">文件结构已读取。完整显影尚未验证，原文件不会被转换成通用调色参数。</p>
    </div>}
    {usbReport && <div role="status" className="mt-2 text-xs text-nps-text-dim">
      <p>{usbReport.devices.length ? `检测到：${usbReport.devices.map(d => d.name).join("、")}` : "未检测到 Nikon USB 相机，请开机并使用数据线连接。"}</p>
      <p>{usbReport.reason}</p>
      {usbReport.sdkLoaded && usbReport.devices.map(device => <p key={device.name}>
        {device.name}：{device.available === false ? "被其他软件占用或暂不可用" :
          device.capabilities?.length ? device.capabilities.map(cap => `${cap.id}（${cap.canGet ? "可读" : "不可读"} / ${cap.canSet ? "报告支持设置" : "不可设置"}）`).join("、") : "未返回优化校准能力"}
      </p>)}
    </div>}
    {recipe && assessment && <div className="mt-3 border border-nps-border rounded p-3">
      <div className="flex items-center gap-2"><strong className="text-xs">{recipe.name}</strong><span className="text-[10px] text-nps-text-dim">{recipe.pictureControl.baseName}</span></div>
      <p className="text-xs text-amber-300 mt-2">{assessment.reason}</p>
      <div className="mt-2 max-h-32 overflow-y-auto grid grid-cols-2 md:grid-cols-4 gap-1 text-[11px]">
        {Object.entries(recipe.pictureControl).map(([key, value]) => <div key={key}><span className="text-nps-text-dim">{key}: </span>{value}</div>)}
      </div>
      <div className="flex gap-2 mt-3">
        <button disabled={busy} onClick={download} className="px-3 py-1 bg-nps-accent text-nps-primary rounded text-xs disabled:opacity-40">下载配方资料 JSON</button>
        <button disabled title="尚未取得完整预设及验证 USB 写入接口" className="px-3 py-1 bg-nps-tertiary rounded text-xs opacity-40">写入 Z30（待验证）</button>
        <a href={recipe.sourceUrl} target="_blank" rel="noreferrer" className="text-xs text-nps-accent self-center">官方详情</a>
      </div>
    </div>}
  </section>;
}
