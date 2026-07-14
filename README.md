# Pet Desk

通用、轻量的 Windows 与 macOS 桌面宠物。v1 内置 `ikunchick`，安装后的应用不需要 Codex、Python 或 Node。

## 开发

需要 Node.js 22+ 和 Rust stable；Windows 还需要 WebView2。安装依赖后运行：

```bash
npm install
npm run tauri dev
npm test
```

主宠物窗口透明、无边框、无任务栏图标；关闭时隐藏到托盘。默认：鼠标穿透、置顶、允许拖动、开机自启均开启。鼠标穿透开启时窗口不接收鼠标事件，请先从托盘或设置关闭它才能拖动。

## Windows 打包、安装与卸载

在 Windows x64 环境执行：

```powershell
npm ci
npm run tauri build -- --bundles nsis
```

安装包输出在 `src-tauri/target/release/bundle/nsis/`，为 `.exe`。NSIS 安装程序从“应用和功能”或卸载程序卸载。安装后请验证：启动、托盘显示/隐藏、穿透切换、拖动、重启位置恢复、导入/删除宠物及开机自启。

## macOS 打包与安装

本地构建当前 Mac 架构的 DMG：

```bash
npm ci
npm run tauri build -- --bundles dmg
```

DMG 位于 `src-tauri/target/release/bundle/dmg/`。打开后将 Pet Desk 拖入“应用程序”。未进行 Apple 签名和公证的开发构建首次运行时，可能需要在 Finder 中右键应用并选择“打开”。

每次推送 `v*` Tag（例如 `v0.1.4`），或手动运行 GitHub Actions 时，Windows 与 macOS 会并行测试和打包。普通的 `main` 推送与 Pull Request 不会触发打包：

- `Pet-Desk-Windows-NSIS`：Windows x64 NSIS `.exe`。
- `Pet-Desk-macOS-Universal-DMG`：同时支持 Apple Silicon 和 Intel Mac 的 Universal `.dmg`。

两个 artifact 都保留 30 天。工作流不会自动创建 GitHub Release。

## 数据位置

Windows 用户导入的宠物和 `settings.json` 位于：

`%APPDATA%\\com.petdesk.app\\pets`

macOS 用户数据位于：

`~/Library/Application Support/com.petdesk.app/pets`

内置 `ikunchick` 由安装资源提供，永远不会被删除；用户自定义宠物只复制到上述数据目录，原始目录不会被引用。

## 宠物包规范

一个宠物就是一个独立目录：

```text
my-pet/
  pet.json
  spritesheet.webp
```

`pet.json`：

```json
{
  "id": "my-pet",
  "displayName": "My Pet",
  "description": "...",
  "spriteVersionNumber": 2,
  "spritesheetPath": "spritesheet.webp",
  "kind": "animal"
}
```

v1 只渲染 Codex v2 格式：透明 WebP、1536×2288、192×208 单格、8 列×11 行。行 0–8 分别为 idle、running-right、running-left、waving、jumping、failed、waiting、running（专注处理任务）、review；行 9–10 为从 000 到 337.5 度的 16 个视线方向。导入时会校验 manifest、文件、WebP 格式、透明通道、尺寸和版本；ID 冲突时可选择覆盖、自动重命名或取消。

`.zip` 导入在前端接口层保留，v1 的可用入口是选择宠物文件夹；后续只需把 zip 解压到临时目录后复用同一个 `import_pet` 命令即可。

## 替换内置资源

替换 `src-tauri/resources/builtin/pet.json` 与 `spritesheet.webp` 后重新打包即可。它们会被复制进应用安装资源，不会读取用户电脑上的 `.codex` 目录。

## 测试

`npm test` 覆盖精灵图坐标与 16 向角度映射；Rust 单元测试覆盖 manifest 失败路径。Tauri 后端的 `validate_package` 集中负责 manifest、图集尺寸和透明通道校验；导入冲突处理与默认设置持久化在命令层实现，适合在 Windows CI 再增加集成测试。
