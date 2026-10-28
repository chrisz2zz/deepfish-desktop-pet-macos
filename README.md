# 蓝色大肥鱼桌宠（macOS / Tauri 2）

一只基于蓝色大肥鱼意象进行二次创作的 macOS 桌面宠物，使用 Tauri 2 + Rust 实现，是从 [deepfish-desktop-pet](https://github.com/Dolphin2026-stl/deepfish-desktop-pet)（Electron / Windows 版）独立出来的 macOS 版本。

她会跟随本地时间作息，能被拖动、点击、投喂和摸头，也会在你从休眠回来时随机伸懒腰或被惊醒。

> 本项目是社区二创，不隶属于、也不代表 DeepSeek 官方。角色立绘沿用上游项目的创作素材；如需公开分发，请确认你拥有对应素材的使用与再授权权利。

## 功能

- 透明、无边框、置顶的 `340 × 430` 桌宠窗口，靠近屏幕边缘不会拉伸气泡
- 菜单栏常驻图标，不占用 Dock；左键显示/隐藏，右键打开菜单
- 原生右键菜单：聊天、投喂、摸头、陪玩、置顶、设置、隐藏、退出
- 待机呼吸浮动与 2.8-7 秒不定时整帧眨眼
- 点击、双击、拖拽、鼠标悬停和键盘操作反馈；快速绕圈拖动会转晕
- 生理时钟：清晨、专注、午饭、下午、晚间、睡眠七段状态
- 自动散步、洗碗、加班喝咖啡、抱鲸鱼玩偶、坐下和睡觉场景
- 头发、脸蛋、腿部区域感应：卖萌、害羞、绊倒，连续绊倒后会拿手帕哭哭
- 随机闲置彩蛋与 31 张固定锚点 PNG 表情、道具和动作帧
- 休眠恢复检测，醒来后随机触发“伸懒腰”或“惊醒”
- 内置聊天面板，保留最近十轮上下文
- 默认使用免 Key 的 Pollinations OpenAI 兼容接口，可切换 DeepSeek 或任意 OpenAI 兼容 API
- API Key 保存在 macOS 系统钥匙串（Keychain），设置文件只记录其余选项

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
| 双击 | 打开聊天面板 |
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

选择“自定义”，填写完整的 HTTPS Chat Completions 地址、模型名与可选 API Key。当前版本仅支持非流式响应。

## 开发与验证

```bash
npm test           # Node 内置测试：作息、行为目录、手势与平台桥接
npm run test:rust  # Rust 测试：聊天请求构造与设置校验
npm run check      # 检查关键文件存在且脚本语法有效
npm run smoke      # 启动已打包的 .app 做端到端自检（需先 npm run build）
```

`npm run smoke` 会把结果写入 `work/smoke-macos/`，校验桌宠帧是否加载、`window.deepFish` 桥接是否可用、设置窗口是否渲染并能经 Rust 后端保存设置，然后自动退出。

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
│  ├─ src/                 # Rust 后端：窗口、菜单栏、菜单、钥匙串、聊天
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
- 设置保存在 `~/Library/Application Support/com.deepfish.desktop-pet/settings.json`；API Key 只存在于系统钥匙串。
- 公共免费模型的请求会离开本机，请先阅读服务提供方条款。

## 已知限制

- 只检测休眠恢复，不区分锁屏与解锁。
- 未配置代码签名与公证，本地首次打开需右键“打开”；分发前请自行配置签名。
- 界面代码当前与上游 Electron 版本各自维护，上游界面更新需要手动同步。

## 许可

源代码使用 [MIT License](./LICENSE)。角色立绘不因源代码许可自动获得再授权；公开发布前请单独确认素材权利。
