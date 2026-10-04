# ReadAll

ReadAll 是一个用 Rust 自研的原生电子书阅读器。项目希望尽量自己完成文档容器解析、文本布局、字体解析、CPU 光栅化和原生窗口交互，不依赖 WebView，也不直接接入现成 EPUB/PDF 阅读引擎。

> **v0.1.0 是首个开发预览版。当前重点是 Linux Wayland + EPUB/TXT；Windows、Android 和 PDF 仍在后续路线中。**

## v0.1.0 已实现

### 原生 Linux 阅读界面

- 零参数启动原生书库：`./readall`
- 内置文件浏览器，可直接选择 `.epub`
- 选中 EPUB 时按需预览 OPF 书名 / 作者 / 语言，解析失败不阻止打开
- 持久化最近阅读列表，成功关闭 EPUB 后自动记录，可从首页直接继续打开
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
- 窗口尺寸、字号变化后保持内容锚点；连续图片页使用独立图片定位
- 原生全文搜索与结果跳转，书签 / 高亮 / 笔记的保存、查看、删除
- F9 文字拖选，Ctrl+C / Ctrl+V Wayland 剪贴板
- 图片点击查看、缩放与移动；链接图片优先打开书内目标
- 正文链接 / 脚注跳转，Backspace 或“返回”按钮回到原阅读位置
- HTTP/HTTPS 外链确认后交给默认浏览器；取消或启动失败不改变阅读位置
- 纸色 / 护眼 / 深色主题，字号、边距和行距设置持久化

### EPUB 加载状态与响应性

打开 EPUB 时先创建窗口并提交加载页，再开始读取文档；文件读取、章节解析、字体加载、排版与图片解码由单独的阅读工作线程执行。Wayland 事件处理留在窗口线程，加载期间仍能显示状态、接收窗口调整和取消操作。完成后在同一窗口切换正文，不再等全书目录和初始排版全部结束才出现阅读窗口。

加载页显示文件名、当前阶段、阶段耗时和进度条。读取按实际字节数推进，章节排版按正文位置推进，页面绘制按项目数推进；无法准确计数的阶段显示活动条，不用定时器制造百分比。百分比是**当前阶段**的进度，不是整本书加载完成百分比。点击“取消加载”或按 Esc 可协作式取消；取消检查在文件块读取、章节排版和绘制等边界执行，不强杀正在处理的数据。

正文首帧先于标注列表提供；目录第一次点击时才生成并在本次阅读中缓存，未打开章节不再阻塞首屏。首次排版直接使用已保存设置和合成器确认的窗口尺寸，避免默认尺寸排版、设置重排和窗口重排连续重复。UI 字体保留已解析视图，不再每次创建文本绘制器就重解析完整字体。

耗时的翻页、目录、搜索与窗口重排也在工作线程中处理；同尺寸更新保留上一帧并显示状态。窗口调整合并到最新尺寸，连续鼠标移动合并，输入队列和最新图像交换槽均有界，避免旧帧和重复排版积压。失败保留错误界面，支持“重试”和“返回 / 关闭”，不再仅在终端报错后消失。终端还会记录页面准备耗时。

当前仍按章节完成排版，不承诺任意大章节立即可读；单次底层解码或阻塞文件 I/O 的取消要等到下一个检查点。`--frames 1` 是首个**加载帧**的窗口协议测试，不代表已完成 EPUB 解码。TXT 的原生打开路径尚未迁移到这套 EPUB 工作线程流程。

### EPUB

ReadAll 当前自己处理：

- ZIP 中央目录和本地文件头
- Store / raw DEFLATE
  - Stored block
  - Fixed Huffman
  - Dynamic Huffman
- CRC-32、路径安全和资源预算
- `mimetype`
- `META-INF/container.xml`，支持多个 `rootfile` 并按顺序选择第一个可解析 rendering
- OPF metadata / manifest / spine，读取 title / creator / language
- EPUB 3 Navigation Document：`properties="nav"` + `nav epub:type="toc"`
- EPUB 2 NCX fallback：`spine toc` + `navMap/navPoint/navLabel/content`
- 正式目录标题、嵌套层级与 `href#fragment` 目标解析
- XHTML 可见正文提取，并保留样式范围、图片位置和目录锚点
- CSS 子集：级联、标题字号、粗斜体、颜色、对齐、缩进、行高及嵌套块盒模型
- 书籍静态 TrueType 内嵌字体：`@font-face`、字体族列表、样式匹配和缺字回退
- PNG（含 Adam7）、JPEG、WebP、SVG 的透明度合成、等比例缩放和图文分页
- 常见 HTML / XHTML DOCTYPE
- 常见 legacy XHTML 命名实体（如 `&nbsp;`、`&mdash;`、`&hellip;`、`&copy;`），固定映射且不加载外部 DTD
- 跨 spine 连续阅读
- 纯图片 XHTML 与独立 SVG spine 可阅读、进入目录并保存进度
- 内联 SVG 与 SVG 中的包内栅格图片引用
- 自动跳过空 XHTML 和当前不可读 spine；缺失或暂不支持的图片显示占位提示
- 稳定文本定位：`epub-v1:<book-sha256>:<spine>:<utf8-offset>`
- 精确图片定位：`epub-v2:<book-sha256>:<spine>:<utf8-offset>:<image-index>`；仍读取旧 v1 进度

### CSS 子集与图文排版

GUI 与 `render-epub` 使用同一套图文分页实现；`epub-text` 仍导出规范化纯文本。

CSS 从 XHTML 的 `<style>`、行内 `style` 和包内 `<link rel="stylesheet">` 读取。支持标签、`.class`、`#id`、`p.note` 等复合选择器及逗号分组，按属性处理继承、选择器优先级、源顺序和 `!important`。不支持的选择器和 at-rule 整体忽略，不会把 `p:hover` 或 `div p` 错当成全局规则。

| 属性 | 当前支持 |
| --- | --- |
| `color` | `#RGB` / `#RRGGBB`、整数 `rgb()`、常见命名颜色 |
| `font-family` | 书内命名字体族、带引号/多词名称、逗号回退列表、继承/initial；通用族使用阅读器回退链 |
| `font-size` | `px` / `em` / `%`；CSS 16px 作为阅读基准字号，随阅读字号缩放 |
| `text-align` | left / center / right；当前为从左到右排版 |
| `text-indent` | `px` / `em` 正首行缩进；最多占可用宽度的一半 |
| `line-height` | normal / 有限范围的无单位倍数 |
| `display` | none；隐藏内容保留原文本偏移，但不参与可见绘制 |
| `font-weight` / `font-style` | normal / bold / 数字字重、normal / italic / oblique；优先真实字形变体，无匹配时有限合成 |
| `margin` / `padding` | 正长度、百分比、四边简写/单边；横向 auto 边距可居中 |
| `border` | 四边宽度、solid、颜色及其简写/单边覆盖 |
| `background` / `background-color` | 纯色或透明；不加载 CSS 背景图 |
| `width` / `max-width` | 正长度、百分比、auto（max-width 可 none） |
| `break-before` / `break-after` | page / auto，兼容 page-break-before/after 的 always |

默认区分 h1–h6 字号层级。块盒模型对 section、div、p 等块元素和 body 生效，包含嵌套内容宽度、边距、内边距、纯色背景与实线边框；跨页时延续背景和侧边框，不在切分处重复顶底装饰。极端装饰长度会受页面可用范围限制。尚无 margin collapsing、浮动、定位、负边距、完整表格布局、复杂选择器、`@import` / `@media`、完整字体简写和可变字体匹配；这不是完整浏览器 CSS 引擎。

PNG 仍由 `readall-image` 自研解码，复用 ReadAll 的 DEFLATE。支持非交错和 Adam7 交错图像、灰度/RGB/索引色/alpha 的合法位深组合，校验 CRC、Adler-32 与资源预算。JPEG 通过独立的 `jpeg-decoder` 接入，涵盖基线、渐进、灰度、CMYK 及有界 EXIF 方向处理。图片按独立块排入正文、保留宽高比；缩放仍采用最近邻，没有完整色彩管理。

WebP 使用 `image-webp = 0.2.4`，支持有损/无损/透明通道，动画只显示首帧。SVG 通过隔离的 `resvg` / `roxmltree` 路径渲染向量、变换、裁剪和文本；内联 SVG 与独立 SVG 章节也进入图片定位。SVG 的图片只允许受预算约束的 EPUB 包内栅格资源，不读取外部文件、网络、DTD 或递归 SVG。JPEG/WebP/SVG 使用独立 Rust 依赖，不启动外部转换进程，也不接入完整 EPUB 阅读引擎。

图片和 CSS 只读取 EPUB manifest 中声明的包内资源，不下载远程 URI，不访问包外文件。单张栅格图片默认不超过 16 MiB / 8M 像素。分页进行有界头部探测（JPEG 可能需要扩展前缀，SVG 需要有界解析），绘制当前页才完整读取、校验并生成像素。ZIP 前缀只是未验证的尺寸提示，不能替代完整读取时的 CRC 校验。

RGBA 缓存使用 LRU，最多驻留 64 个资源且总量不超过 64 MiB。**64 是缓存容量，不是每章可阅读的图片数量。** 超限先淘汰旧图，回翻时重新解码；没有跨整章持续消耗的累计图片字节配额。仍保留每个 XHTML 最多 1024 个图片引用等结构保护。WebP 的单图尺寸与输出字节在分配前检查，并设置解码器内存限制；上游部分内部工作区尚不完全遵守该限制，因此缓存上限不代表进程总内存硬上限。

损坏、缺失、暂不支持的图片显示占位块，不阻断后续正文。失败状态与 RGBA 缓存分开，同一资源在当前章节实例内只报告一次；大量失败时限制终端日志数量。回翻不会反复解码同一损坏资源，也不会因前面有 64 张失败图片而跳过后面的有效图片。

样式、盒模型和图片注释不向规范化文本插入占位字符。文本位置仍使用 `epub-v1`；图片页新增 `epub-v2`，包含章节内的图片序号，解决连续无文字图片共用偏移的问题。重排、重启和图片书签会保留图片目标；旧 v1 进度继续兼容。页码会随字号与窗口变化，保存的是内容锚点，不是固定页码。

当前 EPUB **尚未完整渲染**：

- 完整 CSS
- GIF / AVIF 等其他图片格式、动画播放与完整色彩管理
- MathML
- WOFF/WOFF2、CFF/CFF2、可变字体及字体混淆的内嵌字体路径
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

EPUB 正文已接入 `rustybuzz` shaping、`unicode-bidi` 双向重排、按脚本切分，以及 `unicode-linebreak` / `unicode-segmentation` 的断行和字素簇边界。按 shaping 返回的字形 ID、advance 和 offset 绘制，保留逻辑文本范围用于定位与选择。回退字体优先覆盖完整字素簇；真实粗斜体优先，无适合变体时使用受限合成。仍非完整排版规范实现：竖排、两端对齐、自动断词和所有复杂书写系统组合尚未全面验收。TXT 页面仍使用原有标量字形绘制路径，但换行已采用单词/字素边界。

原生书库启动会有界扫描常见系统 TTF/TTC；也可设置 `READALL_FALLBACK_FONTS`（系统路径分隔符分隔），或为 `open-epub` / `render-epub` 重复指定 `--fallback-font FILE`。显式导出不自动扫描系统字体，以便结果可复现。最多加载 12 个回退字体、合计 96 MiB；当前选 TTC 第一个 face。正文轮廓引擎仍不支持 CFF/CFF2、WOFF/WOFF2、可变字体和 hinting；字体缺少所需字形时仍可能出现替代字形。

### 书籍内嵌字体

EPUB 正文支持未加密的静态 TrueType 字体（TTF、采用 glyf 轮廓的 OpenType，以及 TTC 的第一个 face）。`@font-face` 从内联 `<style>` 或包内外链样式表读取；相对 `src: url(...)` 以声明所在样式表为基准，内联样式以 XHTML 为基准。保留文件名大小写，不使用系统路径或网络 URL。支持按顺序尝试多个 URL；缺失、损坏或不受支持的候选文件会报告原因，并继续尝试后续来源。

`font-family` 支持命名字体族、带引号的名称、逗号列表、继承、`initial` / `unset` 和 `!important`。按字体族顺序及普通/粗体、正体/斜体描述符选择；描述符优先于字体文件内部的样式标记。当前数字字重仍折合普通/粗体两档，并非完整 CSS 字重距离算法。支持有限的 `unicode-range` 列表及尾部问号通配范围；优先用同一字体覆盖整个字素簇，再走原有回退链。系统命名字体查询、`local()`、`font` 简写、可变权重范围、字体集合片段选择和 SVG 内部的作者字体映射尚未接入。

只加载当前章节可见正文实际引用的字体族。单字体最多 16 MiB，每章最多 16 个成功读取的不同字体文件、64 MiB 累计字体输入、64 次来源读取尝试，最多 32 个字体声明；失败内容也计入输入预算。超预算或加载失败不会把阅读器默认字体替换掉。所有来源必须出现在 EPUB manifest 中，并完整验证 ZIP CRC。字体解析结果及字形缓存共享只读存储，不反复复制整份字体；切换章节会清除旧的字体选择和字形缓存，避免同名族跨章节串用。

内嵌字体会参与实际字宽测量、shaping 和分页，而不是只给文字换样式。`epub-v1` / `epub-v2` 仍记录原内容锚点，字体变化不修改规范化文本。字体混淆（IDPF / Adobe）、WOFF/WOFF2、CFF/CFF2、可变字体与 DRM 仍不支持；含 `encryption.xml` 的书籍仍按原有策略拒绝，不把混淆字体误称为已支持。

### 搜索、标注与阅读设置

底部工具栏新增“查找 / 标注 / 设置”。搜索结果可直接跳转，标注面板支持查看、跳转和删除书签/高亮/笔记；F9 开关选择模式，拖动选择后可复制、高亮或添加笔记。选择目前限当前页。点击图片打开查看面板，支持缩放与平移。

主题为 paper / sepia / dark，支持字号、页边距和行距。默认设置和标注存放在 `$XDG_STATE_HOME/readall/library-v1`；未设置绝对 XDG_STATE_HOME 时使用 `$HOME/.local/state/readall/library-v1`。可用 `--data-dir DIR` 隔离标注/设置；它与保存翻页进度的 `--state-dir` 不同。写入采用协作锁和原子替换，损坏数据会报告而不是悄悄覆盖。

输入使用合成器提供的 XKB keymap；Wayland 剪贴板已接入，但 text-input/输入法组合协议尚未实现。中文查询与笔记可通过 Ctrl+V 粘贴。

### 正文链接与脚注返回

XHTML `<a href>` 的文字和所包裹图片已接入点击命中，下划线提示可点击区域；保留原正文偏移，不向文本插入链接标记。支持当前章节 `#fragment`、包内相对路径、UTF-8 / 百分号编码目标，以及 spine 中 `linear="no"` 的注释章节。除元素 `id` / `xml:id` 外，也识别旧式 `<a name="...">` 锚点；同名时优先匹配 id。`epub:type="noteref"` / `role="doc-noteref"` 识别为注释引用。当前脚注采用跳转阅读，不是弹出式脚注预览。

跳转后可按 Backspace 或点击提示栏右侧“返回”。返回栈最多 64 层，保留原文本或 `epub-v2` 图片锚点；返回栈不持久化，当前阅读位置仍正常保存。F9 选择模式优先于链接操作。file/javascript/data 等不受支持的 URI、包外路径、缺失 fragment 和不在 spine 的目标显示错误并保留当前页面，不会误作翻页。尚未实现非 spine 注释弹窗、SVG 内部链接命中和正文链接的键盘焦点遍历。

HTTP/HTTPS 外链（含链接图片）首次点击显示确认窗口，独立显示规范化目标站点和完整网址；长网址可用 ↑↓ / Home / End 查看、Ctrl+C 复制。点击“在浏览器打开”或 Enter 才调用系统 `xdg-open`，Esc / Backspace / “取消”返回阅读；取消不会退出阅读器。`//host/path` 链接在明确确认时使用 HTTPS。外链操作不改变正文锚点或返回栈，也不会把网页下载进阅读器。

网页地址使用 GUI 可选依赖 `url = 2.5.8` 解析，保留查询和 fragment、支持 Unicode 路径及域名，拒绝凭据、控制字符和模糊地址。启动器采用独立参数调用，不将书籍内容传入 shell。浏览器启动在有界后台任务中进行，失败显示原因；启动器长时间不退出时不阻塞窗口，也不会杀死用户浏览器。外链需要桌面会话中的 `xdg-open`（xdg-utils）和已配置的默认浏览器；没有它仍可正常阅读和跳转书内链接。

## 构建

### 要求

- Rust **1.88+**（源码已使用 let chains；本轮在当前工具链验证，最低版本仍需独立 CI 验证）
- Linux Wayland
- 系统 `libwayland-client`、`libxkbcommon`

Gentoo：

```bash
sudo emerge --ask dev-libs/wayland x11-libs/libxkbcommon
```

Debian / Ubuntu：

```bash
sudo apt install libwayland-dev libxkbcommon-dev
```

### 编译

```bash
git clone https://github.com/MRsoymilk/ReadAll.git
cd ReadAll
cargo build --release
./target/release/readall
```

普通 `--release` 保持 `opt-level=3`，采用 16 个 codegen units，不启用跨 crate 的链接时优化，以降低大依赖图的发布构建压力。资源受限环境可用 `cargo build --release -j 1`。原先的 thin LTO / 单 codegen unit 配置保留为可选 `cargo build --profile release-lto`（输出在 `target/release-lto/`），需要单独验证该构建配置的资源消耗和性能收益。

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
| 返回链接来源 / 返回书库 | Backspace（先逐层返回链接来源，再返回书库） |
| 搜索 / 标注 / 添加书签 | F2 / F3 / F4 |
| 阅读设置 / 切换主题 | F5 / F6 |
| 笔记 / 高亮 / 选择模式 | F7 / F8 / F9 |
| 复制 / 粘贴 | Ctrl+C / Ctrl+V |
| 图片查看 | 点击图片；+/− 缩放，↑↓ 平移，Home/End 水平移动 |
| 关闭面板 / 退出选择 / 关闭阅读器 | Esc 按当前状态逐级处理 |

CLI 帮助：

```bash
./target/release/readall --help
```

一些开发/诊断命令：

```bash
./target/release/readall epub-info /path/to/book.epub
./target/release/readall epub-text /path/to/book.epub --spine 1
./target/release/readall search /path/to/book.epub '查询内容'
./target/release/readall annotations /path/to/book.epub
./target/release/readall settings theme dark
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
│   ├── readall-image/      # PNG/JPEG/WebP/SVG dispatch / RGBA pixels
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
- EPUB 已支持 CSS 文本/块子集、PNG/JPEG/WebP/SVG；完整 CSS、MathML、固定版式与更多内嵌字体格式尚待实现
- 已有 shaping / bidi / 字素断行 / 有界字体回退，但竖排、完整排版规范和更多语言仍需验收
- 已有搜索、书签、高亮、笔记、设置和正文链接/脚注跳转；封面墙、弹出脚注、跨页选择尚未实现
- Wayland 输入法组合协议、动画播放、完整色彩管理尚未实现
- HiDPI / 分数缩放仍需继续完善

## Roadmap

1. 弹出脚注与非 spine 注释、字体混淆/WOFF、完整 CSS 表格与分页约束
2. 封面书架、跨页选择、输入法组合与 HiDPI/分数缩放
3. 更多图片格式、高质量缩放、色彩管理、更多语言排版验收
4. Windows 原生窗口后端
5. Android 平台入口
6. PDF 对象、页面、字体、图像和绘制指令

## 许可证与第三方资源

ReadAll 项目源码目前**尚未声明统一的开源许可证**。在明确项目代码许可证之前，请不要假定项目源码可以按 MIT/Apache/GPL 等许可证再分发。

JPEG/WebP/SVG、shaping、Unicode、XKB 和剪贴板功能使用独立依赖，版本固定在 `Cargo.lock`。主要包括 `jpeg-decoder`、`image-webp`、`resvg`、`roxmltree`、`rustybuzz`、`unicode-bidi`、`unicode-script`、`unicode-linebreak`、`unicode-segmentation`、`xkbcommon` 与 `wl-clipboard-rs`。项目不再是零第三方运行时依赖；发布时需按各 crate 的许可证保留相应许可文本。`readall licenses` 当前显示内置字体许可，不是所有 Cargo 依赖的完整许可清单。

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
