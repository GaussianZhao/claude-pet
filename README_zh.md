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

## 状态检测原理（Hook 驱动的状态机）

没有官方「Claude 在干什么」的 API，因此状态由 **Claude Code 的生命周期 Hook** 驱动——这些 Hook 把每一轮对话「括起来」。每个会话取**最新一条** Hook 事件直接决定状态，我们信任事件，而不是靠文件时间去猜。

`hooks/claude-pet-hook.sh` 在每个事件时往 `~/.claude/claude-pet/sessions/<id>.json` 写一份；`monitor/hooks.rs` 读取，`monitor/mod.rs::hook_status()` 做映射：

| 最新 Hook 事件 | 状态 |
| --- | --- |
| `UserPromptSubmit`、`PreToolUse`、`PostToolUse`、`PostToolBatch`、`PermissionDenied` | **running（运行中）** |
| `PermissionRequest`、`Notification[permission_prompt]` | **waiting（等待确认）** |
| `Notification[idle_prompt]` | idle（空闲） |
| `Stop` | **completed（完成，粘滞至确认）** |
| `StopFailure`（API/限流错误） | **error（错误）** |
| `SessionStart`、`SessionEnd` | idle（空闲） |

关键特性：一轮任务从 `UserPromptSubmit` **持续 running 到终止事件**——在「思考中」或长耗时工具（如构建）静默运行期间**不会**被误判超时。两路辅助信号：

- **进程扫描** (`process.rs`) — 是否存在 `claude` 进程？→ OFFLINE / 在线。每 ~5 秒轮询。
- **会话 Transcript** (`session.rs`) — 读取最新的 `.jsonl` 获取项目名与任务标题。其**最后一条对话记录**（而非文件 mtime，因为打开窗口的元数据写入也会改 mtime）作为**兜底**活跃度信号，仅在某会话还没有 Hook（尚未安装）时使用。

> **关于 `waiting` 与桌面 App：** 「等待确认」依赖 `PermissionRequest` / `Notification`。终端版 Claude Code 会发这些事件；部分桌面 App 版本可能不发，此时真实的权限弹窗会显示为 `running` 而非 `waiting`。这是有意为之——之前的「挂起工具调用启发式」会把长耗时工具误判为「等待」，已移除。

---

## 安装 Claude Code Hooks（推荐）

安装脚本会把 Hook 脚本**拷贝到稳定位置**（`~/.claude/claude-pet/claude-pet-hook.sh`，与本仓库解耦），即使你移动或删除代码仓库，Hook 依然有效；并把事件注册合并进 `~/.claude/settings.json`：

```bash
./hooks/install-hooks.sh   # 安全、幂等；执行前会备份 settings.json
```

安装后请重启正在运行的 Claude / 桌面会话，使新注册的事件生效。

不装 Hook 时桌宠仍能从 Transcript 兜底显示 `离线 / 运行中 / 空闲`，但 `等待确认 / 完成 / 错误` 需要 Hook。

> 在环境变量中设置 `CLAUDE_PET_DEBUG=1` 可把每条 Hook 事件记录到 `~/.claude/claude-pet/events.log`（默认关闭；最多 400 行）。

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
| AC3 | 等待确认 → 挥手动画 + 系统通知            | `PermissionRequest`/`Notification` Hook → `animateWaiting` |
| AC4 | 完成 → 跳舞动画 + 系统通知               | `Stop` Hook → `animateCompleted`              |
| AC5 | 点击桌宠 → 卡片面板（每任务一张）         | `Pet.tsx` + `StatusPanel.tsx`                 |
| AC6 | 生成 macOS `.dmg` 安装包                  | `npm run app:build`                           |

## 性能

PRD 目标（CPU < 3%，内存 < 150MB）：monitor 每 5 秒做一次重扫描，每秒只读一次状态文件；空闲时动画降为 10fps，活跃时 30fps；Transcript 文件仅在 mtime 变化时才重新解析。

## 长期在本机运行的注意事项

如果打算一直装着、每天运行：

- **Hook 脚本位于 `~/.claude/`，不在本仓库。** `install-hooks.sh` 会把它拷贝到 `~/.claude/claude-pet/`，所以删除/移动代码仓库不会影响你的 Claude 会话。重复运行安装脚本是幂等的（不会产生重复条目）。
- **磁盘占用有界。** `~/.claude/claude-pet/sessions/` 下的会话状态文件超过 1 天会被清理；调试用 `events.log` **默认关闭**（通过 `CLAUDE_PET_DEBUG=1` 开启），且最多 400 行。
- **超大 Transcript。** 会话 `.jsonl` 在 mtime 变化时（活跃时约每 5 秒）会被整文件解析；超长会话（数十 MB）解析开销较大；按 mtime 缓存，空闲会话零开销。
- **更新 Hook。** 如果改了 `hooks/claude-pet-hook.sh`，需重新运行 `./hooks/install-hooks.sh` 把新版本拷贝进 `~/.claude/`。
- **升级 Claude Code。** Hook 写在用户 `settings.json` 里，升级后依然保留；新事件（`PermissionRequest`、`StopFailure` 等）在不支持的版本中会被忽略，无副作用。

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
