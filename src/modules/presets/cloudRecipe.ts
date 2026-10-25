/** Public recipe metadata is not an executable Nikon Picture Control file. */
export interface CloudRecipe {
  id: string;
  name: string;
  pictureControlName: string;
  pictureControl: Record<string, string>;
  captureReference: Record<string, unknown>;
  sourceUrl: string;
}

const ID = /^[0-9A-HJKMNP-TV-Z]{26}$/;
export type CloudRegion = "international" | "china";
const hosts: Record<CloudRegion, string> = {international: "imagingcloud.nikon.com", china: "imagingcloud.nikon.com.cn"};

export function parseRecipeId(input: string, region: CloudRegion = "international"): string {
  const value = input.trim();
  if (ID.test(value)) return value;
  try {
    const url = new URL(value);
    if (url.protocol === "https:" && url.host === hosts[region] && !url.username && !url.password
      && /^\/recipe\/release\/detail\/?$/.test(url.pathname)) {
      const id = url.searchParams.get("id") ?? "";
      if (ID.test(id)) return id;
    }
  } catch { /* Report one actionable input error below. */ }
  throw new Error("请输入当前所选站点的公开风格详情链接或 26 位配方 ID");
}

function record(value: unknown): Record<string, unknown> {
  if (!value || typeof value !== "object" || Array.isArray(value)) throw new Error("云创响应结构不完整");
  return value as Record<string, unknown>;
}

export function normalizeCloudRecipe(input: unknown, region: CloudRegion = "international"): CloudRecipe {
  const root = record(input);
  if (root.error_code) throw new Error("云创服务返回错误，请重新加载或在官方网站查看");
  const raw = record(root.recipe);
  if (typeof raw.id !== "string" || !ID.test(raw.id) || typeof raw.name !== "string" || !raw.name.trim()) {
    throw new Error("云创响应缺少有效配方 ID 或名称");
  }
  const control = record(raw.pictureControl);
  if (typeof control.baseName !== "string" || !control.baseName.trim()) throw new Error("配方缺少基础优化校准类型");
  const pictureControl: Record<string, string> = {};
  for (const [key, value] of Object.entries(control)) {
    if (typeof value === "string") pictureControl[key] = value;
    else if (typeof value === "number" && Number.isFinite(value)) pictureControl[key] = String(value);
    else throw new Error(`尚不支持的云创参数结构：${key}`);
  }
  return {
    id: raw.id, name: raw.name,
    pictureControlName: typeof raw.pictureControlName === "string" ? raw.pictureControlName : raw.name,
    pictureControl,
    captureReference: raw.exif ? record(raw.exif) : {},
    sourceUrl: `https://${hosts[region]}/recipe/release/detail/?id=${raw.id}`,
  };
}

export function assessCloudRecipe(recipe: CloudRecipe) {
  const base = recipe.pictureControl.baseName.trim().toUpperCase();
  const traditional = ["AUTO", "STANDARD", "NEUTRAL", "VIVID", "MONOCHROME", "PORTRAIT", "LANDSCAPE", "FLAT"];
  const kind = base === "FLEXIBLE COLOR" ? "flexible-color" : traditional.includes(base) ? "traditional" : "unknown";
  const missing = ["picture-control-binary"];
  if (Object.values(recipe.pictureControl).some(value => value.toLowerCase() === "gamma")) missing.push("curve-data");
  if (kind === "flexible-color") missing.push("color-blender", "color-grading");
  return {
    kind, missing, canWriteZ30: false, canRenderFaithfully: false,
    reason: kind === "flexible-color"
      ? "此风格使用 Flexible Color，Z30 无法原样机内执行；公开详情还缺少完整曲线及分色数据。"
      : "已取得公开参数资料，尚未取得并验证可执行的优化校准文件，不能直接写入相机。",
  };
}
