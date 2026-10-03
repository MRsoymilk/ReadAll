# ReadAll

Rust 自研电子书阅读器，目标平台为 Linux、Windows、Android。文档解析、排版、绘制和阅读交互由项目自身实现，不使用 WebView 或现成 PDF/EPUB 引擎。

## 当前状态

项目处于基础引擎阶段，**Linux Wayland 下零参数启动进入原生 ReadAll 书库界面，可在 GUI 内浏览目录、选择 `.epub` 并直接进入阅读，同时保留命令行、TXT/EPUB 页面渲染和原生阅读窗口**。窗口已通过编译、本地模拟合成器协议测试和无显示环境的真实 CLI 错误路径测试，尚未完成真实桌面目视验收；EPUB 当前只支持自研 XHTML 文本子集，CSS/图片等仍未渲染，PDF 尚未实现。不得把文件签名检测当作对应格式已经支持。

已实现：有大小上限的文档输入、平台无关的随机读取接口、UTF-8/UTF-8 BOM/UTF-16 BOM 文本解码、换行规范化、原始文件 SHA-256 内容标识、可序列化且校验文档/字符边界的文本定位、诊断分页与重排定位、真实字宽驱动的基础分页、自研 TrueType 字形抗锯齿绘制、CPU 矩形绘制/嵌套裁剪/透明度合成。

当前外部 crate 依赖为零，仅使用 Rust 标准库和工作区内部 crate。GUI 发布二进制内置 `LXGW WenKai Lite Regular` 中文字体；字体文件采用 SIL Open Font License 1.1，许可证保存在 `licenses/LXGW_WenKai_Lite_OFL.txt`，也可运行 `readall licenses` 查看。`readall` 应用默认启用 `wayland` feature，在 Linux 会链接系统 `libwayland-client` 处理窗口协议与文件描述符传输；字体解析、排版和像素绘制仍由 Rust 自研代码完成。如需纯命令行/无窗口构建，可显式使用 `--no-default-features`。核心/字体/渲染模块继续禁止 unsafe；平台层默认 deny，仅私有 `wayland` 模块允许必要 FFI，原始指针不暴露给应用。操作系统、系统库和标准库不属于“零依赖”承诺。

## 构建与运行

```sh
cargo build --release
./target/release/readall
./target/release/readall --help
./target/release/readall licenses

cargo test --workspace --offline
cargo run -p readall --offline -- inspect tests/fixtures/sample.txt
cargo run -p readall --offline -- read tests/fixtures/sample.txt --columns 40 --rows 8 --page 1
cargo run -p readall --offline -- render-demo target/calibration.ppm
cargo clippy --workspace --all-targets --offline -- -D warnings
cargo fmt --all -- --check
```

Linux 默认构建已经启用 Wayland；直接运行 `readall` 会启动自绘书库。点击“打开图书”或按 Enter 进入内置文件浏览器，浏览器从 `$HOME` 开始，只显示目录和 `.epub` 文件；点击目录继续进入，点击 EPUB 直接打开，Backspace 或顶部“返回”回到上级。GUI 已移除 5×7 像素字库，书库、中文文件名、路径、状态栏、阅读栏和默认 EPUB 正文统一使用**内置中文 TrueType 字体**与项目自己的抗锯齿光栅化；最终用户不需要安装或下载字体。`READALL_UI_FONT=/path/font.ttf` 仅保留为开发覆盖项。

`read` 的页码从 1 开始；输出 `Start locator` 可通过 `--at 'txt-v1:…'` 恢复到包含对应内容的页面，允许同时更改 `--columns`/`--rows`。`--page` 与 `--at` 互斥。`render-demo` 输出图形校准 PPM，不是电子书页面；为保护文件，目标已存在时拒绝覆盖。

诊断分页按 ASCII 1 格、其他 Unicode 标量 2 格、Tab 4 格制表位估算宽度，仅验证源文本覆盖和位置映射。不等同于终端真实显示宽度，也不支持真实字体塑形、字素簇、双向排版、单词断行或完整 Unicode 规则。`render-text` 使用独立的真实字宽排版模块，不使用这个格数估算。

Cargo 声明 Rust 1.85 / edition 2024 作为最低目标；这不是已经完成所有工具链与系统验证的声明。Linux、Windows 和 Android 实机验证分别推进，不把宿主机编译等同于三端验收。

## 模块边界

| 模块 | 职责 |
| --- | --- |
| `readall-archive` | 自研受限 ZIP 与 raw DEFLATE（Stored/Fixed/Dynamic Huffman）、CRC-32、路径和资源预算检查 |
| `readall-epub` | EPUB mimetype、container.xml、OPF metadata/manifest/spine、本地资源路径、XHTML 文本子集与稳定 epub-v1 locator；CSS/图片尚未渲染 |
| `readall-core` | 文档输入约束、格式模型、文本解析、内容位置；不依赖文件路径或窗口对象 |
| `readall-font` | 自研 TrueType/TTC 解析、Unicode 字形映射、真实字宽、简单及复合字形轮廓；不执行字体字节码 |
| `readall-render` | 自研 RGBA 像素缓冲区、矩形裁剪与合成、二次曲线字形光栅化和灰度蒙版；无 GPU 或窗口呈现 |
| `readall-platform` | 本地文件访问、安全窗口接口、可选 Wayland 输入/共享内存呈现；Android URI 尚未实现 |
| `readall` | 默认原生主窗口、CLI、TXT/EPUB 页面编排、字形缓存、图片导出和原生阅读会话；失败时保留原页面与位置 |

矩形绘制模块限制像素数量、指令数量、裁剪深度和累计混合像素数；`draw` 先检查整份矩形/裁剪指令，再修改像素，失败不会留下部分绘制结果。字形绘制逐次检查蒙版与裁剪；页面导出在内存中完成整页后才创建目标文件，输入解析或绘制失败不会生成页面文件，但磁盘写入失败仍可能留下新建的部分文件。透明度为直通 Alpha 的字节空间 source-over 合成，不提供线性光或完整 PDF 色彩管理。诊断分页默认最多 200,000 行。

文本读取默认限制原文件 32 MiB、解码后 64 MiB，调用方可配置。SHA-256 用于内容身份，不提供数字签名验证。读取能发现长度变化，但不承诺对正在修改的文件取得原子快照。

文本位置格式为 `txt-v1:<原始文件SHA-256>:<规范化UTF-8字节偏移>`。v1 去除编码 BOM，将 CRLF/CR 规范化为 LF；偏移不是 UTF-16 文件的原始字节位置，也不是字符序号或页码。原文件内容改变后，旧位置拒绝恢复。原生阅读默认按文档内容 SHA-256 保存该 locator，而不是保存易受重排影响的页码。

UTF-16 必须带 BOM；GBK 等旧编码和 UTF-32 尚未支持。无效编码、NUL/终端控制字符会报错，而不是有损替换。`ZIP` 签名只说明可能为 EPUB，尚不验证 ZIP/EPUB 结构。

## EPUB 容器与包结构

当前 EPUB 阶段没有使用 `zip`、`flate2`、XML/HTML 等第三方 crate。新增 `readall-archive` 自行解析 ZIP 中央目录和本地文件头，支持 Store 与 raw DEFLATE 的 Stored/Fixed/Dynamic Huffman block，并验证 CRC-32、大小预算、重复条目、越界/重叠范围和危险路径。初期明确拒绝 ZIP64、多磁盘、加密以及未支持的压缩方法，不把未知特性静默降级。

`readall-epub` 在此基础上校验 EPUB 的首个未压缩 `mimetype`、`META-INF/container.xml`、package OPF、manifest 与 spine，并解析标题和本地资源引用。自研受限 XML 解析器支持命名空间名、实体、注释和 CDATA，同时拒绝 DTD、自定义实体、过深结构和资源超限。当前为了边界清晰，`META-INF/encryption.xml`、远程资源 URI、多 package rootfile 都直接报告未支持。现在也能读取 `application/xhtml+xml` spine 项的初始阅读子集：只提取 `body` 可见文本，保留常见块级/换行语义并规范化空白，忽略 `script/style/template` 内容；CSS、图片、SVG、MathML 尚未进入排版。

可以对真实 EPUB 做结构检查：

```sh
cargo run -p readall --offline -- epub-info /path/to/book.epub
cargo run -p readall --offline -- epub-text /path/to/book.epub --spine 1
cargo run -p readall --offline -- render-epub /path/to/book.epub target/epub-page.ppm --spine 1 --font /path/to/font.ttf
```

`epub-info` 只输出结构信息；`epub-text` 可检查指定 spine 的规范化正文。`render-epub` 是无窗口诊断导出，而默认 GUI 已可选择 EPUB、跨 `linear=yes` spine 连续翻页并自动保存进度。EPUB 使用独立的 `epub-v1:<整书SHA-256>:<零基spine>:<规范化UTF-8偏移>` locator；页面尺寸或字号变化后仍按内容位置恢复，并拒绝其他 EPUB 修订版的 locator。

## 字体引擎

`readall-font` 已支持静态 TrueType sfnt 与 TTC 指定 face、Unicode `cmap` 4/12、`hhea/hmtx` 字宽、长短 `loca`、简单轮廓与复合轮廓（平移、缩放、矩阵变换和实际轮廓点对齐）。循环引用、截断数据、越界索引及超出深度/点数/组件预算会报错。解析格式参考 [OpenType 规范](https://learn.microsoft.com/en-us/typography/opentype/spec/)、[字符映射](https://learn.microsoft.com/en-us/typography/opentype/spec/cmap)与[字形轮廓](https://learn.microsoft.com/en-us/typography/opentype/spec/glyf)。

当前不支持 CFF/CFF2、WOFF、可变字体、字体塑形、hinting 字节码、phantom point 对齐或带变换的 USE_MY_METRICS。文件结构校验不等于字体真实性认证，也未实现字体校验和检查。字体加载上限默认 64 MiB；解析核心不访问系统路径。测试在 Rust 中构造小型样本，不将系统字体复制进项目。

```sh
cargo run -p readall-font --example inspect --offline -- --discover
cargo run -p readall-font --example inspect --offline -- /usr/share/fonts/dejavu/DejaVuSans.ttf
```

上面的实际字体路径只用于字体引擎开发诊断；GUI 不再依赖系统中文字体。默认内置字体固定为 `LXGW WenKai Lite Regular`，构建脚本锁定上游提交 `4cddacbe244b0a24b10076369105f0495e5ec898`，下载后会校验字节大小、TrueType 结构和关键中英文字形，再通过 `include_bytes!` 编入二进制。开发诊断仍可用 `--discover-cjk` 查找系统 CJK 字体。

## 真实字体 TXT 页面导出

```sh
cargo run -p readall --offline -- render-text tests/fixtures/sample_latin.txt target/text-page.ppm --font /usr/share/fonts/dejavu/DejaVuSans.ttf --font-size 24 --width 800 --height 1000
```

`render-text` 将文本通过自研字体解析、字宽测量、分页和像素合成输出为 PPM 图片。`--font` 必填；TTC 可用 `--face N` 选择字体。页码 `--page N` 从 1 开始，也可用 `--at 'txt-v1:…'` 恢复到包含内容位置的页面，两者互斥。可调整 `--width`、`--height`、`--font-size`、`--margin`。目标存在时拒绝覆盖，包括误把原书或字体路径作为输出。

默认遇到缺字直接报错。`--missing replacement` 是显式降级选项，使用该字体的 `.notdef` 轮廓并报告缺失字符，不静默忽略；尚无字体回退。示例字体已在开发宿主上验证拉丁文字、重音字符和希腊字母，但不包含中文。中文渲染需要含相应字形且采用当前支持轮廓格式的字体，不能仅凭文件扩展名判断。

字形绘制采用自适应二次曲线细分、非零环绕填充和 4×4 灰度采样，包含轮廓空洞、重叠与负侧边距处理。当前不执行 hinting、不做亚像素定位；字形基线取整数像素。每页按需缓存可见字形蒙版，限制独立字形数、解码点数、蒙版缓存和绘制工作量。

`MeasuredLayout` 接收真实字宽回调，不依赖特定字体或窗口。它保留规范化文本范围与 locator 映射，但当前仍按 Unicode 标量逐个换行，不支持完整单词断行、字素簇、字距调整、复杂文字塑形或双向排版；不能将图片导出等同于完整阅读体验。单次排版默认最多 1,000,000 个 Unicode 标量、200,000 行，超过限制明确报错。

## Linux 原生阅读窗口

在已登录的 Wayland 桌面终端中运行，系统须有可供链接的 `libwayland-client`：

```sh
cargo run -p readall --features wayland --offline -- open tests/fixtures/sample_latin.txt --font /usr/share/fonts/dejavu/DejaVuSans.ttf
cargo run -p readall --features wayland --offline -- open-epub /path/to/book.epub --font /path/to/font.ttf
```

默认 GUI 打开 EPUB 时直接复用二进制内置的中文字体，不扫描系统字体。阅读会话会自动跳过 `linear=yes` 中当前没有可读文本的 SVG 封面、纯图片页、空 XHTML 以及当前未支持的非 XHTML spine；只有整本书都找不到可读 XHTML 文本时才报错。阅读页顶部使用该真实字体显示书名、章节/页码和字号，底部显示中文操作提示与整书进度条。左右半区点击、竖向滚轮、PageUp/PageDown/方向键/Space 翻页，`+/-` 调字号，Backspace 返回书库。命令行 `open-epub` 仍支持显式 `--font`、`--spine` 和 `--at epub-v1:...`。

窗口缩放和字号变化保留同一个精确内容 anchor，不反复替换成“当前屏幕第一页文字”，防止连续缩放后位置向前漂移。TXT 与 EPUB 原生阅读都默认使用 `$XDG_STATE_HOME/readall/progress-v1`，或未设置绝对 `XDG_STATE_HOME` 时使用 `$HOME/.local/state/readall/progress-v1`；TXT 保存 `TextLocator`，EPUB 保存绑定整书 SHA-256、spine 和规范化文本偏移的 `epub-v1` locator。成功翻页及正常关闭时用同目录临时文件 + rename 更新状态；显式 `--page`/`--spine`/`--at` 优先于自动恢复。两种窗口都可用 `--progress off` 禁用或 `--state-dir DIR` 指定状态目录。失败的跨章节翻页、重排或字号修改保留当前可见页面并在终端报错。

窗口采用 `wl_compositor` v4、`xdg-shell` v1 和 XRGB8888 SHM；输入需要 `wl_seat` v5。等待 configure 并 ack 后才附加缓冲区；同一时刻最多两个未释放缓冲区，释放前不覆盖其内容。临时文件以独占方式创建并立即解除路径关联。空闲时阻塞等待事件，不持续绘制。

`--display <socket>` 用于显式选择合成器，默认只使用调用进程的 Wayland 会话配置，不猜测用户 socket，也不修改桌面环境。`--frames 1` 是一次提交后关闭的协议诊断选项，只表示提交已由合成器处理，不代表用户已经看到画面。

```sh
cargo test --workspace --features wayland --offline
cargo clippy --workspace --all-targets --features wayland --offline -- -D warnings
cargo run -p readall-platform --example probe --offline
```

从源码进行**第一次干净 GUI 构建**时，`apps/readall/build.rs` 会自动获取固定提交的字体 TTF；这属于构建资源获取，发布后的 `readall` 二进制已包含字体，运行时不联网。源码打包器或完全离线构建可预先准备同一字体，并通过 `READALL_BUILTIN_FONT_SOURCE=/path/LXGWWenKaiLite-Regular.ttf` 指定本地来源。字体已经存在于 Cargo 的构建输出后，后续构建不会重复获取；因此首次推荐 `cargo build --release`，完全缓存后再使用 Cargo `--offline`。

当前验证环境缺少 `WAYLAND_DISPLAY`、`XDG_RUNTIME_DIR` 和桌面 socket。测试使用真实 `libwayland-client` 连接 Rust 本地模拟合成器，覆盖 ping/pong、首次 configure、提交顺序、键盘事件、重配尺寸和 buffer release，**不等同于 Hyprland/Weston 等真实桌面的视觉/交互验收**。模拟测试不连接或操作用户桌面。接口依据 [Wayland 客户端 API](https://wayland.freedesktop.org/docs/html/apb.html)、[核心协议](https://wayland.freedesktop.org/docs/html/apa.html)及系统安装的稳定 xdg-shell 协议描述。

这一版已有基础书库/文件浏览和阅读状态栏，但尚无封面墙、EPUB 元数据书架、目录侧栏、文字选择、真正多字体回退、输入法、无障碍接口或 HiDPI/分数缩放适配。SHM 像素按 1:1 逻辑尺寸提交。文档和字体文件只读取一次；同一字号与窗口几何下翻页复用整本文本的测量布局，并跨页保留已经光栅化的字形蒙版。窗口尺寸变化只重建布局，字号变化建立新的字号专属字形缓存。仍未缓存完整页面像素，大文档首次排版性能需要继续优化。Windows/Android 窗口尚未实现。

## 后续顺序

1. 优先继续 UI：最近阅读/封面书架、EPUB 元数据展示、目录侧栏、设置页、阅读主题和更完整的鼠标交互。
2. EPUB：在已完成的 GUI 打开、XHTML 文本、稳定 locator、跨 spine 翻页和自动进度基础上，加入 navigation/TOC、CSS 子集、图片和更完整的自研排版。
3. 文本与字体：真正字体回退、字素/单词断行、文字选择，并优化首次排版和大书缓存。
4. PDF：对象与交叉引用、页面/资源、绘制指令、字体与图像；按功能建立兼容性矩阵。
5. 原生书架、搜索、书签、高亮、笔记及可靠持久化。

不预先宣称完整 Unicode 排版、完整 EPUB/PDF 兼容或跨平台发布可用。新增格式必须有正常、损坏和资源超限测试。
