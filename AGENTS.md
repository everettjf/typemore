# TypeMore 仓库协作指南（AGENTS）

## 1. 项目结构
本仓库是 **Tauri 桌面应用**，前端为 **React + TypeScript**。

- `src/`：前端应用代码（如 `App.tsx`、`main.tsx`、样式、资源）。
- `public/`：Vite 提供的静态资源。
- `src-tauri/src/`：Rust 后端命令与应用启动逻辑。
- `src-tauri/tauri.conf.json`：窗口、构建、运行时配置。
- `dist/`：前端构建产物（自动生成）。
- `src-tauri/target/`：Rust 构建产物（自动生成）。

## 2. 常用命令
在仓库根目录执行：

- `bun install`：安装前端依赖。
- `bun run dev`：仅启动 Vite 前端开发服务。
- `bun run tauri dev`：启动完整桌面应用（前端 + Tauri）。
- `bun run build`：执行 TypeScript 检查并构建前端生产包。
- `bun run tauri build`：构建桌面应用发行包。
- `cd src-tauri && cargo check`：检查 Rust 后端是否可编译。
- `cd src-tauri && cargo test`：运行 Rust 测试。

## 3. 代码风格
- 前端启用 TypeScript strict mode，保持无警告。
- 缩进：TS/CSS 使用 2 空格；Rust 使用 `cargo fmt`。
- React 组件与文件名：PascalCase（如 `RecordingList.tsx`）。
- 变量/函数：camelCase；常量：UPPER_SNAKE_CASE。
- Tauri 命令命名需清晰、动作导向（如 `init_model`、`transcribe_recording`）。

## 4. 前端库优先级
进行 UI 开发时优先复用成熟库，减少手写样式。

优先顺序：
1. Radix UI: `https://www.radix-ui.com/`
2. shadcn/ui: `https://ui.shadcn.com/`
3. Tailwind CSS: `https://tailwindcss.com/`
4. Chart.js（图表）: `https://www.chartjs.org/docs/latest/`

仅在库无法满足产品需求时再添加自定义 CSS。

## 5. 测试与验证
当前未配置专门的 JS 测试框架。

- 前端改动最少验证：`bun run build`
- 后端改动最少验证：`cd src-tauri && cargo check`
- Rust 单元测试应就近放在 `src-tauri/src/` 对应实现附近。
- 测试命名建议聚焦行为，例如：`transcribe_returns_text_for_valid_wav`。

## 6. 提交与 PR 规范
提交历史倾向简洁祈使句，常见 `iterNN:` 前缀。

- 推荐提交格式：`iterNN: <imperative summary>` 或 `<scope>: <imperative summary>`
- 每次提交只处理一个关注点。
- PR 需包含：变更摘要、动机、验证步骤、UI 改动截图。
- 若涉及配置变更，需明确指出 `src-tauri/tauri.conf.json` 或相关 capability 文件变更。
