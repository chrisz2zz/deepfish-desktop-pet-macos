# 蓝色大肥鱼桌宠（macOS / Tauri 2）

一只基于蓝色大肥鱼意象进行二次创作的 macOS 桌面宠物，使用 Tauri 2 + Rust 实现，是从 [deepfish-desktop-pet](https://github.com/Dolphin2026-stl/deepfish-desktop-pet)（Electron / Windows 版）独立出来的 macOS 版本。

她会跟随本地时间作息，能被拖动、点击、投喂和摸头，也会在你从休眠回来时随机伸懒腰或被惊醒。

> 本项目是社区二创，不隶属于、也不代表 DeepSeek 官方。角色立绘沿用上游项目的创作素材；如需公开分发，请确认你拥有对应素材的使用与再授权权利。

## 功能

- 透明、无边框、置顶的 `360 × 560` 桌宠窗口，靠近屏幕边缘不会拉伸气泡
- 按立绘轮廓穿透点击：空白区域的点击直接落到后面的窗口，立绘范围内照常拖动、点击和右键
- 菜单栏常驻图标，不占用 Dock；左键显示/隐藏，右键打开菜单
- 原生右键菜单：聊天、投喂、摸头、陪玩、置顶、设置、隐藏、退出
- 待机呼吸浮动与 2.8-7 秒不定时整帧眨眼
- 点击、拖拽、鼠标悬停和键盘操作反馈；快速绕圈拖动会转晕
- 生理时钟：清晨、专注、午饭、下午、晚间、睡眠七段状态
- 自动散步、洗碗、加班喝咖啡、抱鲸鱼玩偶、坐下和睡觉场景
- 头发、脸蛋、腿部区域感应：卖萌、害羞、绊倒，连续绊倒后会拿手帕哭哭
- 随机闲置彩蛋与 31 张固定锚点 PNG 表情、道具和动作帧
- 休眠恢复检测，醒来后随机触发“伸懒腰”或“惊醒”
- Agent harness：多会话管理、上下文预算与自动压缩、流式输出、工具调用与后台多步任务
- 气泡式聊天：不遮挡桌宠、限制最大高度、底色透明，可切换/新建/删除会话
- 默认使用免 Key 的 Pollinations OpenAI 兼容接口，可切换 DeepSeek 或任意 OpenAI 兼容 API
- API Key 只保存在本机的设置文件里（权限 0600），不写入系统钥匙串，避免申请访问其他密钥的权限

## 环境要求

- macOS 10.15 或更高版本
- Xcode Command Line Tools：`xcode-select --install`
- Rust stable：<https://rustup.rs>
- Node.js 20 或更高版本

## 快速开始

```bash
npm install
npm run dev      # 开发模式：透明置顶窗口 + 菜单栏图标
npm run build    # 打包 .app 与 .dmg
```

构建产物：

- `src-tauri/target/release/bundle/macos/大肥鱼桌宠.app`
- `src-tauri/target/release/bundle/dmg/大肥鱼桌宠_<版本>_<架构>.dmg`

两个命令都会先执行 `npm run web:prepare`，把 `src/renderer`、`src/shared` 与 `assets` 复制到 `build/web`，作为 Tauri 的静态资源根目录。

应用以 `Accessory` 激活策略运行（`Info.plist` 同时设置了 `LSUIElement`）：只在菜单栏显示图标，不占用 Dock。退出请使用菜单栏或右键菜单中的“退出大肥鱼”。

## 交互方式

| 操作 | 反馈 |
| --- | --- |
| 鼠标悬停 | 抬头轻晃并显示当前作息台词 |
| 散步时经过头发 | 随机眯眼卖萌或自信 pose |
| 散步时经过脸蛋 | 捂脸害羞、脸红 |
| 散步时经过腿 | 绊倒；短时间多次绊倒后会拿手帕哭哭 |
| 单击 | 触发点击台词和爱心粒子 |
| 点击头部 💬 | 打开气泡聊天（顶栏可切换/新建会话、手动压缩上下文） |
| 按住拖动 | 移动桌宠，松手后给出拖拽反馈 |
| 快速绕圈拖动 | 松手后触发晕眩、转圈星星与专属台词 |
| 右键 | 回应“干嘛？”并打开原生菜单 |
| `Enter` / 空格 | 聚焦宠物后打开聊天 |

## 模型设置

右键桌宠并选择“设置”。

### 免费模型（默认）

- API 地址：`https://text.pollinations.ai/openai`
- 模型：`openai-fast`
- 不要求 API Key

免费公共服务可能有频率限制、可用性波动或隐私条款变化。不要发送密码、身份证号、内部代码等敏感内容。

### DeepSeek

- API 地址：`https://api.deepseek.com/chat/completions`
- 模型：`deepseek-chat`
- 在设置页填写自己的 API Key

### 自定义 OpenAI 兼容服务

选择“自定义”，填写完整的 HTTPS Chat Completions 地址、模型名与可选 API Key。请求会带上 `stream: true`，按 Server-Sent Events 逐段渲染；如果服务端忽略该参数并直接返回完整 JSON，也会退化为一次性显示整段回复。

## 开发与验证

```bash
npm test           # Node 内置测试：作息、行为目录、手势与平台桥接
npm run test:rust  # Rust 测试：聊天请求构造与设置校验
npm run check      # 检查关键文件存在且脚本语法有效
npm run smoke      # 启动已打包的 .app 做端到端自检（需先 npm run build）
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
```

Rust 侧在 `Cargo.toml` 的 `[lints.clippy]` 里开了 pedantic（放开了数值转换、文档必填、Tauri 命令参数必须 owned 这几类噪音），因此 `-D warnings` 应该保持零警告。

`npm run smoke` 会把结果写入 `work/smoke-macos/`，校验桌宠帧是否加载、`window.deepFish` 桥接是否可用、气泡聊天与流式接口是否就绪、穿透点击蒙版是否上报、设置窗口是否渲染并能经 Rust 后端保存设置，然后自动退出。

需要联网验证真实模型请求时：

```bash
npm run web:prepare
cargo test --manifest-path src-tauri/Cargo.toml -- --ignored
```

## 项目结构

```text
deepfish-desktop-pet-macos/
├─ assets/                 # 立绘与固定锚点动作帧
├─ scripts/                # 静态资源准备、静态检查与冒烟测试
├─ src/
│  ├─ renderer/            # 桌宠、聊天与设置界面
│  └─ shared/              # 作息、行为目录、手势与 Tauri 桥接
├─ src-tauri/
│  ├─ src/                 # Rust 后端：窗口、菜单栏、菜单、设置
│  │  └─ agent/            # Agent harness：会话、上下文压缩、工具循环
│  ├─ capabilities/        # Tauri 权限
│  ├─ icons/               # 应用图标与菜单栏图标
│  ├─ Info.plist           # LSUIElement 等 macOS 配置
│  └─ tauri.conf.json      # 窗口、CSP、打包配置
└─ test/                   # Node.js 内置测试
```

`src/shared/desktop-bridge.js` 用 `window.__TAURI__` 暴露与上游 Electron preload 完全一致的 `window.deepFish` API，因此界面代码与上游保持一致，只有平台桥接不同。

素材制作脚本（`prepare-generated-frame.py` 等）与 Windows 启动器保留在上游仓库；本仓库直接使用已经处理好的 PNG 帧。替换角色时请保持每张画布尺寸与脚底锚点一致，并同步更新 `src/renderer/app.js` 中的 `frameNames`。

## 隐私与安全

- 渲染页面通过最小化桥接调用 Rust 命令，自定义命令按窗口能力配置开放。
- 自定义 API 地址必须使用 HTTPS。
- 模型请求由 Rust 后端发起，API Key 不暴露给页面脚本。
- 设置（含 API Key）保存在 `~/Library/Application Support/com.deepfish.desktop-pet/settings.json`，文件权限为 `0600`，只有当前用户可读；旧版本存在系统钥匙串里的 Key 不会自动迁移，需要重新填写一次。
- 会话历史保存在同一目录的 `agent/sessions.json`，只在本机；模型只能调用白名单工具，文件与命令访问被限制在用户主目录内，并按风险等级自动执行或要求确认（见下节）。
- 公共免费模型的请求会离开本机，请先阅读服务提供方条款。

## Agent 能力

聊天由 `src-tauri/src/agent/` 里的 harness 驱动，页面只负责渲染事件。

- **多会话**：会话保存在 `~/Library/Application Support/com.deepfish.desktop-pet/agent/sessions.json`（原子写入，最多 40 个会话、每个 400 条消息）。顶栏显示当前会话，点标题切换、`＋` 新建、`×` 两段确认删除；标题会按第一条用户消息自动生成，也可由模型通过 `rename_session` 改动。
- **上下文管理**：每次请求按「系统提示 + 压缩摘要 + 最近消息」组装，用启发式 token 估算（CJK 1 字 ≈ 1 token，其它 4 字符 ≈ 1 token）控制在 6000 token 以内（超过后压缩到约 2400，仍保留最近 80 条消息），并保证工具结果不会与对应的调用被拆开。
- **压缩**：超出预算时把较早的对话交给模型压缩成 ≤200 字摘要，成功后原始消息仍保留在本地供回看；摘要请求失败时退化为本地生成的确定性摘要，保证上下文一定会缩短。顶栏 `⤓` 可手动触发。
- **工具调用**：harness 以 `tools` + `tool_choice: auto` 发起请求，每一步都先过风险分级再决定自动执行、等审批还是直接拒绝。
- **防死循环**：设置页「Agent 上限」可改两个全局配置——**单次问答循环上限**（默认 50 轮）和**单次问答工具调用上限**（默认 10 次），Rust 侧硬夹取到 1–200 / 1–100。循环到顶后会再发一次"不带工具"的请求强制收尾；工具额度用完后，模型再发起的调用不执行、只回一条"超额度"结果（保证每个 `tool_call` 都有对应的 tool 消息，不会让接口报错），聊天里会显示一条黄色提示说明是哪个上限拦下的。
- **工具清单**：

  | 工具 | 能力 | 默认风险 |
  | --- | --- | --- |
  | `get_time` / `pet_behavior` / `pet_say` / `open_settings` / `rename_session` | 桌宠自身：时间、18 种动作白名单、气泡台词、打开设置、改标题 | 自动执行 |
  | `list_dir` | 列出主目录内某个目录 | 自动执行（敏感目录需审批） |
  | `read_file` | 读取主目录内文本文件（默认 200 行、单次最多 400 行 / 200 KB） | 自动执行（敏感目录需审批） |
  | `write_file` | 覆盖或新建文件，自动建父目录，单次上限 200 KB | **每次都要审批** |
  | `run_command` | 在 `sh -c` 里执行命令，默认 15 s 超时、最长 60 s，输出截断到 6000 字符 | 只读命令自动执行，其它需审批 |

- **作用范围**：文件工具只能触碰 `$HOME` 之内（`~` / `~/x` 写法，`..` 逃逸会直接拒绝）；`DEEPFISH_TOOL_ROOT` 可整体收窄或放宽这个根目录。命令无法真正沙箱化，所以"超出主目录"的路径、重定向、命令替换、`&&`/`;` 一律升级为审批。
- **风险分级**（`src-tauri/src/agent/risk.rs`）：
  - **自动执行**：`ls / pwd / cat / head / tail / wc / grep / rg / find / stat / du / df / jq / diff …` 这类只读命令，以及 `git status|diff|log|show|branch|rev-parse …`、`node --version`；管道里的每一段都必须是只读命令，且不能引用凭据路径。
  - **需要审批**：`write_file`、所有写操作、未知程序（`npm test`、`sed -i`、`find -delete`、`git push`、`rm -rf build`…），以及读取 `.ssh`、`.aws`、`.npmrc`、钥匙串、本应用设置等敏感路径。审批卡片出现在聊天里，显示将要执行的具体内容，`允许` / `拒绝` 二选一，**120 秒不点视为拒绝**；每次都问，不记忆。
  - **直接拒绝**：`sudo`/`su`、`shutdown`/`reboot`/`diskutil`/`mkfs`/`dd`、`security`（钥匙串命令行）、`launchctl`/`csrutil`/`nvram`/`tccutil`、`osascript`，以及 `curl`/`wget`/`ssh`/`scp`/`nc` 等外网与远程工具——避免读完本地文件再发出去。
- **被拒绝之后**：模型会收到一条明确的工具结果（"用户拒绝了这次操作" / "该操作被安全策略禁止"），系统提示要求它不要重试同一调用，改换方式或直接说明做不到。
- **后台任务**：一次回复的多步执行完全在 Rust 侧（工具在阻塞线程池里跑，不会卡住其它会话的流式输出），关闭聊天、切换会话都不会打断；结果写入会话，重新打开即能看到。命令超时上限 600 s，单次请求最多 8 步工具循环，审批等待窗口 300 s。
- **桌宠通知气泡**：后台任务不看聊天窗口也能知道进展——需要审批时宠物头顶弹出「🔐 需要审批 / 主人，有一个操作需要审批」，任务结束弹出「✅ 任务完成 / 主人，任务完成了」或「⚠ 任务失败 / 主人，任务失败了」。这个气泡是深色卡片、左侧彩色强调条、没有小尾巴，和聊天气泡、动作台词气泡都不一样；点它直接打开对应会话（需要审批时卡片已经在那儿等着）。聊天窗口正开着且就是该会话时不再弹气泡，避免重复打扰。

## 实现说明

- 穿透点击由渲染层把当前帧的 alpha 通道采样成 8 px 网格（`set_hit_mask`），Rust 侧以 40 ms 轮询光标并按网格切换窗口的鼠标穿透；拖动期间整窗临时保持可交互。
- 气泡区高度跟随宠物缩放后的真实高度，对话与动作台词都紧贴头顶；聊天打开时不再弹动作台词，避免两块气泡叠在一起。

## 已知限制

- 只检测休眠恢复，不区分锁屏与解锁。
- 穿透蒙版按窗口上次移动时的位置缓存，运行中改变显示器排列或缩放比例后需要重新拖动一次桌宠才能完全对齐。
- 上下文预算按字符估算 token，与真实分词存在偏差；压缩质量取决于所用模型，免费模型偶尔会忽略工具调用。
- 未配置代码签名与公证，本地首次打开需右键“打开”；分发前请自行配置签名。
- 界面代码当前与上游 Electron 版本各自维护，上游界面更新需要手动同步。

## 许可

源代码使用 [MIT License](./LICENSE)。角色立绘不因源代码许可自动获得再授权；公开发布前请单独确认素材权利。
