# ReadAll

ReadAll 是一个用 Rust 自研的原生电子书阅读器。项目希望尽量自己完成文档容器解析、文本布局、字体解析、CPU 光栅化和原生窗口交互，不依赖 WebView，也不直接接入现成 EPUB/PDF 阅读引擎。

> **v0.1.0 是首个开发预览版。当前重点是 Linux Wayland + EPUB/TXT；Windows、Android 和 PDF 仍在后续路线中。**

## v0.1.0 已实现

### 原生 Linux 阅读界面

- 零参数启动原生书库：`./readall`
- 内置文件浏览器，可直接选择 `.epub`
- 中文文件名、中文界面和中文正文
- 内置 **LXGW WenKai Lite Regular / 霞鹜文楷轻便版**
- 鼠标 hover 高亮，不需要先点击
- 长书名自动横向滚动，不覆盖右侧章节/页码
- 三层底部悬浮工具栏：
  - 上一页 / 下一页
  - 目录
  - 字号减 / 字号加
  - 展开 / 收起
- Feather SVG 图标由 ReadAll 自己解析和抗锯齿绘制
- EPUB 阅读进度自动保存和恢复
- 窗口尺寸、字号变化后尽量保持同一内容位置

### EPUB

ReadAll 当前自己处理：

- ZIP 中央目录和本地文件头
- Store / raw DEFLATE
  - Stored block
  - Fixed Huffman
  - Dynamic Huffman
- CRC-32、路径安全和资源预算
- `mimetype`
- `META-INF/container.xml`
- OPF metadata / manifest / spine
- EPUB 3 Navigation Document：`properties="nav"` + `nav epub:type="toc"`
- EPUB 2 NCX fallback：`spine toc` + `navMap/navPoint/navLabel/content`
- 正式目录标题、嵌套层级与 `href#fragment` 目标解析
- XHTML 可见正文提取
- 常见 HTML / XHTML DOCTYPE
- 常见 legacy XHTML 命名实体（如 `&nbsp;`、`&mdash;`、`&hellip;`、`&copy;`），固定映射且不加载外部 DTD
- 跨 spine 连续阅读
- 自动跳过 SVG 封面、纯图片页、空 XHTML 和当前不可读 spine
- 稳定内容定位：
  `epub-v1:<book-sha256>:<spine>:<utf8-offset>`

当前 EPUB **尚未完整渲染**：

- CSS
- 图片
- SVG 正文
- MathML
- EPUB 内嵌字体
- DRM / 加密 EPUB
- Fixed-layout EPUB

目录面板优先使用 EPUB 3 Navigation Document；没有 EPUB 3 nav 时会尝试 EPUB 2 NCX，再没有可用目录资源时才回退为从可读 spine 正文首行推断标题。两种正式目录都会保留嵌套层级；`href/src#fragment` 会解析到目标 XHTML 元素的 `id/xml:id`，再映射到规范化正文 UTF-8 offset，因此同一 XHTML 内的子目录也可以精确跳转，并继续复用 `epub-v1` locator 保持重排后的内容位置。fragment 缺失或无法解析时回退到目标 spine 开头。为兼容旧 XHTML，ReadAll 内置少量固定的常见命名字符实体；未知自定义实体、内部 DTD 子集和外部实体仍然拒绝，不会下载或展开外部 DTD。

### TXT / 字体 / 渲染

- UTF-8、UTF-8 BOM
- UTF-16 LE/BE BOM
- CRLF / CR → LF 规范化
- SHA-256 内容身份
- 稳定 `txt-v1` 内容 locator
- 自研 TrueType/TTC 解析
- `cmap` format 4 / 12
- `hhea/hmtx` 字宽
- `loca/glyf`
- 简单字形、复合字形
- 二次 Bézier 轮廓光栅化
- 4×4 灰度抗锯齿
- CPU RGBA surface、裁剪与 source-over
- TXT/EPUB 页面导出诊断路径

字体引擎目前不支持 CFF/CFF2、WOFF、可变字体、hinting、复杂文字 shaping、完整双向排版和真正的多字体 fallback。

## 构建

### 要求

- Rust **1.85+**
- Linux Wayland
- 系统 `libwayland-client`

Gentoo：

```bash
sudo emerge --ask dev-libs/wayland
```

Debian / Ubuntu：

```bash
sudo apt install libwayland-dev
```

### 编译

```bash
git clone https://github.com/MRsoymilk/ReadAll.git
cd ReadAll
cargo build --release
./target/release/readall
```

首次从干净源码构建 GUI 时，构建脚本会获取固定版本的 LXGW WenKai Lite TTF，校验后通过 `include_bytes!` 嵌入二进制。**最终运行 ReadAll 不需要联网，也不要求用户安装中文字体。**

完全离线打包可预先准备同一字体：

```bash
READALL_BUILTIN_FONT_SOURCE=/path/to/LXGWWenKaiLite-Regular.ttf \
cargo build --release
```

纯命令行 / 无 Wayland 构建：

```bash
cargo build -p readall --release --no-default-features
```

## 使用

直接启动书库：

```bash
./target/release/readall
```

常用阅读操作：

| 操作 | 键盘 / 鼠标 |
| --- | --- |
| 下一页 | PageDown / → / ↓ / Space / 页面右侧点击 |
| 上一页 | PageUp / ← / ↑ / 页面左侧点击 |
| 第一页 | Home |
| 最后一页 | End |
| 字号 | `+` / `-` 或底部工具栏 |
| 目录 | 底部“目录” |
| 返回书库 | Backspace |
| 关闭 | Esc |

CLI 帮助：

```bash
./target/release/readall --help
```

一些开发/诊断命令：

```bash
./target/release/readall epub-info /path/to/book.epub
./target/release/readall epub-text /path/to/book.epub --spine 1
./target/release/readall inspect tests/fixtures/sample.txt
./target/release/readall read tests/fixtures/sample.txt --columns 40 --rows 8 --page 1
```

## EPUB 兼容问题与错误日志

EPUB 打开失败时，ReadAll 会在终端输出失败阶段和诊断日志位置，例如：

```text
无法打开 EPUB: EPUB stage 'parse EPUB ZIP/container/OPF' failed: ...
错误日志: /home/user/.local/state/readall/logs/readall-error.log
```

查看当前日志：

```bash
./target/release/readall diagnostics
```

默认路径：

```text
$XDG_STATE_HOME/readall/logs/readall-error.log
```

未设置绝对 `XDG_STATE_HOME` 时：

```text
$HOME/.local/state/readall/logs/readall-error.log
```

日志包含版本、平台、书籍路径/大小、失败阶段以及完整 `caused_by[n]` 错误链。超过 2 MiB 会轮换为 `readall-error.log.old`。

如果某本 EPUB 无法打开，请优先提交 **EPUB compatibility** Issue，并附上 `readall diagnostics` 输出。请不要上传仍受版权保护的整本电子书，除非你有权公开分发它。

## 项目结构

```text
ReadAll/
├── apps/
│   └── readall/            # GUI、CLI、阅读会话、进度和诊断
├── crates/
│   ├── readall-archive/    # ZIP / DEFLATE / CRC
│   ├── readall-core/       # 文档、文本、locator、布局
│   ├── readall-epub/       # EPUB container / OPF / XHTML
│   ├── readall-font/       # TrueType / TTC / glyph outline
│   ├── readall-render/     # CPU surface / glyph rasterizer
│   └── readall-platform/   # 文件与原生窗口平台层
├── res/
│   └── icons/reader/       # Reader SVG icons
├── licenses/               # Bundled font licenses
└── tests/
    └── fixtures/
```

依赖方向保持分层：解析层不持有窗口对象，渲染层不处理 EPUB，平台层不解析文档。

## 开发检查

```bash
cargo fmt --all -- --check
cargo test --workspace --offline
cargo clippy --workspace --all-targets --offline -- -D warnings
cargo check -p readall --no-default-features --offline
```

项目包含一个本地模拟 Wayland compositor，用于测试 configure、buffer commit/release、键盘、鼠标、hover 和无输入动画 tick。模拟测试不能替代 Hyprland/Weston 等真实桌面的视觉验收。

## 当前限制

v0.1.0 不是“完整 EPUB 阅读器”声明。当前主要限制：

- Linux 原生 GUI 当前只实现 Wayland
- Windows / Android 窗口后端尚未实现
- PDF 尚未实现
- EPUB CSS / 图片 / SVG / MathML 尚未完整进入阅读排版
- 没有复杂字体 shaping / bidi / 字素簇级排版
- 没有真正的字体 fallback
- 没有封面墙、搜索、书签、高亮和笔记
- HiDPI / 分数缩放仍需继续完善

## Roadmap

1. EPUB CSS 子集、图片与更完整的排版
2. 最近阅读、封面书架、阅读主题、设置页
3. 字体 fallback、字素/单词断行、选择/高亮
4. Windows 原生窗口后端
5. Android 平台入口
6. PDF 对象、页面、字体、图像和绘制指令

## 许可证与第三方资源

ReadAll 项目源码目前**尚未声明统一的开源许可证**。在明确项目代码许可证之前，请不要假定项目源码可以按 MIT/Apache/GPL 等许可证再分发。

已捆绑的第三方资源分别遵循其自己的许可证：

- **LXGW WenKai Lite Regular**：SIL Open Font License 1.1  
  许可证：`licenses/LXGW_WenKai_Lite_OFL.txt`
- **Feather Icons v4.29.2**：MIT  
  许可证：`res/icons/reader/LICENSE`

运行：

```bash
./target/release/readall licenses
```

可查看内置字体许可证。

---

Repository: https://github.com/MRsoymilk/ReadAll
