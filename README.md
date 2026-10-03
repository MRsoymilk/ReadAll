# ReadAll

Rust 自研电子书阅读器，目标平台为 Linux、Windows、Android。文档解析、排版、绘制和阅读交互由项目自身实现，不使用 WebView 或现成 PDF/EPUB 引擎。

## 当前状态

项目处于基础引擎阶段，**已有命令行、真实字体 TXT 页面导出，以及可选的 Linux Wayland 原生 TXT 窗口和翻页交互**。窗口已通过编译与本地模拟合成器协议测试，尚未完成真实桌面目视验收；不能阅读 PDF/EPUB。不得把文件签名检测当作对应格式已经支持。

已实现：有大小上限的文档输入、平台无关的随机读取接口、UTF-8/UTF-8 BOM/UTF-16 BOM 文本解码、换行规范化、原始文件 SHA-256 内容标识、可序列化且校验文档/字符边界的文本定位、诊断分页与重排定位、真实字宽驱动的基础分页、自研 TrueType 字形抗锯齿绘制、CPU 矩形绘制/嵌套裁剪/透明度合成。

当前外部 crate 依赖为零，仅使用 Rust 标准库和工作区内部 crate。启用 `wayland` 功能时，会链接系统 `libwayland-client` 处理窗口协议与文件描述符传输；字体解析、排版和像素绘制仍由 Rust 自研代码完成。默认不启用窗口，不链接该显示库。核心/字体/渲染模块继续禁止 unsafe；平台层默认 deny，仅私有 `wayland` 模块允许必要 FFI，原始指针不暴露给应用。操作系统、系统库和标准库不属于“零依赖”承诺。

## 构建与运行

```sh
cargo test --workspace --offline
cargo run -p readall --offline -- inspect tests/fixtures/sample.txt
cargo run -p readall --offline -- read tests/fixtures/sample.txt --columns 40 --rows 8 --page 1
cargo run -p readall --offline -- render-demo target/calibration.ppm
cargo clippy --workspace --all-targets --offline -- -D warnings
cargo fmt --all -- --check
```

`read` 的页码从 1 开始；输出 `Start locator` 可通过 `--at 'txt-v1:…'` 恢复到包含对应内容的页面，允许同时更改 `--columns`/`--rows`。`--page` 与 `--at` 互斥。`render-demo` 输出图形校准 PPM，不是电子书页面；为保护文件，目标已存在时拒绝覆盖。

诊断分页按 ASCII 1 格、其他 Unicode 标量 2 格、Tab 4 格制表位估算宽度，仅验证源文本覆盖和位置映射。不等同于终端真实显示宽度，也不支持真实字体塑形、字素簇、双向排版、单词断行或完整 Unicode 规则。`render-text` 使用独立的真实字宽排版模块，不使用这个格数估算。

Cargo 声明 Rust 1.85 / edition 2024 作为最低目标；这不是已经完成所有工具链与系统验证的声明。Linux、Windows 和 Android 实机验证分别推进，不把宿主机编译等同于三端验收。

## 模块边界

| 模块 | 职责 |
| --- | --- |
| `readall-core` | 文档输入约束、格式模型、文本解析、内容位置；不依赖文件路径或窗口对象 |
| `readall-font` | 自研 TrueType/TTC 解析、Unicode 字形映射、真实字宽、简单及复合字形轮廓；不执行字体字节码 |
| `readall-render` | 自研 RGBA 像素缓冲区、矩形裁剪与合成、二次曲线字形光栅化和灰度蒙版；无 GPU 或窗口呈现 |
| `readall-platform` | 本地文件访问、安全窗口接口、可选 Wayland 输入/共享内存呈现；Android URI 尚未实现 |
| `readall` | CLI、TXT 页面编排、字形缓存、图片导出和原生阅读会话；失败时保留原页面与位置 |

矩形绘制模块限制像素数量、指令数量、裁剪深度和累计混合像素数；`draw` 先检查整份矩形/裁剪指令，再修改像素，失败不会留下部分绘制结果。字形绘制逐次检查蒙版与裁剪；页面导出在内存中完成整页后才创建目标文件，输入解析或绘制失败不会生成页面文件，但磁盘写入失败仍可能留下新建的部分文件。透明度为直通 Alpha 的字节空间 source-over 合成，不提供线性光或完整 PDF 色彩管理。诊断分页默认最多 200,000 行。

文本读取默认限制原文件 32 MiB、解码后 64 MiB，调用方可配置。SHA-256 用于内容身份，不提供数字签名验证。读取能发现长度变化，但不承诺对正在修改的文件取得原子快照。

文本位置格式为 `txt-v1:<原始文件SHA-256>:<规范化UTF-8字节偏移>`。v1 去除编码 BOM，将 CRLF/CR 规范化为 LF；偏移不是 UTF-16 文件的原始字节位置，也不是字符序号或页码。原文件内容改变后，旧位置拒绝恢复。阅读进度自动持久化尚未实现。

UTF-16 必须带 BOM；GBK 等旧编码和 UTF-32 尚未支持。无效编码、NUL/终端控制字符会报错，而不是有损替换。`ZIP` 签名只说明可能为 EPUB，尚不验证 ZIP/EPUB 结构。

## 字体引擎

`readall-font` 已支持静态 TrueType sfnt 与 TTC 指定 face、Unicode `cmap` 4/12、`hhea/hmtx` 字宽、长短 `loca`、简单轮廓与复合轮廓（平移、缩放、矩阵变换和实际轮廓点对齐）。循环引用、截断数据、越界索引及超出深度/点数/组件预算会报错。解析格式参考 [OpenType 规范](https://learn.microsoft.com/en-us/typography/opentype/spec/)、[字符映射](https://learn.microsoft.com/en-us/typography/opentype/spec/cmap)与[字形轮廓](https://learn.microsoft.com/en-us/typography/opentype/spec/glyf)。

当前不支持 CFF/CFF2、WOFF、可变字体、字体塑形、hinting 字节码、phantom point 对齐或带变换的 USE_MY_METRICS。文件结构校验不等于字体真实性认证，也未实现字体校验和检查。字体加载上限默认 64 MiB；解析核心不访问系统路径。测试在 Rust 中构造小型样本，不将系统字体复制进项目。

```sh
cargo run -p readall-font --example inspect --offline -- --discover
cargo run -p readall-font --example inspect --offline -- /usr/share/fonts/dejavu/DejaVuSans.ttf
```

上面的实际字体路径取决于系统安装情况；缺字显示 `MISSING`，不把 .notdef 当作该字符已受支持。

## 真实字体 TXT 页面导出

```sh
cargo run -p readall --offline -- render-text tests/fixtures/sample_latin.txt target/text-page.ppm --font /usr/share/fonts/dejavu/DejaVuSans.ttf --font-size 24 --width 800 --height 1000
```

`render-text` 将文本通过自研字体解析、字宽测量、分页和像素合成输出为 PPM 图片。`--font` 必填；TTC 可用 `--face N` 选择字体。页码 `--page N` 从 1 开始，也可用 `--at 'txt-v1:…'` 恢复到包含内容位置的页面，两者互斥。可调整 `--width`、`--height`、`--font-size`、`--margin`。目标存在时拒绝覆盖，包括误把原书或字体路径作为输出。

默认遇到缺字直接报错。`--missing replacement` 是显式降级选项，使用该字体的 `.notdef` 轮廓并报告缺失字符，不静默忽略；尚无字体回退。示例字体已在开发宿主上验证拉丁文字、重音字符和希腊字母，但不包含中文。中文渲染需要含相应字形且采用当前支持轮廓格式的字体，不能仅凭文件扩展名判断。

字形绘制采用自适应二次曲线细分、非零环绕填充和 4×4 灰度采样，包含轮廓空洞、重叠与负侧边距处理。当前不执行 hinting、不做亚像素定位；字形基线取整数像素。每页按需缓存可见字形蒙版，限制独立字形数、解码点数、蒙版缓存和绘制工作量。

`MeasuredLayout` 接收真实字宽回调，不依赖特定字体或窗口。它保留规范化文本范围与 locator 映射，但当前仍按 Unicode 标量逐个换行，不支持完整单词断行、字素簇、字距调整、复杂文字塑形或双向排版；不能将图片导出等同于完整阅读体验。单次排版默认最多 1,000,000 个 Unicode 标量、200,000 行，超过限制明确报错。

## Linux 原生 TXT 窗口

在已登录的 Wayland 桌面终端中运行，系统须有可供链接的 `libwayland-client`：

```sh
cargo run -p readall --features wayland --offline -- open tests/fixtures/sample_latin.txt --font /usr/share/fonts/dejavu/DejaVuSans.ttf
```

`open` 复用 `render-text` 的字体、尺寸、页边距、`--page`/`--at` 和缺字策略参数，但不需要图片输出路径。标题包含页码和字号。PageUp/左/上翻到前页；PageDown/右/下/Space 翻到后页；Home/End 跳到首末页；加减号位置键调整字号；Esc 关闭。左键点击页面左半区/右半区翻页，竖向滚轮翻页。目前快捷键按 Linux 物理键码处理，不实现文字输入、键盘布局转换或长按自动重复。

窗口缩放和字号变化保留同一个精确文本 anchor，不反复替换成“当前屏幕第一页文字”，防止连续缩放后位置向前漂移。正常关闭后终端输出 `Reading locator`，下次可用 `--at` 恢复；尚未自动保存阅读进度。失败的翻页/字号修改保留原状态并在终端报错；无法适配的窗口尺寸会明确报错退出。

窗口采用 `wl_compositor` v4、`xdg-shell` v1 和 XRGB8888 SHM；输入需要 `wl_seat` v5。等待 configure 并 ack 后才附加缓冲区；同一时刻最多两个未释放缓冲区，释放前不覆盖其内容。临时文件以独占方式创建并立即解除路径关联。空闲时阻塞等待事件，不持续绘制。

`--display <socket>` 用于显式选择合成器，默认只使用调用进程的 Wayland 会话配置，不猜测用户 socket，也不修改桌面环境。`--frames 1` 是一次提交后关闭的协议诊断选项，只表示提交已由合成器处理，不代表用户已经看到画面。

```sh
cargo test --workspace --features wayland --offline
cargo clippy --workspace --all-targets --features wayland --offline -- -D warnings
cargo run -p readall-platform --example probe --offline
```

当前验证环境缺少 `WAYLAND_DISPLAY`、`XDG_RUNTIME_DIR` 和桌面 socket。测试使用真实 `libwayland-client` 连接 Rust 本地模拟合成器，覆盖 ping/pong、首次 configure、提交顺序、键盘事件、重配尺寸和 buffer release，**不等同于 Hyprland/Weston 等真实桌面的视觉/交互验收**。模拟测试不连接或操作用户桌面。接口依据 [Wayland 客户端 API](https://wayland.freedesktop.org/docs/html/apb.html)、[核心协议](https://wayland.freedesktop.org/docs/html/apa.html)及系统安装的稳定 xdg-shell 协议描述。

这一版尚无工具栏/书库、自绘窗口装饰、文字选择、字体回退、输入法、无障碍接口或 HiDPI/分数缩放适配。SHM 像素按 1:1 逻辑尺寸提交。文档和字体文件只读取一次，但翻页和重排仍重建测量布局，字形缓存尚未跨页复用；大文档性能需要继续优化。Windows/Android 窗口尚未实现。

## 后续顺序

1. 真实 Wayland 桌面验收，补足 HiDPI、原生界面与 Windows/Android 平台入口。
2. TXT 阅读：跨页缓存、字体回退、字素/单词断行、文字选择和阅读进度持久化。
3. EPUB：受限 ZIP、包结构、目录、XHTML/CSS 阅读子集、自研排版。
4. PDF：对象与交叉引用、页面/资源、绘制指令、字体与图像；按功能建立兼容性矩阵。
5. 原生书架、搜索、书签、高亮、笔记及可靠持久化。

不预先宣称完整 Unicode 排版、完整 EPUB/PDF 兼容或跨平台发布可用。新增格式必须有正常、损坏和资源超限测试。
