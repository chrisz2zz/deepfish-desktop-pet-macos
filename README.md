# 蓝色大肥鱼桌宠（macOS / Tauri 2）

一只基于蓝色大肥鱼意象进行二次创作的 macOS 桌面宠物，使用 Tauri 2 + Rust 实现，是从 [deepfish-desktop-pet](https://github.com/Dolphin2026-stl/deepfish-desktop-pet)（Electron / Windows 版）独立出来的 macOS 版本。

她会跟随本地时间作息，能被拖动、点击、投喂和摸头，也会在你从休眠回来时随机伸懒腰或被惊醒。

> 本项目是社区二创，不隶属于、也不代表 DeepSeek 官方。角色立绘沿用上游项目的创作素材；如需公开分发，请确认你拥有对应素材的使用与再授权权利。

## 功能

- 透明、无边框、置顶的 `360 × 560` 桌宠窗口，靠近屏幕边缘不会拉伸气泡；拖动和自动散步都能跨到其它显示器
- 按立绘轮廓穿透点击：空白区域的点击直接落到后面的窗口，立绘范围内照常拖动、点击和右键
- 菜单栏常驻图标，不占用 Dock；左键显示/隐藏，右键是「显示/隐藏、和她聊天、设置、退出」
- 原生右键菜单：聊天、投喂、摸头、陪玩、置顶、设置、隐藏、退出
- 待机呼吸浮动与 2.8-7 秒不定时整帧眨眼
- 点击、拖拽、鼠标悬停和键盘操作反馈；快速绕圈拖动会转晕
- 生理时钟：清晨、专注、午饭、下午、晚间、睡眠七段状态
- 自动散步、洗碗、加班喝咖啡、抱鲸鱼玩偶、坐下和睡觉场景
- 头发、脸蛋、腿部区域感应：卖萌、害羞、绊倒，连续绊倒后会拿手帕哭哭
- 随机闲置彩蛋与 31 张固定锚点 PNG 表情、道具和动作帧
- 休眠恢复检测，醒来后随机触发“伸懒腰”或“惊醒”
- Agent harness：多会话管理、上下文预算与自动压缩、流式输出、工具调用与后台多步任务
- 命令在系统沙箱（Seatbelt）内执行：只读命令禁网、只允许写当前目录与构建缓存，越界访问由内核直接拒绝
- 后台任务有专属通知气泡（需要审批 / 完成 / 失败），样式与聊天气泡、台词气泡区分开
- 气泡式聊天：不遮挡桌宠、限制最大高度、底色透明，可切换/新建/删除会话
- 默认使用免 Key 的 Pollinations OpenAI 兼容接口，可切换 DeepSeek 或任意 OpenAI 兼容 API
- 可选开启 DeepSeek 的联网搜索（Search）与网页抓取（Fetch），让桌宠能查实时信息
- API Key 只保存在本机的设置文件里（权限 0600），不写入系统钥匙串，避免申请访问其他密钥的权限

## 环境要求

- macOS 10.15 或更高版本
- Xcode Command Line Tools：`xcode-select --install`
- Rust stable：<https://rustup.rs>
- Node.js 20 或更高版本（`npm test` 使用内置的 `node --test`）

以上仅针对 macOS；在 Windows / Linux 上构建的可行性分析、打包命令与未验证风险见 [Windows / Linux 可行性与打包](#windows--linux-可行性与打包)。

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
| 点击通知气泡 | 打开对应会话（后台任务需要审批时卡片已在其中） |

## 模型设置

右键桌宠并选择“设置”。

### 免费模型（默认）

- API 地址：`https://text.pollinations.ai/openai`
- 模型：`openai-fast`
- 不要求 API Key

免费公共服务可能有频率限制、可用性波动或隐私条款变化。不要发送密码、身份证号、内部代码等敏感内容。

### DeepSeek

- API 地址：`https://api.deepseek.com/chat/completions`（官方文档里的 base_url 是 `https://api.deepseek.com`；`/v1/chat/completions` 只是兼容写法，旧配置里出现这个形式会在启动时自动改回上面的地址）
- 模型：`deepseek-chat`（想省钱可改成账号可用的其它模型名，如 `deepseek-reasoner`）
- 在设置页填写自己的 API Key

#### 联网搜索与网页抓取（可选，默认关闭）

设置页的 DeepSeek 区域有两个开关，打开后模型会多出两个工具：

- `web_search` → `POST https://api.deepseek.com/v1/tools/search`，返回标题、链接、发布日期与摘要（支持 `site:` / `lang:` / `after:` 等搜索语法）
- `web_fetch` → `POST https://api.deepseek.com/v1/tools/fetch`，把单个网页抓成 Markdown 正文

两点提醒：

- **按成功请求计费**（Search / Fetch 各 ¥5 / 1000 次，失败、无结果、超时不计费）；`count` 只影响返回条数，不改价格。
- 开启后模型会把**查询词和链接**发给 DeepSeek 的 `/v1/tools/*` 接口——这正是你开启的意图，但也意味着这段内容离开了本机。工具结果会截断到约 4000 字符，避免一次搜索吃掉整个上下文预算。

### 自定义 OpenAI 兼容服务

选择“自定义”，填写完整的 HTTPS Chat Completions 地址、模型名与可选 API Key。请求会带上 `stream: true`，按 Server-Sent Events 逐段渲染；如果服务端忽略该参数并直接返回完整 JSON，也会退化为一次性显示整段回复。

## 开发与验证

```bash
npm test           # Node 内置测试：作息、行为目录、手势与平台桥接
npm run test:rust  # Rust 测试：会话存储、上下文压缩、风险分级、工具执行、SSE 解析与设置校验
npm run check      # 检查关键文件存在且脚本语法有效
npm run smoke      # 启动已打包的 .app 做端到端自检（需先 npm run build；
                   # 也可以 DEEPFISH_BINARY=src-tauri/target/debug/deepfish-desktop-pet npm run smoke 用调试版跑）
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
```

Rust 侧在 `Cargo.toml` 的 `[lints.clippy]` 里开了 pedantic（放开了数值转换、文档必填、Tauri 命令参数必须 owned 这几类噪音），因此 `-D warnings` 应该保持零警告。

`npm run smoke` 会把结果写入 `work/smoke-macos/`，校验桌宠帧是否加载、`window.deepFish` 桥接是否可用、气泡聊天与流式接口是否就绪、穿透点击蒙版是否上报、设置窗口是否渲染并能经 Rust 后端保存设置，然后自动退出。

需要联网验证真实模型请求时：

```bash
npm run web:prepare
cargo test --manifest-path src-tauri/Cargo.toml -- --ignored
```

其中 `agent::web::tests::live_search_and_fetch` 会用本机设置里的 DeepSeek Key 真跑一次 Search + Fetch（会计费），只想验证模型对话可以单独过滤：

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib agent::web -- --ignored
```

## Windows / Linux 可行性与打包

> ⚠️ **本项目只在 macOS 上开发和验证过。Windows 与 Linux 版本没有在真实设备上运行验证过**，下面是根据代码判断的迁移可行性，不保证开箱即用。实际效果很可能需要针对性修改，欢迎在对应平台上测试并提 PR。

### 代码层面现状

| 能力 | Windows | Linux | 说明 |
| --- | --- | --- | --- |
| 构建（Tauri 2 + Rust + 原生 HTML/JS） | 可行 | 可行 | 前端没有 macOS 专属代码；`objc2-app-kit` 依赖只在 `[target.'cfg(target_os = "macos")']` 下编译 |
| 透明、无边框、置顶、隐藏任务栏 | 大概率可行 | 取决于合成器 | 窗口已是 `transparent + decorations:false`；Linux 需要合成器（X11 + picom / KWin 等），Wayland 下透明与穿透行为不可控 |
| 按立绘轮廓穿透点击 | **未实现** | **未实现** | 光标查询只有 macOS 实现（`NSEvent.mouseLocation`），其它平台 `cursor_points()` 返回 `None`，整窗保持可点击——透明区域会挡住后面的窗口。移植思路：Windows 用 `GetCursorPos` + `WS_EX_TRANSPARENT`/`SetWindowRgn`，Linux X11 用 `XQueryPointer` + shape 扩展（Wayland 无对应能力） |
| 拖动、自动散步、跨屏夹取 | 可行 | 可行 | 只用 `outer_position` / `set_position` / `available_monitors` |
| 菜单栏 / 托盘图标 | 可行 | 依赖 libayatana-appindicator | 托盘图标用 `icons/tray.png`；GNOME 需要装 AppIndicator 扩展才显示 |
| 休眠恢复检测 | 可行 | 可行 | 纯 `SystemTime` 时间差，无平台 API |
| 设置与会话存储 | 可行 | 可行 | 路径来自 `app_config_dir()`（Windows 在 `%APPDATA%\com.deepfish.desktop-pet`，Linux 在 `~/.config/com.deepfish.desktop-pet`）；`0600` 权限只在 unix 生效，Windows 依赖用户目录 ACL |
| 命令工具 `run_command` | **需要改代码** | 可行 | 现在固定用 `sh -c`，Windows 要改成 `cmd /c` 或 PowerShell；风险白名单（`ls`/`cat`/`grep`/`git status`）也是 POSIX 命令，Windows 原生没有 |
| 命令沙箱（Seatbelt） | **无对应实现** | **无对应实现** | 依赖 macOS 的 `sandbox-exec`；其它平台会自动跳过沙箱（不会因此拒绝执行），但也就没有这层防护——移植时应对接 Windows Job Object/AppContainer 与 Linux 的 Landlock/seccomp |
| `Accessory` 激活策略 | 不需要 | 不需要 | 已用 `#[cfg(target_os = "macos")]` 隔离 |

### 打包方式

Tauri **不支持交叉编译安装包**，需要在目标系统上各构建一次（或在 CI 里跑矩阵）。两个平台都需要先把 `src-tauri/tauri.conf.json` 里的 `bundle.targets` 从当前的 `["app", "dmg"]` 改成目标格式。

**Windows**

```bash
# 依赖：Rust MSVC 工具链、Visual Studio Build Tools（C++ 桌面开发）、WebView2 Runtime（Win10 1803+ 通常自带）
# bundle.targets 改成 ["nsis"]（msi 需要额外装 WiX）
npm install
npx tauri build --bundles nsis
# 产物：src-tauri/target/release/bundle/nsis/*-setup.exe（msi 在 bundle/msi/）
```

**Linux（Debian / Ubuntu）**

```bash
sudo apt update && sudo apt install -y libwebkit2gtk-4.1-dev build-essential curl wget file \
  libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev
# bundle.targets 改成 ["deb", "appimage"]
npm install
npx tauri build --bundles deb,appimage
# 产物：bundle/deb/*.deb 与 bundle/appimage/*.AppImage（AppImage 运行需要 libfuse2）
```

**用 CI 出三平台产物**（没有设备时最省事，但仍属于未人工验证的构建）：

```yaml
# .github/workflows/bundle.yml（示意，需自行调整权限与产物上传）
jobs:
  bundle:
    strategy:
      matrix:
        platform: [macos-latest, ubuntu-22.04, windows-latest]
    runs-on: ${{ matrix.platform }}
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-node@v4
        with: { node-version: 20 }
      - uses: dtolnay/rust-toolchain@stable
      - uses: tauri-apps/tauri-action@v0
        with: { projectPath: "src-tauri" }
```

### 未验证的风险点

- 透明窗口在 Windows 上可能出现过黑/整窗不透明（需要无边框 + DWM 合成），Linux 无合成器时干脆不透明。
- 穿透点击在非 macOS 上不生效，桌宠的透明区域会挡住后面窗口的点击。
- 高 DPI 缩放（150% / 200%）下气泡位置与 `--pet-height` 计算没有校准，可能偏移。
- 中文字体回退（缺 `PingFang SC` / 微软雅黑时）可能影响气泡排版。
- 命令工具与风险分级按 POSIX 假设编写，Windows 上要适配。
- 托盘图标在部分桌面环境（尤其 Wayland）可能不显示。

## 项目结构

```text
deepfish-desktop-pet-macos/
├─ assets/                 # 立绘与固定锚点动作帧
├─ scripts/                # 静态资源准备、静态检查与冒烟测试
├─ src/
│  ├─ renderer/            # 桌宠、聊天与设置界面
│  └─ shared/              # 作息、行为目录、手势与 Tauri 桥接
├─ src-tauri/
│  ├─ src/                 # Rust 后端：窗口、菜单栏、穿透点击、聊天传输、设置
│  │  └─ agent/            # Agent harness：会话、上下文压缩、风险分级、工具循环
│  ├─ capabilities/        # Tauri 权限
│  ├─ icons/               # 应用图标与菜单栏图标
│  ├─ Info.plist           # LSUIElement 等 macOS 配置
│  └─ tauri.conf.json      # 窗口、CSP、打包配置
└─ test/                   # Node.js 内置测试
```

`src/shared/desktop-bridge.js` 用 `window.__TAURI__` 暴露 `window.deepFish`：桌面交互（拖动、菜单、设置、台词事件）沿用上游 Electron preload 的形状，另外增加了 `agent`（会话、发送、审批、事件订阅）、`setHitMask` / `pinClickThrough`（穿透点击）与 `onSpeak` 三组 macOS 侧能力。

素材制作脚本（`prepare-generated-frame.py` 等）与 Windows 启动器保留在上游仓库；本仓库直接使用已经处理好的 PNG 帧。替换角色时请保持每张画布尺寸与脚底锚点一致，并同步更新 `src/renderer/app.js` 中的 `frameNames`。

## 隐私与安全

- 渲染页面通过最小化桥接调用 Rust 命令，自定义命令按窗口能力配置开放。
- 自定义 API 地址必须使用 HTTPS。
- 模型请求由 Rust 后端发起，API Key 不暴露给页面脚本。
- 设置（含 API Key）保存在 `~/Library/Application Support/com.deepfish.desktop-pet/settings.json`，文件权限为 `0600`，只有当前用户可读；旧版本存在系统钥匙串里的 Key 不会自动迁移，需要重新填写一次。
- 会话历史保存在同一目录的 `agent/sessions.json`，只在本机；模型只能调用白名单工具，文件与命令访问被限制在用户主目录内，并按风险等级自动执行或要求确认（见下节）。命令默认还会套一层系统沙箱，越界的读写与未获批的联网会被内核直接拒绝。
- 公共免费模型的请求会离开本机，请先阅读服务提供方条款。
- 打开「联网搜索 / 网页抓取」后，模型发起的查询词和网页链接会被发送到 DeepSeek 的 `/v1/tools/*` 接口；不开启则这两个工具不会注册给模型。

## Agent 能力

聊天由 `src-tauri/src/agent/` 里的 harness 驱动，页面只负责渲染事件。

- **多会话**：会话保存在 `~/Library/Application Support/com.deepfish.desktop-pet/agent/sessions.json`（原子写入，最多 40 个会话、每个 400 条消息）。消息追加走**防抖合并写入**——内存里立即生效，后台每 700 ms 合并落盘一次，新建/改名/删除/压缩这类结构性改动仍然立即写，运行结束和退出应用时各强制 flush 一次，所以崩溃最多丢最后不到一秒的对话。顶栏显示当前会话，点标题切换、`＋` 新建、`×` 两段确认删除；标题会按第一条用户消息自动生成，也可由模型通过 `rename_session` 改动。
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
  | `run_command` | 在 `sh -c` 里执行命令，默认在系统沙箱内，15 s 超时、最长 600 s，输出截断到 6000 字符 | 只读命令自动执行，其它需审批 |
  | `web_search` | DeepSeek 联网搜索，返回标题/链接/摘要（需在设置里开启） | 开启后自动执行 |
  | `web_fetch` | 抓取网页并转成 Markdown（需在设置里开启） | 开启后自动执行 |

- **作用范围**：文件工具只能触碰 `$HOME` 之内（`~` / `~/x` 写法，`..` 逃逸会直接拒绝）；`DEEPFISH_TOOL_ROOT` 可整体收窄或放宽这个根目录。"超出主目录"的路径、重定向、命令替换、`&&`/`;` 一律升级为审批。
- **风险分级**（`src-tauri/src/agent/risk.rs`）：
  - **自动执行**：`web_search` / `web_fetch`（只有你在设置里打开开关时才会注册给模型）、`ls / pwd / cat / head / tail / wc / grep / rg / find / stat / du / df / jq / diff …` 这类只读命令，以及 `git status|diff|log|show|branch|rev-parse …`、`node --version`；管道里的每一段都必须是只读命令，且不能引用凭据路径。
  - **需要审批**：`write_file`、所有写操作、未知程序（`npm test`、`sed -i`、`find -delete`、`git push`、`rm -rf build`…），以及读取 `.ssh`、`.aws`、`.npmrc`、钥匙串、本应用设置等敏感路径。审批卡片出现在聊天里，显示将要执行的具体内容，`允许` / `拒绝` 二选一，**300 秒不点视为拒绝**；每次都问，不记忆。
  - **直接拒绝**：`sudo`/`su`、`shutdown`/`reboot`/`diskutil`/`mkfs`/`dd`、`security`（钥匙串命令行）、`launchctl`/`csrutil`/`nvram`/`tccutil`、`osascript`，以及 `curl`/`wget`/`ssh`/`scp`/`nc` 等外网与远程工具——避免读完本地文件再发出去。
- **被拒绝之后**：模型会收到一条明确的工具结果（"用户拒绝了这次操作" / "该操作被安全策略禁止"），系统提示要求它不要重试同一调用，改换方式或直接说明做不到。
- **命令沙箱**（`src-tauri/src/agent/sandbox.rs`，可在设置页关闭）：命令交给系统自带的 `/usr/bin/sandbox-exec` 执行，profile 以 `(deny default)` + Apple 的 `system.sb` 基线起步，不引入任何第三方沙箱库或运行时，release 二进制实测只增加约 16 KB（约 0.1%）。策略是：
  - **可读**：`/bin`、`/usr`、`/System`、`/Library`、`/opt`、`/private/etc`、`/dev` 等系统路径，加上当前工作目录，以及主目录里**除凭据目录以外**的一切（`.ssh`、`.aws`、`.gnupg`、`.kube`、`.netrc`、`.npmrc`、`.config/gh`、`Library/Keychains`、`Library/Cookies`、`Library/Safari`、本应用配置目录按 `risk.rs` 的同一份 `SENSITIVE` 列表挖掉，`.config/gh` 这种只挖子目录，同级兄弟目录照常可读）。
  - **可写**：只有当前工作目录、`/tmp` 与 `$TMPDIR`，以及 `~/Library/Caches`、`~/.cache`、`~/.npm`、`~/.cargo`、`~/.rustup`、`~/.yarn`、`~/.gradle`、`~/.m2` 等构建缓存——否则 `npm install` / `cargo build` 会因为 `EACCES` 失败。
  - **联网**：只读命令（`ls`、`git status`…）完全禁网；需要审批的命令在用户点"允许"后才可以联网，审批卡片会写明「沙箱内，禁网」/「沙箱内，可联网」。
  - **失败即拒绝**：`sandbox-exec` 缺失或 profile 无法应用时命令不会执行，而是回一条"沙箱不可用，请手动在终端运行"，绝不静默降级成无限制 shell。
  - 沙箱只在 macOS 生效；`sandbox-exec` 自 10.8 起被标记为 deprecated，但目前在 macOS 26 上仍然可用。实测边界：写 `~/x`、读 `~/.ssh/*`、`ping`/`curl` 在禁网状态下都会得到 `Operation not permitted`，而 `ls`/`git status`/`node --version`/`npm --version`/`cat ~/.gitconfig` 正常。
- **后台任务**：一次回复的多步执行完全在 Rust 侧（工具在阻塞线程池里跑，不会卡住其它会话的流式输出），关闭聊天、切换会话都不会打断；结果写入会话，重新打开即能看到。命令超时上限 600 s，工具循环与工具调用次数按设置页的「Agent 上限」执行（默认 50 轮 / 10 次），审批等待窗口 300 s。
- **桌宠通知气泡**：后台任务不看聊天窗口也能知道进展——需要审批时宠物头顶弹出「🔐 需要审批 / 主人，有一个操作需要审批」，任务结束弹出「✅ 任务完成 / 主人，任务完成了」或「⚠ 任务失败 / 主人，任务失败了」。这个气泡是深色卡片、左侧彩色强调条、没有小尾巴，和聊天气泡、动作台词气泡都不一样；点它直接打开对应会话（需要审批时卡片已经在那儿等着）。聊天窗口正开着且就是该会话时不再弹气泡，避免重复打扰。

## 实现说明

- 穿透点击由渲染层把当前帧的 alpha 通道采样成 8 px 网格（`set_hit_mask`），Rust 侧以 40 ms 轮询光标并按网格切换窗口的鼠标穿透；拖动期间整窗临时保持可交互。
- 气泡区高度跟随宠物缩放后的真实高度，对话与动作台词都紧贴头顶；聊天打开时不再弹动作台词，避免两块气泡叠在一起。

## 已知限制

- 只检测休眠恢复，不区分锁屏与解锁。
- 穿透蒙版按窗口上次移动时的位置缓存，运行中改变显示器排列或缩放比例后需要重新拖动一次桌宠才能完全对齐。
- 上下文预算按字符估算 token，与真实分词存在偏差；压缩质量取决于所用模型，免费模型偶尔会忽略工具调用。
- 命令沙箱拦不住"用户自己批准了、且命令本身就在允许范围内"的破坏（例如在项目目录里 `rm -rf .`），也不隔离内核层漏洞；需要在沙箱里跑完整工具链（Docker、虚拟机）不在本项目的轻量定位内。
- 未配置代码签名与公证，本地首次打开需右键“打开”；分发前请自行配置签名。
- 界面代码当前与上游 Electron 版本各自维护，上游界面更新需要手动同步。

## 许可

源代码使用 [MIT License](./LICENSE)。角色立绘不因源代码许可自动获得再授权；公开发布前请单独确认素材权利。
