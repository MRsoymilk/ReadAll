# ReadAll

ReadAll 是一个用 Rust 自研的原生电子书阅读器。项目希望尽量自己完成文档容器解析、文本布局、字体解析、CPU 光栅化和原生窗口交互，不依赖 WebView，也不直接接入现成 EPUB/PDF 阅读引擎。

> **v0.1.0 是首个开发预览版。当前已验证的界面为 Linux Wayland；Android 基础 APK 已获用户确认可在真机打开 EPUB；本轮统一阅读 UI 的 ARM64 调试 APK 已构建，本轮真机交互验收仍待完成。Windows 和 PDF 仍在后续路线中。**

## Android 开发入口

Android 与 Linux 共用 `ReaderWindow` 阅读界面及同一个 `readall` 引擎：顶部书名/进度、底部可展开/收起工具栏、目录与设置面板、左右滑动/仿书/上下平滑滚动三种模式均走相同 Rust 绘制和交互逻辑。目录不再使用独立系统弹窗；较矮屏幕使用无重叠的紧凑面板。手机通过单指滑动翻页、长按拖选文字、共享操作条复制/高亮/笔记，并接入书内链接、图片查看、搜索及标注列表；系统键盘、剪贴板、浏览器确认后的启动、SAF 和生命周期由 Android 外壳处理。

手机首页保留原生“打开/继续/主题切换”，选区拖动柄、双指缩放和完整可访问性语义树尚未实现。绘制与触摸共用密度换算，最新帧有界传递，背景暂停及输入取消不会积压无限任务。像素一致性和 JVM/JNI 回归不代替实际手机帧率测试；没有引入 WebView、AndroidX 或 Gradle。

Android 已分离逻辑排版与设备像素：保留字号和 UI 比例，正文轮廓、文字与图标按实际像素重新绘制，不再把低分辨率整页放大；输出有 4M 像素上限。原生阅读会话在排版前预留标题栏空间，修正小边距下连续滚动页缝截掉首行上半部的问题。密度变化不改变逻辑分页；界面预留区域修正可能重新分页，但以内容锚点恢复进度。本轮手机清晰度和帧率仍待验收。

构建工具全部通过绝对路径参数调用，不修改 shell 环境变量。见 [Android 构建、SDK 可见性及验收说明](apps/android/README.md)。

```bash
/usr/bin/python3 apps/android/tools/build.py doctor --sdk /opt/android-sdk
/usr/bin/python3 apps/android/tools/build.py build --sdk /opt/android-sdk
```

ARM64 产物为 `target/android/readall-android-debug-arm64-v8a.apk`。系统 Rust 缺少 Android 标准库时，可显式运行 `prepare-rust` 将完全匹配的官方目标库下载到项目缓存，不安装 rustup、不替换系统 Rust；构建通过目标专用 `--sysroot` 使用它。Java Lambda 编译已接入 SDK 的 `core-lambda-stubs.jar`。宿主机 `host-test` 和 APK 构建校验不代替手机安装与交互测试。

## v0.1.0 已实现

### 原生 Linux 阅读界面

- 零参数启动原生书库：`./readall`
- 内置文件浏览器，可直接选择 `.epub` / `.mobi` / `.azw3`（及使用 BOOKMOBI 容器的旧式 `.azw` / `.prc`）
- 选中 EPUB / MOBI / AZW3 时按需预览书名 / 作者 / 语言；MOBI 预览不解压正文，解析失败不阻止尝试打开
- 持久化最近阅读列表，成功关闭 EPUB / MOBI / AZW3 后自动记录，可从首页直接继续打开
- 最近阅读每行右侧固定“删除”按钮：只移除历史记录，保留原书、进度、书签和标注；鼠标松开时确认，移出按钮可取消，保存失败保留原列表并提示
- 最近阅读长文件名在独立裁剪区域内自动左右往返滚动，两端短暂停顿；短文件名静止，按钮不会被遮挡，点击文件名仍打开原书。窗口宽度变化后重新判断是否滚动，动画只重绘改变的行，不逐帧读取文件或元数据
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
- 正文默认文字拖选，选字后显示复制 / 高亮 / 笔记操作条；Ctrl+C / Ctrl+V Wayland 剪贴板
- 图片点击查看、缩放与移动；链接图片优先打开书内目标
- 正文链接 / 脚注跳转，Backspace 或“返回”按钮回到原阅读位置
- HTTP/HTTPS 外链确认后交给默认浏览器；取消或启动失败不改变阅读位置
- 亮色 / 暗色两种主题，阅读界面、书库、目录、工具栏和加载提示使用统一配色；主题、字号、边距和行距持久化

### 亮色与暗色

阅读时打开底部“设置”，第一行“主题”通过加减按钮在“亮色 / 暗色”间切换；Linux 也可按 F6。Linux 书库左下角与 Android 首页均有主题切换按钮。默认亮色，手动选择会保存，关闭图书和重启应用后沿用，不随系统主题自动切换。

`apps/readall/src/reader_data/theme.rs` 是唯一配色来源，正文界面、目录活动行、工具栏、选区操作条、搜索/笔记/外链弹层和加载提示共用该配色。Android 通过 JNI 获取相同的颜色，首页、错误框、进度条、状态栏及导航栏的背景和图标明暗一起更新；系统文件选择器和输入法仍由各自应用管理。图片和书籍显式作者底色不做整页反相。

主题切换保留字号、边距、行距、翻页模式和内容锚点，不修改原书。旧 `paper` / `sepia` 配置可读取为亮色，读取不改写文件，显式保存时使用 `light` / `dark`。高密度渲染及三种翻页模式继续可用；主题回归包含像素配色、对比度、持久化和真实 JVM/JNI，手机视觉验收仍需覆盖安装后的实测。

### EPUB / MOBI / AZW3 加载状态与响应性

打开 EPUB / MOBI / AZW3 时先创建窗口并提交加载页，再开始读取文档；文件读取、章节解析、字体加载、排版与图片解码由单独的阅读工作线程执行。Wayland 事件处理留在窗口线程，加载期间仍能显示状态、接收窗口调整和取消操作。完成后在同一窗口切换正文，不再等全书目录和初始排版全部结束才出现阅读窗口。

加载页显示文件名、当前阶段、阶段耗时和进度条。读取按实际字节数推进，章节排版按正文位置推进，页面绘制按项目数推进；无法准确计数的阶段显示活动条，不用定时器制造百分比。百分比是**当前阶段**的进度，不是整本书加载完成百分比。点击“取消加载”或按 Esc 可协作式取消；取消检查在文件块读取、章节排版和绘制等边界执行，不强杀正在处理的数据。

正文首帧先于标注列表提供；目录第一次点击时才生成并在本次阅读中缓存，未打开章节不再阻塞首屏。首次排版直接使用已保存设置和合成器确认的窗口尺寸，避免默认尺寸排版、设置重排和窗口重排连续重复。UI 字体保留已解析视图，不再每次创建文本绘制器就重解析完整字体。

耗时的翻页、目录、搜索与窗口重排也在工作线程中处理；同尺寸更新保留上一帧并显示状态。窗口调整合并到最新尺寸，连续鼠标移动合并，输入队列和最新图像交换槽均有界，避免旧帧和重复排版积压。失败保留错误界面，支持“重试”和“返回 / 关闭”，不再仅在终端报错后消失。终端还会记录页面准备耗时。

当前仍按章节完成排版，不承诺任意大章节立即可读；单次底层解码或阻塞文件 I/O 的取消要等到下一个检查点。`--frames 1` 是首个**加载帧**的窗口协议测试，不代表已完成 EPUB 解码。TXT 的原生打开路径尚未迁移到这套 EPUB 工作线程流程。

### MOBI

新增原生 Rust `readall-mobi`，支持未加密的 **MOBI6/7**。PalmDB 必须有 `BOOKMOBI` 文件签名，扩展名本身不能证明格式兼容；普通 Palm PRC 数据库、KFX 和受 DRM 保护的图书会给出明确错误。独立 AZW3/KF8 走下述专用重建路径。双格式 MOBI/KF8 使用其中的 MOBI6/7 兼容部分，并提示未导入 KF8 专有布局。

支持未压缩、PalmDOC LZ77 和 HUFF/CDIC 正文，UTF-8 / Windows-1252 编码。UTF-8 在记录拼接后解码，避免中文字符恰好跨压缩记录时乱码。读取书名、作者、语言、EXTH 封面；`recindex` 图片转换为包内资源引用，`filepos` 链接以原始编码的字节偏移转换，避免中文和 HTML 实体改变跳转位置。识别 guide 指定的正文目录，否则从标题或章节生成目录。保留代码块换行/缩进，并兼容常见未加引号属性、大小写标签、未闭合段落和旧式 font/align 样式。

MOBI HTML 在内存中整理为有界 XHTML/EPUB 适配数据，复用已有的文字排版、图片解码、三种翻页模式、选字复制、链接返回、搜索、书签/高亮/笔记以及加载状态；**不修改原书，不在磁盘写中间 EPUB，不调用 Calibre、Kindle 工具或 WebView**。原文件名与路径仍用于文件浏览和最近阅读。适配数据包含源文件 SHA-256，生成顺序和时间字段固定，相同源文件的重复打开可恢复同一组阅读记录。进度内部复用 `epub-v1/v2/v3`，不是原始 MOBI 字节位置或 Kindle 同步位置；将来改变适配算法时需要显式迁移这套内部定位。

读取 MOBI 时会显示“解压 MOBI 正文 / 整理 MOBI 章节与链接 / 准备 MOBI 图片资源”等阶段。首次显示前仍需完成有界正文解压和适配，像素解码由原图片缓存按需执行；不声称直接随机分页解码 MOBI。默认限制：输入 128 MiB、正文 32 MiB、单图片 16 MiB、内存适配包 192 MiB、1024 个章节。压缩字典、递归、符号工作量、标签数量/深度也有上限，异常文件报错而不是无限递归或分配。脚本、iframe/object/embed 不执行，图片只来自原书记录，不下载远程内容。

MOBI6/7 HTML 是兼容子集，不保证还原所有出版工具生成的旧标签、复杂表格、字典/索引和嵌入字体；MOBI6/7 路径的独立二进制 INDX/NCX 目录及音视频尚未接入；KF8 专用重建独立实现，不改变旧 MOBI 的适配结果。部分书会得到基于正文目录/标题的扁平目录，而非原始层级。图片格式的支持范围与原 EPUB 引擎相同，无法解码的图片显示占位。不能把“支持 MOBI”理解成支持所有 Kindle 文件。

```bash
./target/release/readall /path/to/book.mobi
./target/release/readall open-mobi /path/to/book.mobi
./target/release/readall mobi-info /path/to/book.mobi
./target/release/readall mobi-text /path/to/book.mobi --spine 2
./target/release/readall render-mobi /path/to/book.mobi new-page.ppm --font /path/to/font.ttf
./target/release/readall search /path/to/book.mobi '查询内容'
```

`mobi-info` 仅读取元数据；`mobi-text --spine N` 导出第 N 个适配章节，N 从 1 开始，独立封面可能占首个章节。`open` 也会按识别出的文档扩展名转到共同阅读入口；无 GUI 构建仍可运行元数据、文本、渲染和搜索命令。仅指定文件打开 GUI 时默认使用捆绑的中文字体。

### AZW3 / KF8

支持未加密、可重排的 **独立 AZW3（MOBI version 8 / KF8）**，包括 `.AZW3` 大写扩展名。以 PalmDB/MOBI 内容识别实际格式，不只是将扩展名加入文件选择器；KF8 签名出现在 `.mobi` 中也会进入 KF8 路径。双格式 MOBI/KF8 仍按原约定读取旧 MOBI 部分，避免已有 MOBI 进度和标注失效，不在本轮自动切换为 KF8。

`readall-mobi/src/kf8/` 解析 INDX / TAGX / IDXT / CNCX、FDST 流表、章节 skeleton 与 fragment 表，按字节位置重建原 XHTML，再接入共同阅读器。NCX 索引的标题、父子关系和 `fid/offset` 转成可跳转目录；缺少有效目录时使用 guide 或章节标题。索引记录末尾的合法四字节对齐填充受限处理，截断、越界、循环引用和异常规模有明确错误。

`kindle:flow:` 和 `kindle:embed:` 转为只指向包内的 CSS、SVG、图片和字体资源；嵌套资源按引用收集，不递归展开循环。SVG 属性大小写与原 XHTML 保留，正文/代码中的相同字样不会作为 URI 被替换。`kindle:pos:fid:…:off:…` 转为确定性的本地位置锚点，支持正文点击、脚注跳转及返回，原有 id 不被覆盖。

支持 FONT 记录的普通存储、zlib 压缩及记录内置 key 的格式混淆解包；这不是 DRM 解密，受保护的书籍仍在读取正文前拒绝。静态 TrueType 字体复用现有 `@font-face`；解包出 CFF、WOFF/WOFF2、可变字体不代表这些字体能渲染，仍按现有字体引擎能力回退。CSS 同样仍是 ReadAll 的既有子集，不声称完整还原所有 Kindle 版式。

左右滑动、仿书翻页、上下平滑滚动、文字选择/复制、搜索、书签、高亮、笔记及图片查看均复用现有实现。读取与重建位于阅读工作线程，显示“解压 AZW3 正文 / 解析 KF8 章节与目录索引 / 重建 AZW3 章节与链接 / 准备 AZW3 样式、图片与字体”，可在检查点取消。首屏仍需完成有界的文档适配，不是随机访问或整本书零等待加载。

不修改原书、不生成磁盘中间 EPUB、不启动 Calibre/WebView。适配标识为 `readall-kf8-v1`，包含源文件身份；生成顺序固定，阅读位置内部复用 `epub-v1/v2/v3`，可重复打开恢复，不是 Kindle 云端位置。沿用 MOBI 输入/解压/包总量限制，KF8 额外限制索引条目、CNCX 文本、重建工作量和单章节大小。坏图片/字体会提示并降级；关键 skeleton/fragment 损坏不会被当作正常正文显示。

```bash
./target/release/readall /path/to/book.azw3
./target/release/readall open-azw3 /path/to/book.azw3
./target/release/readall azw3-info /path/to/book.azw3
./target/release/readall azw3-text /path/to/book.azw3 --spine 2
./target/release/readall render-azw3 /path/to/book.azw3 new-page.ppm --font /path/to/font.ttf
./target/release/readall search /path/to/book.azw3 '查询内容'
```

当前不支持固定版式 KF8、KFX、DRM、Kindle 字典索引/音视频、RESC 高级分页和 Kindle 位置同步。损坏或非 XML 兼容的重建 XHTML 仍可能不能阅读。本轮未新增第三方依赖；KF8 格式参考沿用 foliate-js 的 MIT 许可说明。

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
- PNG（含 Adam7）、JPEG、WebP、GIF、SVG 的透明度合成、等比例缩放和图文分页
- 常见 HTML / XHTML DOCTYPE
- 常见 legacy XHTML 命名实体（如 `&nbsp;`、`&mdash;`、`&hellip;`、`&copy;`），固定映射且不加载外部 DTD
- 跨 spine 连续阅读
- 纯图片 XHTML 与独立 SVG spine 可阅读、进入目录并保存进度
- 内联 SVG 与 SVG 中的包内栅格图片引用
- 自动跳过空 XHTML 和当前不可读 spine；缺失或暂不支持的图片显示占位提示
- 稳定文本定位：`epub-v1:<book-sha256>:<spine>:<utf8-offset>`
- 精确图片定位：`epub-v2:<book-sha256>:<spine>:<utf8-offset>:<image-index>`；仍读取旧 v1 进度
- 保留代码空白后的章节使用 `epub-v3:<book-sha256>:<spine>:<utf8-offset>:<image-index-or->`；旧 v1/v2 偏移自动转换

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
| `white-space` | normal / nowrap / pre / pre-wrap / pre-line / break-spaces 的空白处理；继承、initial、unset、`!important` |
| `display` | none；隐藏内容保留原文本偏移，但不参与可见绘制 |
| `font-weight` / `font-style` | normal / bold / 数字字重、normal / italic / oblique；优先真实字形变体，无匹配时有限合成 |
| `margin` / `padding` | 正长度、百分比、四边简写/单边；横向 auto 边距可居中 |
| `border` | 四边宽度、solid、颜色及其简写/单边覆盖 |
| `background` / `background-color` | 纯色或透明；不加载 CSS 背景图 |
| `width` / `max-width` | 正长度、百分比、auto（max-width 可 none） |
| `break-before` / `break-after` | page / auto，兼容 page-break-before/after 的 always |

默认区分 h1–h6 字号层级。块盒模型对 section、div、p 等块元素和 body 生效，包含嵌套内容宽度、边距、内边距、纯色背景与实线边框；跨页时延续背景和侧边框，不在切分处重复顶底装饰。极端装饰长度会受页面可用范围限制。尚无 margin collapsing、浮动、定位、负边距、完整表格布局、复杂选择器、`@import` / `@media`、完整字体简写和可变字体匹配；这不是完整浏览器 CSS 引擎。

代码块 `<pre>` / `<pre><code>` 默认保留换行、连续空格、Tab、空行和末尾空白，并作为独立块排版；语法高亮的嵌套 `<span>` 继承这些规则。`white-space` 可来自包内 CSS、`<style>` 或行内样式；`pre-line` 只保留换行并折叠空格。连续 `<br>` 不再合并成一个换行。XHTML 按 XML 内容处理，保留 `<pre>` 开头的源换行，CRLF / CR 规范化为 LF。普通正文和未声明预格式化的行内 `<code>` 仍折叠空白，不从 `#define` / 分号猜测源码断行。

代码空行使用完整行高；Tab 参与缩进定位。为避免分页阅读器中长代码被裁掉，默认 `<pre>` 使用 pre-wrap；显式 pre / nowrap 的过长行也执行视口内安全软换行，尚不提供水平滚动，因而不是完整浏览器 white-space 溢出实现。复制、搜索和 `epub-text` 使用保留空白的逻辑文本，视觉软换行不会额外写入复制结果。自动代码着色仅作用于绘制颜色，见下节。

### 代码语法高亮

代码块默认启用轻量词法着色，GUI 与 `render-epub` 共用实现，不需要按键开启或安装第三方语法高亮库。区分关键字、内置类型、注释、字符串、数字、函数/宏调用、预处理指令和大写常量；支持 C/C++、Rust、Python、Shell/Bash、JavaScript/TypeScript 和 JSON 的常见词法结构。它不是编译器或完整语法分析器：模板字符串内插、JavaScript 正则字面量、Shell here-document 等复杂结构尚未逐层解析，也不保证识别所有语言方言。

优先读取 `<pre>` 或其 `<code>` 的 `language-*`、`lang-*`、`highlight-source-*`、`data-language` / `data-lang`；也识别部分常见裸语言 class。没有声明时，依据 `#include` / `#define`、函数定义、shebang 等明显特征保守判断。显式未知语言、`language-text` / `language-plaintext` 或 `nohighlight` / `no-highlight` 保留原样；无明显代码特征的预格式化文字不强行着色。普通行内 `<code>` 不启用自动块级着色。

已有多色样式的代码块保留作者配色，不叠加自动高亮。自动配色根据 paper / sepia / dark 主题和实际作者块背景选择明暗方案；选区、高亮标注和链接命中继续使用原有坐标。只在章节准备时扫描一次整个代码块，跨行注释和字符串状态不会因分页丢失；翻页、重排与切换主题复用标记范围。不会修改正文、缩进、复制结果、搜索偏移、字体测量或 v1/v2/v3 阅读位置，不执行书中的代码。

代码元数据每章最多 1024 块；词法处理默认单块 256 KiB、单章累计 1 MiB、65,536 个着色标记。超过预算的块整体退回普通文字并给出提示，不保留半截着色，也不阻断加载。取消操作在代码块之间检查。

PNG 仍由 `readall-image` 自研解码，复用 ReadAll 的 DEFLATE。支持非交错和 Adam7 交错图像、灰度/RGB/索引色/alpha 的合法位深组合，校验 CRC、Adler-32 与资源预算。JPEG 通过独立的 `jpeg-decoder` 接入，涵盖基线、渐进、灰度、CMYK 及有界 EXIF 方向处理。图片按独立块排入正文、保留宽高比；缩放仍采用最近邻，没有完整色彩管理。

WebP 使用 `image-webp = 0.2.4`，支持有损/无损/透明通道，动画只显示首帧。SVG 通过隔离的 `resvg` / `roxmltree` 路径渲染向量、变换、裁剪和文本；内联 SVG 与独立 SVG 章节也进入图片定位。SVG 的图片只允许受预算约束的 EPUB 包内栅格资源，不读取外部文件、网络、DTD 或递归 SVG。JPEG/WebP/GIF/SVG 使用独立 Rust 依赖，不启动外部转换进程，也不接入完整 EPUB 阅读引擎。

GIF87a/89a 使用已由 `resvg` 引入的 `gif 0.14.2`，现由 `readall-image` 直接接入；不新增第三方包版本或外部程序。支持全局/局部调色板、透明索引、交错扫描和帧偏移，按逻辑画布显示首帧，未覆盖区域保持透明；动画 GIF 与 WebP 一样暂不播放。头部探测不解码像素，完整读取后检查容器边界、帧尺寸、块数、LZW 和输出预算，再进入原 LRU 缓存。

SVG 中的 GIF（包括内联 SVG、独立 SVG 文件和 data URI）也使用同一有界解码器。内联 SVG 的图片路径相对所在章节解析，独立 SVG 的子资源相对 SVG 本身解析；支持包内相对路径和百分号编码文件名，不放开文件系统或网络。资源失败会报告具体 href 和底层原因（manifest/ZIP 缺失、路径限制、校验错误、解码错误或预算），不再统一显示 `SVG image resource missing, disallowed or over budget`。data URI 不会完整写入日志，子资源数量在加载前限制。

图片和 CSS 只读取 EPUB manifest 中声明的包内资源，不下载远程 URI，不访问包外文件。单张栅格图片默认不超过 16 MiB / 8M 像素。分页进行有界头部探测（JPEG 可能需要扩展前缀，SVG 需要有界解析），绘制当前页才完整读取、校验并生成像素。ZIP 前缀只是未验证的尺寸提示，不能替代完整读取时的 CRC 校验。

RGBA 缓存使用 LRU，最多驻留 64 个资源且总量不超过 64 MiB。**64 是缓存容量，不是每章可阅读的图片数量。** 超限先淘汰旧图，回翻时重新解码；没有跨整章持续消耗的累计图片字节配额。仍保留每个 XHTML 最多 1024 个图片引用等结构保护。WebP 的单图尺寸与输出字节在分配前检查，并设置解码器内存限制；上游部分内部工作区尚不完全遵守该限制，因此缓存上限不代表进程总内存硬上限。

损坏、缺失、暂不支持的图片显示占位块，不阻断后续正文。失败状态与 RGBA 缓存分开，同一资源在当前章节实例内只报告一次；大量失败时限制终端日志数量。回翻不会反复解码同一损坏资源，也不会因前面有 64 张失败图片而跳过后面的有效图片。

盒模型和图片注释不向文本插入占位字符。未改变空白的章节继续使用 `epub-v1` 文本定位和 `epub-v2` 图片定位。代码空白或连续显式换行改变文本偏移时，使用 `epub-v3`；最后一段 `-` 表示文本位置，数字表示图片序号。旧 v1/v2 在旧折叠文本与新文本之间按字符顺序转换，不直接把旧数字当成新偏移；图片仍按序号精确恢复。旧高亮/笔记的两端一起转换，读取时仅更新内存，明确编辑标注后才原子写回；不修改 EPUB。旧版已经丢失的同一空白序列内精细位置无法完全恢复，会就近映射，非空白内容位置保持对应。页码会随字号与窗口变化，保存的是内容锚点，不是固定页码。

当前 EPUB **尚未完整渲染**：

- 完整 CSS
- AVIF 等其他图片格式、动画播放与完整色彩管理
- MathML
- WOFF/WOFF2、CFF/CFF2、可变字体及字体混淆的内嵌字体路径
- DRM / 加密 EPUB
- Fixed-layout EPUB

目录面板优先使用 EPUB 3 Navigation Document；没有 EPUB 3 nav 时会尝试 EPUB 2 NCX，再没有可用目录资源时才回退为从可读 spine 正文首行推断标题。两种正式目录都会保留嵌套层级；`href/src#fragment` 会解析到目标 XHTML 元素的 `id/xml:id`，再映射到规范化正文 UTF-8 offset，因此同一 XHTML 内的子目录也可以精确跳转，并使用对应文本版本的 locator 保持重排后的内容位置。fragment 缺失或无法解析时回退到目标 spine 开头。为兼容旧 XHTML，ReadAll 内置少量固定的常见命名字符实体；未知自定义实体、内部 DTD 子集和外部实体仍然拒绝，不会下载或展开外部 DTD。

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

内嵌字体会参与实际字宽测量、shaping 和分页，而不是只给文字换样式。`epub-v1` / `epub-v2` / `epub-v3` 记录内容锚点，字体变化不修改规范化文本。字体混淆（IDPF / Adobe）、WOFF/WOFF2、CFF/CFF2、可变字体与 DRM 仍不支持；含 `encryption.xml` 的书籍仍按原有策略拒绝，不把混淆字体误称为已支持。

### 搜索、标注与阅读设置

底部工具栏提供“查找 / 标注 / 设置”。搜索结果可直接跳转，标注面板支持查看、跳转和删除书签/高亮/笔记。**EPUB 正文默认可以按住左键拖选，不需要先按 F9；单击正文或左右空白不再翻页。** 普通文字单击可选中一个字形对应的文本簇，拖动可按字素边界扩展到同页多行，支持反向拖选。松开后出现“复制 / 高亮 / 笔记 / 取消”操作条，也可使用 Ctrl+C / F8 / F7；复制、存储失败等反馈显示在操作条内。点击空白或 Esc 清除当前选择，不关闭图书。分页模式选择限当前页；连续滚动时可选择当前视口内同一章节的相邻页文字，不跨章节建立选区。

F9 保留为“文字选择优先”开关，用于单击选择链接文字；关闭该开关后普通文字仍可拖选。正常模式下链接和图片在鼠标松开时才激活；拖动链接文字只做选择，不跳转。拖动期间的滚轮/翻页键不会让选择跳到另一页；松开后仍用滚轮、方向键、PageUp/PageDown 或工具栏翻页。输入合并保留首次拖动越界事件，页面忙碌及队列拥堵时保留松开事件，避免拖动被误当作点击或松手后继续选择。图片查看仍支持缩放与平移。

主题为 paper / sepia / dark，支持字号、页边距、行距和翻页模式。默认设置和标注存放在 `$XDG_STATE_HOME/readall/library-v1`；未设置绝对 XDG_STATE_HOME 时使用 `$HOME/.local/state/readall/library-v1`。可用 `--data-dir DIR` 隔离标注/设置；它与保存翻页进度的 `--state-dir` 不同。写入采用协作锁和原子替换，损坏数据会报告而不是悄悄覆盖。

输入使用合成器提供的 XKB keymap；Wayland 剪贴板已接入，但 text-input/输入法组合协议尚未实现。中文查询与笔记可通过 Ctrl+V 粘贴。

### 翻页模式与连续滚动

打开 EPUB 后按 **F5**（或底部“设置”），选择第五行“翻页模式”，点击右侧加减按钮或按 `+` / `-` 切换。支持以下三种模式，默认“左右滑动”；选择与现有阅读设置一起保存，旧 settings.conf 没有这一项时使用默认值。

| 模式 | 行为 | 鼠标/触控板 |
| --- | --- | --- |
| 左右滑动（slide） | 上下页沿水平方向平移，约 240ms 时间驱动缓动 | 滚轮、触控板横向滑动或按住右键左右拖动 |
| 仿书翻页（book） | 曲线折边、纸背和阴影的 2D 翻纸近似，不是完整 3D 物理纸张模拟 | 与左右滑动相同；短距离右拖松手回弹 |
| 上下平滑滚动（scroll） | 页面连续拼接，保留部分页位移，跨页/跨章滚动，不是一次滚轮强制翻一整页 | 滚轮或触控板上下滑动；按住右键上下拖动 |

左键仍用于文字选择、链接和图片，不恢复“点击正文左右两侧翻页”。滚动/动画中的 Esc 先停止当前移动。方向键、PageUp/PageDown、Space 和翻页按钮在分页模式切换整页，在滚动模式移动约一屏；Home/End 到书籍首尾。模式切换不改变字号，保留内容锚点。

滚动模式复用现有分页画面组成连续页面带，不把整本书生成一张超长位图，页间保留原有留白。当前页及前后相邻页最多三个页面槽，只有跨章相邻页另持有对应章节/字体/排版；不预先渲染整本书。空闲时准备相邻页，新输入可在检查点抢占预读，预读不会覆盖加载提示或改变进度。缓存命中后的移动使用裁剪行拷贝，不重新解码字形和图片；新章节或大图未缓存时仍可能需要等待该页准备，底层解码的协作取消不是硬实时中断。

移动按经过时间计算，滚动目标平滑追随连续的 Wayland 位移，连续滚轮输入合并且保留松开边界。阅读窗口动画调度目标为约 16ms，实际显示帧率还取决于 CPU、窗口尺寸和合成器；这不是 GPU 渲染或帧率保证。右上角百分比与底部进度条在滚动中按部分页位置更新。停稳或开始其他操作时才保存进度，不逐帧写盘；保存的是可见内容的文本/图片锚点，重新打开时按锚点恢复，不承诺同一行或图片内部的精确像素位置。

命令行同样可设置（下次打开生效，不远程改变已经打开的窗口）：

```bash
./target/release/readall settings page-mode slide
./target/release/readall settings page-mode book
./target/release/readall settings page-mode scroll
```

本轮模式适用于原生 EPUB 阅读器，TXT 诊断窗口保留原交互。显式性能检查：`cargo test -p readall --release cached_reader_motion_benchmark -- --ignored --nocapture`，测量缓存帧的 CPU 合成，不是桌面实际 FPS。

### 正文链接与脚注返回

XHTML `<a href>` 的文字和所包裹图片已接入点击命中，下划线提示可点击区域；保留原正文偏移，不向文本插入链接标记。支持当前章节 `#fragment`、包内相对路径、UTF-8 / 百分号编码目标，以及 spine 中 `linear="no"` 的注释章节。除元素 `id` / `xml:id` 外，也识别旧式 `<a name="...">` 锚点；同名时优先匹配 id。`epub:type="noteref"` / `role="doc-noteref"` 识别为注释引用。当前脚注采用跳转阅读，不是弹出式脚注预览。

跳转后可按 Backspace 或点击提示栏右侧“返回”。返回栈最多 64 层，保留原文本或 `epub-v2` 图片锚点；返回栈不持久化，当前阅读位置仍正常保存。正文拖选优先于链接激活；F9 可进一步启用单击选取链接文字的模式。file/javascript/data 等不受支持的 URI、包外路径、缺失 fragment 和不在 spine 的目标显示错误并保留当前页面，不会误作翻页。尚未实现非 spine 注释弹窗、SVG 内部链接命中和正文链接的键盘焦点遍历。

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
| 下一页 | PageDown / → / ↓ / Space / 滚轮向下 / “下一页”按钮 |
| 上一页 | PageUp / ← / ↑ / 滚轮向上 / “上一页”按钮 |
| 第一页 | Home |
| 最后一页 | End |
| 字号 | `+` / `-` 或底部工具栏 |
| 目录 | 底部“目录” |
| 返回链接来源 / 返回书库 | Backspace（先逐层返回链接来源，再返回书库） |
| 搜索 / 标注 / 添加书签 | F2 / F3 / F4 |
| 阅读设置 / 切换主题 | F5 / F6 |
| 翻页模式 | F5 → 第五行“翻页模式” → `+` / `-`；选择自动保存 |
| 拖动翻页 / 滚动 | 按住右键；左键仍选择正文 |
| 选择正文 | 直接按住左键拖动；松开显示选字操作条 |
| 笔记 / 高亮 / 文字选择优先 | F7 / F8 / F9；也可点击选字操作条 |
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
./target/release/readall settings page-mode scroll
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
│   ├── readall-image/      # PNG/JPEG/WebP/GIF/SVG dispatch / RGBA pixels
│   ├── readall-mobi/       # PalmDB / PalmDOC / HUFF-CDIC / legacy HTML adapter
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
- Windows 窗口后端尚未实现；Android 是单独开发入口，ARM64 调试 APK 已构建验证，真机验收尚未完成
- PDF 尚未实现
- MOBI 支持未加密 MOBI6/7，AZW3 支持独立可重排 KF8；固定版式 KF8、KFX、DRM、字典专用索引和完整 Kindle 版式尚未实现
- EPUB 已支持 CSS 文本/块子集、PNG/JPEG/WebP/GIF/SVG；完整 CSS、MathML、固定版式与更多内嵌字体格式尚待实现
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

JPEG/WebP/GIF/SVG、shaping、Unicode、XKB 和剪贴板功能使用独立依赖，版本固定在 `Cargo.lock`。主要包括 `jpeg-decoder`、`image-webp`、`gif`、`resvg`、`roxmltree`、`rustybuzz`、`unicode-bidi`、`unicode-script`、`unicode-linebreak`、`unicode-segmentation`、`xkbcommon` 与 `wl-clipboard-rs`。项目不再是零第三方运行时依赖；发布时需按各 crate 的许可证保留相应许可文本。MOBI / KF8 实现参考 foliate-js 的 PalmDOC / HUFF-CDIC、INDX 与 KF8 重建格式，保留其 MIT 许可；HTML 实体处理使用固定版本 `html-escape = 0.2.13`（及间接依赖 `utf8-width`），不引入整个第三方电子书引擎。`readall licenses` 显示内置字体与 foliate-js 参考许可，不是所有 Cargo 依赖的完整许可清单。

已捆绑的第三方资源分别遵循其自己的许可证：

- **LXGW WenKai Lite Regular**：SIL Open Font License 1.1  
  许可证：`licenses/LXGW_WenKai_Lite_OFL.txt`
- **foliate-js MOBI 解压算法参考**：MIT  
  许可证：`licenses/foliate-js-MIT.txt`
- **Feather Icons v4.29.2**：MIT  
  许可证：`res/icons/reader/LICENSE`

运行：

```bash
./target/release/readall licenses
```

可查看内置字体与 MOBI 算法参考许可证。

---

Repository: https://github.com/MRsoymilk/ReadAll
