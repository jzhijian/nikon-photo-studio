# Windows 自动发布

推送到 `main` 后，GitHub Actions 使用锁定的 pnpm 与 Cargo 依赖测试并构建 Windows x64 NSIS 安装包。成功后更新 [`latest` 预发布](https://github.com/jzhijian/nikon-photo-studio/releases/tag/latest)。下载页提供安装程序、`SHA256SUMS.txt` 和记录提交 SHA 的 `release-manifest.json`。这里的自动更新指发布页附件更新；已安装程序尚未接入应用内自动升级。

正式版本使用 `vX.Y.Z` 标签。先将 `package.json`、`src-tauri/Cargo.toml`、`src-tauri/tauri.conf.json` 三处版本设为相同的 `X.Y.Z`，更新 Cargo 锁文件并提交，再推送对应标签。工作流会拒绝覆盖已经发布的正式版本，也不会移动版本标签。中断产生的本工作流草稿可以重试；与当前提交不一致的草稿会被拒绝。此保护由工作流实施，不等于替仓库配置了 GitHub 全局 immutable releases 设置；滚动 `latest` 必须允许更新。

各发布渠道串行运行，上传阶段不会被后续推送取消。主分支构建在发布前再次检查最新提交，过期构建跳过上传。替换 `latest` 附件期间会暂时把该发布设为草稿；若上传失败，修复后重新运行该工作流即可。构建失败不会改动现有发布。

发布包仅包含应用安装程序。CI 不下载或接受 Nikon SDK 协议，不构建或打包 Nikon SDK 检测/显影辅助程序，不包含专有 SDK、第三方 NP3 或研究照片。需要 SDK 的功能仍须用户自行授权安装并完成本机验证，安装包不代表完整云创风格已经可用。GitHub 自动生成的源码归档来自 Git 跟踪文件，发布前仍须确保受限样本和 SDK 没有被提交。

CI 显式执行 Windows MSVC 构建，仓库中的 GNU 专属 linker 设置不会作用于这个目标：

```powershell
pnpm install --frozen-lockfile
pnpm test
cargo test --locked --manifest-path src-tauri/Cargo.toml --target x86_64-pc-windows-msvc
pnpm tauri build --ci --target x86_64-pc-windows-msvc --bundles nsis -- --locked
```

Action 引用固定到完整提交 SHA；构建 job 只有仓库读取权限，发布 job 才有内容写入权限。没有上传整个工作区或 Cargo 输出目录，只有三个明确列出的发布文件。

本地可运行 `pwsh -File tools/release/prepare.ps1 -Mode Validate` 检查版本和打包边界。`publish.ps1 -ValidateOnly` 只验证已生成附件，完全不访问 GitHub。实际发布必须由获授权的 GitHub Actions 环境执行。

参考：[Tauri Windows 打包](https://v2.tauri.app/distribute/windows-installer/)、[GitHub 发布 API](https://docs.github.com/en/rest/releases/releases)、[GitHub Git 引用 API](https://docs.github.com/en/rest/git/refs)。
