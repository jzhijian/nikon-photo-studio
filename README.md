# Nikon Photo Studio

Nikon Photo Studio 是一款面向 Nikon 相机用户的 Windows 桌面影像工作台，使用 Tauri、Rust、React 和 TypeScript 构建。项目将图库浏览、RAW/NEF 预览、照片整理、智能分类、模糊检测、预设管理和相机图传整合到一个本地应用中，核心处理无需上传照片。

> 当前版本：`0.1.0`。项目仍在持续开发，部分相机连接能力需要在真实 Nikon 设备上验证。

## 功能概览

### 图库与照片浏览

- 递归导入本地照片文件夹，支持 JPEG、PNG、TIFF 和 Nikon NEF。
- 文件夹、格式、标记和星级组合筛选。
- 可调节大小的照片网格和底部胶片条导航。
- 单选、多选、Shift 连选、全选、星级评分和标记。
- 双击进入大图查看，支持方向键切换、缩放、拖拽平移和原图对比。
- 显示文件尺寸、图像尺寸及常用 EXIF 信息。
- 删除操作移动到应用回收站，降低误删风险。

### RAW / NEF 高清预览

- 首屏优先读取相机内嵌 JPEG，快速显示可用预览。
- 后台通过 `rawler` 解码 RAW 传感器数据，完成去马赛克、白平衡、相机色彩矩阵和 sRGB 输出。
- 根据窗口尺寸、系统 DPI 和查看倍率，自适应选择高清预览分辨率。
- 使用 Lanczos 高质量缩放和轻量基础锐化改善 RAW 细节。
- 自动读取并应用 JPEG EXIF / NEF TIFF 方向信息。
- 缩略图和高清预览使用本地内容缓存，重复打开无需重复解码。

### 智能整理

- 基于 Laplacian 方差、梯度能量和高频信息计算清晰度分数。
- 提供严格、标准、宽松三档检测灵敏度及连续阈值调整。
- 按模糊照片、人像、风景和其他类别浏览整理结果。
- 支持批量选择、归档和删除。
- 内置离线 ONNX 模型资源：
  - UltraFace：轻量人脸检测。
  - SqueezeNet 1.1：ImageNet 图像语义分类。
- 模型在本机执行，不向云端上传用户照片。

### 非破坏性调色

- 曝光、对比度、高光、阴影、白色、黑色、色温和色调。
- 自然饱和度、饱和度、清晰度、纹理和去雾。
- 八色 HSL 调整、色彩分级和点曲线。
- 基于 Canvas 像素管线生成实时预览。
- 编辑参数可写入同名 `.nps` sidecar，原始照片不会被修改。
- 支持撤销、重置、原图对比和分屏对比。

### 预设管理

- 支持导入 XMP、CUBE、NP3、NCP、 Lightroom `.lrpreset`、3DL 和 Capture One `.costyle`。
- 支持预设筛选、分类、收藏、重命名和删除。
- 解析常见曝光、对比度、白平衡、饱和度、清晰度和锐化参数。
- 可将预设参数应用到当前照片并保存非破坏性 sidecar。

### Nikon 相机图传

- 包含 PTP/IP 通信层和 Nikon 相机命令实现。
- 面向 Nikon Z30 / Z5II 的 Wi-Fi 连接工作流。
- 读取相机照片列表和缩略图，支持批量下载队列。
- 预留 USB PTP/MTP 传输实现。

### 桌面体验

- Lightroom 风格深色布局：顶部模块栏、左右面板、中央工作区和胶片条。
- 图库、整理、预设、图传四个工作模块。
- 中英文界面资源。
- Windows 10/11 原生桌面打包，安装包使用 NSIS 生成。

## 技术架构

| 层级 | 技术 | 用途 |
| --- | --- | --- |
| 桌面容器 | Tauri 2 | 原生窗口、IPC、文件系统和安装包 |
| 前端 | React 18、TypeScript、Vite | 界面与交互 |
| 状态管理 | Zustand | 图库、整理、预设和图传状态 |
| 样式 | Tailwind CSS | 响应式桌面界面 |
| 后端 | Rust、Tokio | 图片处理、缓存、文件操作和相机通信 |
| RAW 解码 | rawler | Nikon NEF 解码与开发 |
| 图片处理 | image-rs | 方向修正、缩放和编码 |
| 数据 | SQLite / rusqlite | 本地元数据和编辑记录 |
| 智能分析 | 本地 ONNX 模型 | 人脸检测和图像语义分类 |
| 测试 | Vitest、Rust tests | 前端状态与后端核心逻辑验证 |

## 项目结构

```text
nikon-photo-studio/
├─ models/                 # 应用需要的轻量 ONNX 模型
├─ src/                    # React / TypeScript 前端
│  ├─ components/          # 通用 UI、预览器、缩略图与胶片条
│  ├─ modules/             # 图库、整理、预设、图传模块
│  ├─ stores/              # Zustand 状态
│  ├─ lib/                 # 前端图像处理管线
│  └─ i18n/                # 中英文语言资源
├─ src-tauri/              # Rust / Tauri 后端
│  ├─ src/camera/          # PTP/IP 与相机通信
│  ├─ src/library/         # 导入、缩略图、RAW 预览、EXIF
│  ├─ src/organize/        # 模糊检测与智能分类
│  ├─ src/presets/         # 预设解析与 sidecar
│  └─ src/fs/              # 扫描、归档和回收站
├─ test_presets/           # 小型预设解析样本
└─ tests/                  # 集成 / 端到端测试目录
```

## 开发环境

推荐环境：

- Windows 10 或 Windows 11
- Node.js 20+
- pnpm 9+
- Rust stable
- Tauri 2 的 Windows 构建依赖
  - Microsoft C++ Build Tools，或可用的 GNU/MinGW 工具链
  - Microsoft Edge WebView2

安装前端依赖：

```powershell
pnpm install
```

启动开发模式：

```powershell
pnpm tauri dev
```

仅启动前端：

```powershell
pnpm dev
```

## 测试与构建

运行前端测试：

```powershell
pnpm test
```

执行前端生产构建：

```powershell
pnpm build
```

检查 Rust 后端：

```powershell
cargo check --manifest-path src-tauri/Cargo.toml
```

生成 Windows NSIS 安装包：

```powershell
pnpm tauri build
```

默认产物位于：

```text
src-tauri/target/release/bundle/nsis/
```

构建产物、依赖目录、缓存、数据库、安装包和大型压缩文件均已通过 `.gitignore` 排除，不应提交到源码仓库。

## 本地数据与隐私

- 照片处理和智能分析默认在本机完成。
- 原始 NEF/JPEG 不会因调色操作被直接修改。
- 缩略图缓存默认保存在用户目录下的 `.nikon-photo-studio` 文件夹。
- `.nps` sidecar、SQLite 数据库和本地缓存均属于用户运行数据，不纳入 Git。

## 已知限制

- RAW 最终观感会受到相机型号支持、原始拍摄清晰度和当前开发管线的影响。
- 低分辨率或已经失焦的 JPEG 无法通过预览算法恢复不存在的真实细节。
- Wi-Fi / USB 图传需要针对具体相机固件和 Windows 驱动继续验证。
- 智能分类是本地轻量模型方案，目标是快速整理，不替代专业素材标注系统。

## License

项目代码使用仓库中的 MIT License。第三方库和模型文件遵循各自许可证；分发应用时请同时保留相应声明。

