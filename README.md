# Nikon Photo Studio

Nikon Photo Studio 是一款面向 Nikon 相机用户的 Windows 本地影像工作台。项目使用 Tauri 2、Rust、React 和 TypeScript 构建，把照片导入、NEF/RAW 高清预览、筛选整理、人像与模糊检测、非破坏性调整、预设管理以及相机传输集中到一个桌面应用中。

照片解码、缩略图生成和机器学习检测均在本机完成，默认不上传照片。

> 当前版本：`0.1.0`。目前以 Windows 10/11 为主要运行平台；Wi‑Fi 与 USB 传输功能仍需要更多 Nikon 机型和固件版本的实机验证。

## 核心功能

### 1. 图库与目录管理

- 递归导入文件夹，支持 Nikon NEF、JPEG、PNG 和 TIFF。
- 以文件夹为基本工作范围，可查看全部目录或单独目录。
- 支持格式、标记、预设分类和星级组合筛选。
- 星级筛选采用“不限 / 大于等于 / 小于等于 + 星级”的明确条件。
- 支持单选、多选、Shift 连选、全选、标记、评分和批量操作。
- 双击照片进入大图模式，支持方向键切换、鼠标滚轮缩放、拖拽平移和原图对比。
- 图库和整理模块共享当前目录范围；从整理结果打开照片后，底部胶片条保持当前整理结果，不跳回全部图库。
- 显示文件类型、大小、图像尺寸及常用 EXIF 信息。

### 2. RAW / NEF 自适应高清预览

- 打开 NEF 时优先显示相机内嵌 JPEG，避免等待完整 RAW 解码后才出现画面。
- 后台使用 `rawler` 解码传感器数据，完成去马赛克、白平衡、相机色彩矩阵和 sRGB 输出。
- 根据可视区域、系统 DPI 和当前缩放倍率，自适应选择 `2048 / 3072 / 4096 / 6144 / 8192 px` 预览档位。
- NEF 不再受内嵌小 JPEG 尺寸限制；放大时会请求更高分辨率 RAW 渲染。
- 使用 Lanczos3 高质量缩放和轻量捕获锐化，改善毛发、植被、建筑纹理和文字细节。
- JPEG 在原始像素范围内按显示尺寸渲染，避免对低分辨率文件进行破坏性过度锐化。
- 自动处理 JPEG EXIF 和 NEF TIFF 方向信息，修复竖构图被横向显示的问题。
- 缩略图、快速预览和 RAW 高清渲染使用内容寻址缓存，重复查看无需再次完整解码。

### 3. 模糊检测与人像检测

整理模块只保留三个清晰类别：

- **模糊**：显示模糊检测分数低于当前阈值的照片。
- **人像**：显示 UltraFace 检测置信度达到人像阈值的照片。
- **其他**：未达到人像阈值、且不按模糊结果查看的照片。

检测能力包括：

- 综合 Laplacian 方差、梯度、高频占比和局部对比度计算清晰度分数。
- 严格、标准、宽松三档分析灵敏度。
- 连续模糊阈值，可即时改变模糊结果范围。
- 内嵌离线 UltraFace ONNX 模型进行人脸/人像检测。
- 人像置信度阈值可在 `40%–95%` 之间线性调整，无需重新运行模型即可重新划分类别。
- 检测可限定为图库当前选中的文件夹，避免每次分析全部照片。
- 支持整理网格双击查看、右键删除、批量选择、归档和删除。

### 4. 照片调整与预设

- 曝光、对比度、高光、阴影、白色、黑色、色温和色调。
- 自然饱和度、饱和度、纹理、清晰度和去雾。
- 八色 HSL、色彩分级、点曲线及效果参数。
- Canvas 像素管线实时生成预览，不直接覆盖原始照片。
- 支持撤销、重置、按住查看原图及前后对比。
- 支持导入 XMP、CUBE、NP3、NCP、Lightroom `.lrpreset`、3DL 和 Capture One `.costyle`。
- 预设模块专注于预设的导入、分类、收藏和管理，照片预览与导出在图库中完成。
- 可保存已导入预设、当前预设选择和最近使用状态。

### 5. 高质量照片导出

- 图库顶部与照片调整面板共用同一个真实导出流程，不再显示“功能开发中”。
- 支持 JPEG、PNG 和 TIFF；JPEG 可设置 `1–100` 品质，PNG/TIFF 支持 `8 bit` 与 `16 bit` 输出。
- 支持保持原始像素、指定最长边、限定最大宽高三种尺寸方式，始终保持原图比例。
- 可选择“不放大原图”，避免通过插值制造并不存在的细节；缩小使用 Lanczos3 高质量重采样。
- RAW/NEF 导出直接解码全尺寸传感器数据，不使用低分辨率内嵌预览，并应用当前曝光、色温、明暗、饱和度、清晰度等调整。
- 可设置输出目录、文件名后缀、同名覆盖或自动编号，并显示预计输出像素和打印 DPI。
- 屏幕清晰度由实际像素尺寸决定；DPI 用于描述打印尺寸，不会凭空增加照片细节。

### 6. 会话恢复与便携数据库

便携版运行时会在 EXE 同目录创建：

```text
nikon-photo-studio.db
```

SQLite 数据库保存以下工作状态：

- 已导入目录及照片索引。
- 当前文件夹、格式、星级、分类等筛选条件。
- 照片星级、标记、模糊分数和人像检测结果。
- 当前照片、查看位置、缩略图尺寸和照片调整参数。
- 导入的预设、预设分类、收藏、选择及使用状态。
- 整理阈值、界面语言、主题、面板状态和传输设置。

应用重新打开时会检查照片路径是否仍然存在，并恢复上次工作状态。若 EXE 所在目录不可写，数据库自动回退到 Windows 用户应用数据目录。

### 7. Nikon 相机传输

#### Wi‑Fi 直连（AP）

- 相机创建无线网络，电脑连接相机 SSID。
- Windows 显示“无 Internet”属于直连模式的正常现象。
- 自动读取当前 Windows Wi‑Fi 的 SSID、本机 IPv4 与默认网关；选择 AP 模式并连接 `NIKON_…` 热点后自动使用相机网关，不再要求手工猜测地址。
- 使用标准 PTP/IP 命令与事件双通道建立持久会话，再通过数据阶段读取设备型号、真实存储 ID、文件列表、缩略图与原始文件。
- 连接过程显示网络确认、PTP/IP 握手、会话建立和存储卡索引进度；大目录扫描会持续显示已扫描对象数与已识别照片数。
- 连接等待支持随时取消；应用会主动中止当前 PTP/IP 命令并关闭命令/事件双通道，不必等待卡住的缩略图或 RAW 读取自行超时。
- 缩略图按当前可见范围逐张读取并立即显示，成功结果保存在有界会话缓存中；改变卡片大小不会重复请求相机固定的内嵌缩略图。
- 原文件使用 64 KiB 流式传输和同目录 `.part` 暂存，完整长度校验后再原子发布，下载界面显示字节进度、百分比和实时速度。
- 相机必须停留在“连接到计算机 → Wi‑Fi 连接 → 建立连接”的等待画面。仅打开相机 Wi‑Fi 或 SnapBridge“连接至智能设备”不会开放电脑图传服务。

#### Wi‑Fi 基础设施（STA）

- 相机和电脑连接同一个路由器或局域网。
- 在界面中填写相机获得的 IPv4 地址。
- 电脑可继续使用路由器提供的互联网连接。

Wi‑Fi 连接前需在相机的“连接到计算机”菜单中完成设置；部分机型需要先使用 Nikon Wireless Transmitter Utility 配对。

#### USB（MTP/PTP）

- 使用 Windows Portable Devices / Explorer Shell 枚举 Nikon MTP/PTP 设备。
- 自动查找 Z30、Z50/Z50II 等 Nikon 相机及其存储卡目录。
- 扫描 NEF/NRW、JPEG 和 TIFF 文件，显示真实设备名称、文件名、类型和大小。
- 通过 Windows WPD/Shell 图像提取器按可见区域分批读取最高 1024px 的真实相机缩略图，无需为网格预览先复制完整 NEF；驱动未提供预览时会显示“无预览”，不会显示误导性的通用图标。
- 支持普通单选、Ctrl 增减选择、Shift 连选、Ctrl+A 全选和响应式照片卡片调节；最大档会按中央区域宽度显示单列大图。
- 有选择时完整复制所选原始文件，无选择时完整复制全部文件；逐文件显示进度，并校验落盘字节数与相机报告大小一致。
- 读取相机保护/只读状态并显示锁标识；支持右键或工具栏永久删除，受保护照片自动跳过。相机存储删除不会进入 Windows 回收站。
- 相机菜单中的 USB 连接模式必须设置为 `MTP/PTP`；连接时应解锁相机并退出可能占用存储卡的菜单或回放状态。

应用不会再使用模拟的设备名称、电量、容量或照片数量。当前界面只显示协议真实返回或 Windows 实际枚举到的信息。

传输网格会按滚动位置分批加载缩略图并持续显示载入进度。双击后先显示相机内嵌图，再把完整原片流式写入本次会话的稳定缓存，并复用图库/整理的详情查看器；内嵌图会一直保留到高清画布真正绘制首帧，避免加载阶段出现黑屏。底部胶片条保持当前相机目录，支持双击放大、Ctrl+滚轮、拖拽平移和方向键切换。RAW/JPEG 会按可视区域、系统 DPI 和缩放倍率自适应选择 `2048–8192 px` 渲染档位；最近三张或约 512 MiB 原片会保留，档位升级、来回查看或预览后下载都无需重复传输。连接方式切换使用前端串行转换队列、后端原子会话代次和可中断 PTP/IP 通道，切换页面会立即响应，过期的 USB/Wi-Fi 任务不会覆盖或断开新会话。

## 技术架构

| 层级 | 技术 | 用途 |
| --- | --- | --- |
| 桌面容器 | Tauri 2 | Windows 窗口、IPC、插件和打包 |
| 前端 | React 18、TypeScript、Vite | 界面、查看器与交互 |
| 状态管理 | Zustand | 图库、整理、预设和传输状态 |
| 样式 | Tailwind CSS | 桌面布局与主题 |
| 后端 | Rust、Tokio | 文件处理、缓存、数据库和相机通信 |
| RAW 解码 | rawler | Nikon NEF 解码与开发 |
| 图像处理 | image-rs | 方向修正、缩放、锐化和 JPEG 编码 |
| 数据库 | SQLite、rusqlite | 工作区状态和历史信息 |
| 机器学习 | tract-onnx、UltraFace | 本地人像检测 |
| 相机协议 | PTP/IP、Windows MTP/PTP | Wi‑Fi 与 USB 照片读取 |
| 测试 | Vitest、Rust tests | 前端状态和后端算法验证 |

## 项目结构

```text
nikon-photo-studio/
├─ models/                     # 离线 ONNX 模型
├─ src/
│  ├─ components/              # 通用 UI、预览器、缩略图和胶片条
│  ├─ modules/                 # 图库、整理、预设、传输模块
│  ├─ stores/                  # Zustand 状态
│  ├─ lib/                     # 图像处理和会话持久化
│  └─ i18n/                    # 中英文资源
├─ src-tauri/
│  └─ src/
│     ├─ camera/               # PTP/IP 与 Windows MTP/PTP
│     ├─ library/              # 导入、EXIF、缩略图和 RAW 渲染
│     ├─ organize/             # 模糊检测与人像检测
│     ├─ presets/              # 预设解析和应用
│     ├─ fs/                   # 扫描、归档和回收站
│     └─ db.rs                 # SQLite 数据库
├─ test_presets/               # 预设解析样本
├─ CHANGELOG.md                # 按日期维护的每日更新记录
└─ README.md
```

## 开发环境

推荐配置：

- Windows 10/11 x64
- Node.js 20 或更高版本
- pnpm 9 或更高版本
- Rust stable
- Microsoft Edge WebView2 Runtime
- Microsoft C++ Build Tools，或完整的 MinGW-w64 工具链

安装依赖：

```powershell
pnpm install
```

启动开发环境：

```powershell
pnpm tauri dev
```

仅启动浏览器前端：

```powershell
pnpm dev
```

## 测试

```powershell
# 前端测试
pnpm test

# 前端生产构建
pnpm build

# Rust 检查
cargo check --manifest-path src-tauri/Cargo.toml

# Rust 全量测试
cargo test --manifest-path src-tauri/Cargo.toml
```

使用真实 NEF 运行可选集成测试：

```powershell
$env:NPS_TEST_NEF = "D:\Photos\sample.NEF"
cargo test --manifest-path src-tauri/Cargo.toml full_raw_decode_real_nef_if_available -- --nocapture
```

## 构建

生成普通 Release 可执行文件：

```powershell
pnpm build
cargo build --release --manifest-path src-tauri/Cargo.toml
```

可执行文件位于：

```text
src-tauri/target/release/nikon-photo-studio.exe
```

生成 NSIS 安装包：

```powershell
pnpm tauri build
```

安装包默认位于：

```text
src-tauri/target/release/bundle/nsis/
```

构建产物、EXE、数据库、缓存、照片 sidecar 和大型压缩文件均由 `.gitignore` 排除，不应提交到源码仓库。

## 数据安全与隐私

- RAW 开发、图片调整、缩略图和机器学习检测在本机完成。
- 原始 NEF/JPEG 不因查看或调整而被直接修改。
- 删除操作优先使用应用回收站逻辑。
- 下载相机文件时由用户明确选择保存目录。
- 数据库、缓存和 `.nps` sidecar 属于用户运行数据，不提交到 Git。

## 已知限制

- 无法从已经失焦或低分辨率的原文件中恢复本来不存在的真实细节。
- RAW 色彩仍可能与 Nikon NX Studio 的专有 Picture Control 渲染存在差异。
- 8192px RAW 预览在高像素机型上会消耗较多内存，首次完整解码可能需要数秒。
- Wi‑Fi 连接依赖相机固件、电脑网络配置、配对状态及 PTP/IP 可达性。
- USB 枚举依赖 Windows 正确识别相机为便携式 MTP/PTP 设备。
- USB 模式目前显示文件信息并下载原文件；Windows MTP 不一定直接提供缩略图。
- 人像检测是轻量人脸检测方案，极小人脸、严重遮挡、背影和特殊角度可能归入“其他”。

## 每日更新

### 云创实验功能（2026-10-25）

预设管理页可以选择中国站或国际站，读取公开风格列表、查看原始优化校准参数，并另存为 JSON。两站配方 ID 独立；跨站链接会被拒绝。下载只请求尼康公开接口，不读取浏览器登录信息。

JSON 是公开参数资料，**不是可导入相机的 NP3**。`gamma` 表示公开响应没有给出曲线点；Flexible Color 的完整数据也不在该响应中。软件不会将这些缺失值静默替换成默认参数。新增的 NP3 结构检查器保留原始字节、未知字段与自定义曲线，并报告解析版本和字段覆盖情况；结构解析成功不等于可完整渲染，因此生产导入入口仍明确拒绝把 NP3/NCP 转为应用现有预设。

“检测 USB 相机”枚举 Windows WPD 存储设备；“检测官方 SDK”通过本地独立辅助程序只读检查 SDK 与设备能力。官方 Z30 文档包含传统自定义优化校准的 USB 命令，但 Z30 固件不支持 Flexible Color，不能因文件扩展名是 NP3 就认定可用。写入按钮保持禁用，尚未完成相机写入、读回及断开 USB 后的拍摄验证。

完整云创风格以电脑端显影为研究方向。Nikon SDK 属于需单独接受许可的软件，仓库和发布包不附带 SDK、官方配置文件、真实第三方 NP3 或研究照片；开发者工具位于 `tools/nikon-sdk` 与 `tools/nikon-image`。当前安装包尚未提供经过验证的完整云创风格渲染。

本机 Windows 10 已通过 Image SDK 1.47 基础显影实测：真实 Z30 NEF 输出 5568 × 3712 的 sRGB 图像，中文路径复测输出一致，原片哈希不变。显影工具须与 SDK 的 DLL 和数据文件同目录。此结果验证原拍摄设置显影，尚未验证云创 NP3 的完整风格匹配；步骤及证据见 [tools/nikon-image/README.md](tools/nikon-image/README.md)。

验证命令：`pnpm test`、`pnpm build`、在 `src-tauri` 下执行 `cargo test --bin nikon-photo-studio`。真实网络测试需要单独运行 `cargo test live_public_recipe_download -- --ignored --nocapture`，会访问两站并将元数据保存到临时目录。部分既有缓存测试需要正常的用户目录写权限；无需连接相机的测试不能证明硬件写入成功。

详细记录见 [CHANGELOG.md](CHANGELOG.md)。更新内容按日期倒序维护，每次功能提交应补充当天的新增、修复、验证和已知限制。

## GitHub 自动构建与软件发布

源码与安装包统一放在 [jzhijian/nikon-photo-studio](https://github.com/jzhijian/nikon-photo-studio)。推送 `main` 后，GitHub Actions 运行前后端测试并构建 Windows x64 安装包；全部成功后更新 [latest 预发布版](https://github.com/jzhijian/nikon-photo-studio/releases/tag/latest)，同时将 `latest` 标签指向实际构建的提交。构建失败时不发布新安装包。

正式版本使用 `vX.Y.Z` 标签，必须与 `package.json`、`src-tauri/Cargo.toml` 和 `tauri.conf.json` 的版本一致；已发布的正式版本不自动覆盖。每个发布包含安装程序、SHA-256 校验文件及记录提交号的构建清单。此流程更新 GitHub 下载文件，已安装应用尚不支持后台自动升级。操作说明见 [tools/release/README.md](tools/release/README.md)。

## License

项目代码使用仓库中的 MIT License。第三方依赖和模型文件遵循各自许可证，分发应用时应保留相应声明。
