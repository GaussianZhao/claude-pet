# Claude Pet 🤖

**[English](README.md)**

一个 macOS 桌面像素宠物，实时展示 **Claude Code 当前工作状态**，并在 Claude 需要你介入时主动提醒——让你无需盯着终端也能了解任务进展。

<img src="docs/states/running.png" width="160" alt="Claude Pet – 运行中">

---

## 它能做什么

一个小型、置顶、透明背景的桌宠悬浮在桌面上。它的动画反映 Claude Code 当前的工作状态。**点击桌宠**可以在其上方弹出卡片面板——每个活跃任务一张卡片。可以把桌宠拖到屏幕任何位置。

### 状态动画

| Claude 状态        | 宠物动画                          | 截图                                                |
| ------------------ | --------------------------------- | --------------------------------------------------- |
| `idle`（空闲）     | 轻柔站立摇晃                      | <img src="docs/states/idle.png" width="90">         |
| `running`（运行中）| 双臂交替捶击（敲代码）            | <img src="docs/states/running.png" width="90">      |
| `waiting`（等待确认）| 右臂挥手 + "!" 对话气泡          | <img src="docs/states/waiting.png" width="90">      |
| `completed`（完成）| 左右摇摆跳舞 + 飘浮音符           | <img src="docs/states/completed.png" width="90">    |
| `error`（错误）    | 弯腰低头 + 落下眼泪               | <img src="docs/states/error.png" width="90">        |
| *Claude 未运行*    | 宠物变暗淡                        | <img src="docs/states/offline.png" width="90">      |

> 截图存放于 `docs/states/`。运行 `npm run dev` 可在浏览器中看到 Mock 状态循环，方便截图（每 3.5 秒切换一次状态）。

当 Claude **进入等待确认** 或 **完成任务** 时，会触发 macOS 原生通知——即使桌宠被其他窗口遮住也能收到。

### 任务卡片面板

面板从桌宠**上方**弹出（窗口向上生长，桌宠不动）。每个活跃任务对应一张横条卡片，显示项目名、状态徽章、任务名、时间。任务较多时卡片区可滚动；空闲超过 10 分钟的任务自动隐藏。

**完成态粘滞：** 完成的任务卡片会轻微脉冲闪烁，桌宠持续跳舞，直到你**点击该卡片确认**才退出——确保你不会错过已完成的任务。点击卡片同时会把 Claude 桌面应用或终端唤到前台。

### 菜单栏托盘

应用常驻菜单栏（可设置无 Dock 图标）。托盘菜单实时显示状态，并提供 **显示/隐藏桌宠**、**总在最前**、**开机自启**、**退出** 等选项。

### 诊断工具

```bash
"Claude Pet.app/Contents/MacOS/claude-pet" --probe
```

对当前真实环境执行一次扫描，以 JSON 格式打印融合后的 `ClaudeState`——便于验证检测是否正常工作，无需打开 UI。

---

## 状态检测原理（混合方案）

在没有官方 API 的情况下，我们融合三路信号（`src-tauri/src/monitor/`）：

1. **进程扫描** (`process.rs`) — 是否存在 `claude` CLI 或桌面进程？→ OFFLINE / 在线。每 ~5 秒轮询一次。
2. **会话 Transcript** (`session.rs`) — 读取最新的 `~/.claude/projects/<cwd>/<session>.jsonl`，获取项目名、任务标题和活跃度（文件最后追加时间）。
3. **Hook 推送文件** (`hooks.rs`) — `~/.claude/claude-pet/sessions/<id>.json`，Claude Code Hook 在 `Stop`/工具调用事件时写入。`COMPLETED` 状态可即时感知。
4. **挂起工具调用启发式** — Transcript 最后一条是 assistant 发出的 `tool_use` 且没有对应 `tool_result`，且已安静 ≥5 秒 → 判定为 `WAITING`。这是唯一能捕捉 **桌面 App 权限确认弹窗** 的方式（桌面 App 不会触发 `Notification` Hook）。

`monitor/mod.rs::compute()` 融合这几路信号：新鲜的 Hook 事件优先，但更新的 Transcript 活动可以覆盖（例如批准后继续工作）。

---

## 安装 Claude Code Hooks（推荐）

不装 Hook 时，桌宠仍然可以显示 `离线 / 运行中 / 空闲 / 等待确认`（基于挂起工具调用启发式）。装了 Hook 之后还能可靠地感知 `completed`，并使用 Hook 时间戳作为状态切换时刻。

```bash
./hooks/install-hooks.sh   # 安全合并进 ~/.claude/settings.json（会备份原文件）
```

安装后重启所有正在运行的 Claude 会话，使 Hook 生效。

---

## 开发

```bash
npm install
npm run app:dev      # tauri dev —— 带热重载启动桌宠
```

单独运行 `npm run dev` 会在浏览器中打开 Web UI；没有 Tauri 运行时时，会自动进入 **Demo 循环**，依次切换所有状态，方便预览动画效果。

## 构建安装包（.dmg）

```bash
# 一次性操作：从源 PNG 生成完整图标集
cargo tauri icon src-tauri/icons/source.png

npm run app:build    # dmg 生成在 src-tauri/target/release/bundle/dmg/
```

> ⚠️ 注意：`tauri build` 会将前端嵌入到 app 里。单独运行 `cargo build --release` 只生成开发模式二进制（加载 `localhost:1420`，会显示白屏）——请始终通过打包好的 `.app` 测试。

---

## 验收标准

| ID  | 标准                                      | 位置                                          |
| --- | ----------------------------------------- | --------------------------------------------- |
| AC1 | 启动后显示桌宠                            | `tauri.conf.json` + `App.tsx`                 |
| AC2 | 运行时 → 双臂捶击动画                     | `process.rs` + `PixelPet.animateRunning`      |
| AC3 | 等待确认 → 挥手动画 + 系统通知            | 挂起工具调用启发式 → `animateWaiting`         |
| AC4 | 完成 → 跳舞动画 + 系统通知               | `Stop` Hook → `animateCompleted`              |
| AC5 | 点击桌宠 → 卡片面板（每任务一张）         | `Pet.tsx` + `StatusPanel.tsx`                 |
| AC6 | 生成 macOS `.dmg` 安装包                  | `npm run app:build`                           |

## 性能

PRD 目标（CPU < 3%，内存 < 150MB）：monitor 每 5 秒做一次重扫描，每秒只读一次状态文件；空闲时动画降为 10fps，活跃时 30fps；Transcript 文件仅在 mtime 变化时才重新解析。

---

## 技术栈

| 层级   | 技术                             |
|--------|----------------------------------|
| UI     | React + TypeScript + Vite        |
| 渲染器 | PixiJS v8（WebGL，程序化绘制）   |
| 桌面框架| Tauri v2                        |
| 后端   | Rust                             |
| 通知   | macOS Notification Center        |

---

## 后续版本规划

- V1.1 Token / 配额显示 · 今日 Token 统计
- V1.2 GitHub Copilot 支持
- V1.3 Cursor 支持
- V2.0 多 Agent 统一监控中心
