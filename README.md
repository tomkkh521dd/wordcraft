<p align="center">
  <b>WordCraft · HarmonyOS（鸿蒙）原生移植</b><br>
  <sub>纯 Rust 开源文字处理器 —— 跑在鸿蒙平板与 PC/2in1 上，界面与桌面版完全一致。</sub>
</p>

> **本仓库是什么**：这是 **Karl卡尔大叔** 基于开源项目 **WordCraft** 的 **HarmonyOS NEXT（鸿蒙）原生移植**分支
> （工作分支 `ohos/port`，不合并进 `main`；`main` 用于跟随上游）。
> 上游项目（原作者）：[storytold/wordcraft](https://github.com/storytold/wordcraft) ·
> 许可：MIT OR Apache-2.0（见 [`LICENSE-MIT`](LICENSE-MIT)、[`LICENSE-APACHE`](LICENSE-APACHE)）·
> 上游 README 原文完整保留在 [`README.upstream.md`](README.upstream.md)，一字未改。

---

## 关于我们

| | |
|---|---|
| 作者 | **Karl卡尔大叔** |
| X (Twitter) | <https://x.com/Karl02049> |
| YouTube | <https://www.youtube.com/@karl1367> |

做这件事的原因很简单：**世界需要一个真正开放的、用 Rust 写的、能听 agent 指挥的文字处理器**，
而它现在缺一块拼图 —— 鸿蒙。我们把这一块补上：同一套引擎、同一套界面、同一份 `.docx`，
在鸿蒙平板上原生运行，不套壳、不转译、不依赖 WebView。

---

## 1. 项目是什么

**WordCraft**（上游）是一个对标 Microsoft Word 的开源文字处理器，全部用 Rust 编写：

- **引擎**：文档模型、排版、分页、表格、页眉页脚、修订、域、邮件合并、公式、图表占位；
- **格式**：`.docx` 读写（ECMA-376/ISO 29500）、RTF / ODT / HTML / Markdown / TXT / PDF 导出；
- **界面**：Ribbon 风格 egui 界面，与 Word 的布局和快捷键对齐；
- **可被自动化**：数百条命令（上游统计 433 条）、命令行、控制通道、MCP 服务 —— 每个功能都是一个命令；
- **跨平台**：macOS / Windows / Linux / BSD 原生，浏览器 WASM。

我们**不改它的引擎和界面**，只在它上面新建一层 **鸿蒙壳**。

## 2. 我们做了什么（本次移植）

```
ohos/                 鸿蒙工程（ArkTS 页面 + C++ NAPI/原生窗口缓冲 + 资源 + hvigor 配置）
apps/wordcraft-ohos/  Rust cdylib 壳：C ABI（wc_*），唯一允许 unsafe 的薄层
crates/ohos-app/      鸿蒙应用逻辑：配置、输入、帧循环、平台服务、宿主桥（全为安全 Rust）
crates/ui-paint/      egui 输出的 CPU 软件光栅器（鸿蒙上不建 GPU 上下文）
packaging/ohos/       环境探测、构建、打包、签名脚本
docs/platform-ohos.md 平台文档：能做什么、怎么做、上游改动清单、真机证据
```

技术要点：

- **不建 GPU 上下文**：文档页由 `vello_cpu` 光栅化，界面由自研 `ui-paint` 合成，直接写原生窗口缓冲
  （`OH_NativeWindow` 请求缓冲 → 拷贝 → `FlushBuffer`），驱动问题不会让编辑器崩掉；
- **脏矩形 + 整行拷贝**：静态内容不重绘，页面位图走整行 1:1 拷贝，三角形路径按行递推边缘函数；
- **平台服务走「Rust 提问 / ArkTS 应答」的宿主桥**：文件选择器、剪贴板、输入法都由 ArkTS 侧完成，
  Rust 侧只发一个 JSON 请求、在后续帧拿答案，永不阻塞帧；
- **文档以沙箱工作副本编辑**：鸿蒙 picker 只授权「文件」不授权「目录」，而安全保存需要写同级临时文件；
  因此打开时把用户文件复制进沙箱，保存后自动回写到用户选择的位置（含导出与 PDF）；
- **输入法接到自绘画布**：`inputMethod.attach` 绑定 egui 画布，软键盘在光标处弹出、候选窗跟随，
  拼音候选上屏走 `ImeEvent::Commit`（一次撤销）；
- **可跟随上游**：上游文件只改 5 处、每处追加式几十行（字体目录、设置目录、界面 CJK 回退、`xtask` 入口、
  分层表），其余全部是新增目录 —— 随时可以 `git merge main`。

## 3. 进展（截至 2026-10-22）

| 里程碑 | 状态 | 说明 |
|---|---|---|
| OHOS.1 目标打通与注入点 | ✅ | `cargo xtask ohos` 对 `aarch64`/`x86_64-unknown-linux-ohos` 双目标通过（20 个 crate，含全部引擎/PDF/界面） |
| OHOS.2 HAP 骨架 + 首帧上屏 | ✅ | HAP 工程、C++ NAPI 壳、原生窗口缓冲、`mmap` 帧呈现、hilog；模拟器像素格式/stride/DPI/输入全部实测 |
| OHOS.3 真实界面 + `ui-paint` | ✅ | Ribbon、标尺、示例文档、表格、列表、中文界面；脏矩形；1:1 整行拷贝；光栅器优化 |
| OHOS.4 平台服务 | ✅ | 系统文件选择器（打开/另存）、剪贴板（文本/HTML/图片读、文本/HTML 写）、跨应用粘贴；打开→改→保存→重开一致 |
| OHOS.5a/5b 输入法 | 🚧 | 软键盘、英文与拼音上屏、Backspace/Delete/Enter/方向键、候选窗跟随光标；页面内组词未做（华为输入法不给自绘客户端发 preview 文本） |
| OHOS.6 打包签名/上架 | ⏳ | 本机调试签名可用；AppGallery 发布未做 |
| OHOS.7 打磨 | ⏳ | 手机竖屏布局、打印、朗读、Zotero 等 |

**真机验证**：HUAWEI BKY-W00（HarmonyOS 6.1.0.135，API 24，1840×2650 @ 2.25×）：

- 完整界面在平板上运行（中文 Ribbon、标尺、文档、表格、状态栏）；
- 用系统 picker 保存 10.6 KB `.docx` 到「个人」目录，重启应用后重新打开内容完整；
- 剪贴板：复制 2021 字符 + HTML，跨应用（备忘录 → WordCraft）粘贴成功；
- 输入法：点击文档弹出软键盘，按键盘输入 `Gekko`，切拼音打 `ni` 选「呢」上屏；
- 性能：一整帧（7172 三角形 / 17.0M 像素）`paint 36.8 ms`（优化前 46.2 ms）。

**质量门禁**：`cargo xtask ohos`（aarch64 + x86_64 双目标）、`cargo xtask layers`、`cargo xtask assets`、
`cargo fmt`、`cargo clippy -D warnings`，以及 `crates/ohos-app` 23 个、`crates/ui-paint` 11 个测试全部通过；
引擎与界面层 `unsafe_code` 一律 `forbid`（仅 C ABI 导出允许 `#[unsafe(no_mangle)]`）。

## 4. 已知限制

- 输入法组词（拼音字母）只在键盘自带的浮泡里显示，页面内不显示组合串（上游桌面版有该绘制，鸿蒙侧拿不到 preview 事件）；
- 选区未与输入法同步（全选/剪切/复制的 IME 指令暂无上下文），滚动时候选窗只在下次按键时跟随；
- 打印、朗读（TTS）、Zotero 尚无鸿蒙实现；手机竖屏布局未适配；
- 剪贴板图片的读取链路已实现并单测，但尚未在真机上粘贴过图片；
- 帧率仍受 CPU 光栅限制（大窗口高 DPI 下明显），只重绘脏区与字形采样是下一步；
- 未上架 AppGallery，仅本机调试签名 + `hdc install`。

## 5. 快速开始

```sh
# 1) 目标平台
rustup target add aarch64-unknown-linux-ohos x86_64-unknown-linux-ohos

# 2) 检查（自动寻找 DevEco/OpenHarmony SDK）
cargo xtask ohos

# 3) 打 HAP（需要 hvigor 与 DevEco 的签名材料）
packaging/ohos/build-hap.sh
hdc install -r dist/release/wordcraft-0.9.0-debug.hap
hdc shell aa start -a EntryAbility -b karl.hmsoft.wordcraft
```

详细的 SDK 路径、签名、设备与日志说明见 **[`docs/platform-ohos.md`](docs/platform-ohos.md)**。

## 6. 上游与我们的边界

- **不碰**上游的桌面壳（`apps/wordcraft`，eframe/winit 没有鸿蒙后端）、引擎、排版、格式层；
- **新增**：`ohos/`、`apps/wordcraft-ohos/`、`crates/ohos-app/`、`crates/ui-paint/`、`packaging/ohos/`、
  `docs/platform-ohos.md`；
- **上游文件只做 5 处追加式改动**（系统字体目录、设置目录、界面 CJK 回退字体、`xtask ohos` 入口、分层表），
  全部带 `// OHOS:` 注释，便于长期跟随上游主线。

## 7. 许可、来源与声明

- 上游代码版权归 **ArtCraft 团队与 WordCraft 贡献者**所有，采用 **MIT OR Apache-2.0** 双许可；
  本仓库保留两份许可原文，移植新增部分同样以 MIT OR Apache-2.0 发布。
- 上游 README 原文、商标与徽章属于上游，已原样保留在 [`README.upstream.md`](README.upstream.md)。
- 本项目与 Microsoft 无关；「Word」「Microsoft 365」是微软商标，仅用于说明兼容目标。
- **清洁室**：不含任何来自 Microsoft Word/Office 或 Adobe/Avid/Autodesk 的图标、图片、模板、字体或文案；
  界面图标由代码绘制，示例文档与主题为本仓库原创文本与配色。

---

<p align="center"><sub>基于开源项目 WordCraft 构建 · 鸿蒙移植与维护：Karl卡尔大叔 ·
<a href="https://x.com/Karl02049">X</a> · <a href="https://www.youtube.com/@karl1367">YouTube</a></sub></p>
