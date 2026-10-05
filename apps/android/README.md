# ReadAll Android 开发入口

Android 与 Linux 现在复用同一个 `ReaderWindow` 阅读界面：不只是共用排版，还共用顶部进度、可收起工具栏、目录、设置、选区操作和翻页动画。不是 WebView，也不是在手机上运行 Wayland 程序。平台差异收敛到文件选择、触摸、输入法、剪贴板、浏览器和生命周期。

## 当前范围

`apps/readall/src/mobile.rs` 提供有界消息队列、独立阅读线程、最新页面快照、加载阶段与取消。EPUB、MOBI、AZW3 仍使用原解析器、字体、图片、代码着色与分页；跨 JNI 只传递操作、状态和完成的像素帧。`crates/readall-android` 把句柄与异常边界单独隔离，不通过 Kotlin 保存 Rust 裸指针。取消不在 Android 主线程等待解码线程退出；计入尚未完成退出的线程，最多允许四个阅读线程。

手机应用层已全部使用 Kotlin/JVM 调用 Android SDK 原生 View/API，使用固定版本的独立 Kotlin 编译器，不要求 Android Studio、Gradle、Compose 或 AndroidX。书籍打开后隐藏独立的平台按钮栏；`ReaderView` 展示包含完整共享 UI 的 Rust 页面，不再使用系统 AlertDialog 显示目录。首页已改为 Android 原生多书书库，提供列表和封面网格两种视图；SAF 选择器、加载错误界面及系统键盘仍由 Android 管理。阅读界面继续与 Linux 共享，书库布局并非逐像素复制 Linux 文件浏览器。

最低 API 26，默认编译/目标 API 36，仅提供 ARM64 或 x86_64 单 ABI 调试包。共享 UI 已接入目录、搜索、标注列表、字号/边距/行距/主题/翻页模式、长按拖选和复制/高亮/笔记、书内链接返回、外链确认及图片查看。触摸选区拖动柄、双指缩放、跨页选择和完整可访问性语义树仍未实现。仿书模式是与 Linux 一致的 2D 卷页，不是真实三维纸张模拟。

## Kotlin 全量迁移

`src/xin/soymilk/readall/` 的 20 个应用模块以及原有 13 个测试/截图工具已全部由 Java 改为 Kotlin；另新增 `KotlinMigrationSmoke.kt`。应用与测试源目录没有手写 `.java`，构建入口发现 Java 残留会立即拒绝，避免悄悄使用旧实现。SDK 的 AAPT2 仍会在临时构建目录生成 `R.java` 资源索引，由 javac 编译；这不是 Java 应用代码，也不纳入源码。Rust 阅读引擎、Linux 端及 Python 构建脚本不改写成 Kotlin。

JNI 仍使用 `xin.soymilk.readall.NativeReader` 的 12 个原生入口、原有参数描述符与操作码。Kotlin 通过 `@JvmStatic external` 保持静态入口，协议字段保持原有 JVM 字段访问；句柄操作保留同步保护，`copyPixels` 继续在短暂获取句柄后释放锁，不把整帧复制移回输入锁中。没有新增 C++ 桥接或替换 Rust 排版。

包名、Activity 名称、debug 签名、版本、API、SharedPreferences 键、书库二进制结构、图书副本路径及 `reader-state` 保持不变。迁移测试读取迁移前 Java 写出的冻结索引，覆盖中文与代理对字符、置顶、别名和阅读进度；原值写回与 Java 索引逐字节一致，不需要清空数据或重新导入。

已通过 13 组 Kotlin/JVM 回归、23 项 Python 构建测试、447 项 Rust 测试（4 项手动测试默认跳过）、严格 Clippy 与格式检查。原 Java 和新 Kotlin JNI 客户端在相同逻辑尺寸/字体下导出的展开、收起、目录和设置四个 1080×2330 阅读帧逐像素一致。APK 已检查 DEX 中 20 个应用模块均来自 `.kt`、未打包测试或编译器，并与旧包比较签名一致。构建/迁移记录位于 `target/android/build-report.json` 和 `target/kotlin-migration/`。这些检查不代替新版 Activity、首页原生 View、SAF、输入法及真机滚动验收；本轮没有向手机安装应用。

## Android 简约界面

Android 的简约样式覆盖书库首页、图书管理面板、首次加载卡片和阅读中的菜单。阅读界面仍由共享 Rust ReaderWindow 驱动，Linux 也已复用现代菜单皮肤，桌面保留悬浮/快捷键提示，手机保留轻点/长按说明；正文排版、阅读位置和手机翻页行为不变。保留列表/封面两种书库模式，立体模式不会重新出现。

首页统一 20 dp 外边距，用标题与小字区分书库名称和数量；右上角使用太阳/月亮图标切换主题，导入按钮为主要强调色。搜索框独立成行，包含搜索图标、聚焦描边与清空按钮；列表/封面改为一组分段按钮，排序位于同一行右侧。窄窗口或较大系统字体时分段按钮优先保留文字，隐藏装饰图标，避免挤压。继续阅读使用独立卡片，书名与百分比分开，长书名截断而不覆盖进度。较矮窗口暂时收起数量说明与继续卡片，把空间留给书库。

列表不再每行堆叠大色块，改为小封面、清晰书名与细分隔线；封面模式突出原书图片，书名最多两行，格式和百分比分列，进度线细化。两种模式仍纵向惯性滚动；管理菜单图标的实际命中区域为 48×48 dp，绘制与触摸共用 `ShelfGeometry.Tile`。图书按下有轻量反馈，不新增透视、模糊或大面积阴影。空书库显示简洁线条图标与导入引导，无通知时不常驻底部说明条。

长按图书或点击管理图标打开底部圆角面板，打开、置顶、修改书名、刷新封面与移除按行排列。修改与移除仍有独立确认，清理应用副本默认不勾选，存储语义不变；窄屏/横屏下长面板可滚动。普通按钮使用有边界的水波纹与键盘焦点描边。颜色继续取自现有 Rust 亮暗配色，图标为 `ShelfIcon` 内置线条路径，不新增字体、网络资源或第三方 UI 依赖。

样式组件在 `ShelfStyle.kt`，原生管理面板在 `ShelfDialogs.kt`。`ShelfStoreSmoke` 新增间距、48 dp 命中区域、行间互不重叠、滚动后坐标一致和模式回退回归；实际 Android Views 经 SDK 编译，但宿主逻辑测试不代替真机视觉、TalkBack、字体放大与手势验收。本轮不自动安装应用。

## 结果提示自动收起

导入完成、失败和普通书库提示默认约 6 秒自动隐藏，右侧关闭按钮可立即收起；无消息时底部不保留空白。Android API 29 及以上会按系统的辅助功能建议适当延长提示时间。新消息重新计时，旧计时回调不会误关闭新消息；再次导入、进入阅读或切到后台会清理旧的短暂提示。

正在导入时，阶段文字、旋转进度和取消入口仍保持显示，不自动超时，也不被关闭按钮隐藏。任务结束才转为限时结果；过期任务的回调不会覆盖后续导入的结果。这里仅修正消息停留行为，不掩盖失败，也不把失败图书标记为导入成功。`ShelfNotice` 和 `ShelfNoticeSmoke` 覆盖超时、替换、关闭、活跃任务保留与生命周期清理。

## 阅读中的简约菜单

Android 的底部展开工具栏、收起控制、目录、搜索、设置、书签/标注、笔记、图片查看、选字操作条和外链确认使用圆角与轻量背景，减少粗分隔线和阴影。目录仍只有一个活动行，保留连续滚动、部分行命中和惯性；选中行使用浅色强调。搜索/笔记的提交按钮独立突出，设置名称和值分层显示，右侧加减按钮明确区分。设置名称只选择该项，只有点加减才修改值，避免点击文字时误减字号。

手机菜单中的初始操作说明改为轻点/长按提示，不再显示桌面 Enter、Ctrl、F 键说明；Linux 的快捷键继续保留，外观同步到现代圆角菜单。外链仍需确认，关闭面板和取消输入的逻辑不变。此次没有移动现有工具栏、目录和输入框命中区域，Kotlin 输入法适配与触摸坐标不变；正文 Surface、分页、书内定位和翻页模式仍与 Linux 共享。界面整体像素不要求两端一致，桌面的悬浮状态、收起箭头与快捷键说明和手机有所区别。

`menu_style.rs` 与 `tools/mobile_menu.rs` 负责共享皮肤，圆角基础绘制抽取到 `ui/shapes.rs` 供 Linux 书库复用。Linux 专属交互位于 `desktop.rs`，不接管 Android 触摸路由；圆角在设备像素分辨率下抗锯齿绘制，复用只读页面及目录缓存，不降低整页清晰度、不增加 UI 依赖。`ReaderMenuSmoke` 经真实 JVM/JNI 检查菜单像素、主题、字号加减、搜索与关闭位置；Rust 回归检查两端正文/布局/操作一致。另以真实中文字体导出 1080×2330 菜单帧；这些测试不代替真机触摸、辅助功能和视觉验收。

## 多书管理与两种书库视图

首页右上角「导入」通过系统文件选择器添加 EPUB / MOBI / AZW3，可多选，每批最多 64 本；导入结果留在书库，不会自动打开其中一本。已导入的同内容文件按 SHA-256 合并，保留自定义书名、置顶和阅读进度。导入显示当前文件和阶段，可取消，已成功加入的图书保留。

| 首页视图 | 操作与显示 |
| --- | --- |
| 列表 | 纵向惯性滚动，显示小封面、书名、作者或文件名、格式与阅读百分比 |
| 封面 | 自适应列数的封面网格，上下滚动；卡片带书名、格式、进度及管理按钮 |

点列表/网格图书直接阅读；长按图书或点击 ⋯ 进入管理。管理菜单支持打开、置顶/取消置顶、修改显示书名、重新读取封面和移出书库。移除默认只清理书库记录；确认框可另勾选清理应用内副本和封面，始终不删除系统原书或阅读进度、书签、高亮、笔记。显示书名留空可恢复原始书名。

立体模式已移除，包括入口、横向吸附、透视绘制及相关命中逻辑。旧版保存的模式值 2（或其他未知值）自动映射并保存为封面模式；只更新视图偏好，不重建书库、清空浏览焦点或修改排序、原书和阅读状态。列表/封面模式的既有编号保持不变，正文的三种翻页模式不受影响。

搜索支持书名、作者、文件名和格式；排序按钮循环切换最近阅读、书名和导入时间，置顶优先。视图、排序及浏览焦点保存在当前设备，下次启动沿用。网格列数变化、横竖屏切换与视图切换保持原浏览图书可见。首页「继续阅读」及每本书的进度显示最近一次打开/退出/后台同步的值，不每帧写书库索引。

`ShelfHome` / `ShelfCanvas` 负责主页和列表/封面两种视图，`ShelfGeometry` 负责可见范围与布局，`ShelfStore` / `ShelfController` 负责原子持久化、导入和管理。列表与网格只画可见条目；缩略图异步读取，20 MiB LRU 缓存，待解码任务最多 12 个。滚动时不重解压 EPUB、不启动正文排版。实际手机帧率仍需验收。

封面由 Rust 读取 EPUB cover-image、旧版 cover 元数据/guide 及明确命名的封面；支持专用 XHTML/SVG 封面页中的包内图片引用。MOBI/AZW3 直接读取封面记录，不重建正文。封面资源限 8 MiB、解码限 8M 像素，缩为不超过 384×512 的预览；不存在、损坏、超限或不支持的封面使用带书名的文字封面，不阻止图书显示。不会访问网络封面、下载字体或修改原书。复杂 SVG 合成封面不承诺完整还原。

`ShelfStoreSmoke` 验证真实磁盘读写、重复导入、重命名、置顶、搜索排序、安全删除、并发写入、损坏索引拒绝覆盖，以及两种视图切换、列数变化与旧模式回退；`ShelfPreviewSmoke` 使用实际 JVM/JNI 验证元数据与封面像素。SDK/APK 构建和宿主测试不等于手机上的书库视觉、SAF 多选及触摸验收。本轮不自动安装 APK；批量选中管理、云同步和完整书库可访问性节点树不在本轮范围内。

## 与 Linux 一致的操作

底部三层浮动工具栏、展开/收起箭头及目录面板来自同一套 Rust 绘制与命中测试。点“目录”展开，再点可收起；目录内拖动直接改变可见列表，不再模拟键盘选中行：手指上划，目录内容向上移动并显示后面的章节；下划显示前面的章节。目录现在按连续位移滚动，不再累计到一行才移动：可以停在半行，松手后按速度惯性减速，再次按住立即停止。保留一个可见活动行，只有点按才跳章。标题固定，顶部/底部的局部行和点按坐标共用裁剪变换，右侧细条显示列表位置。目录到顶/到底不积压位移，反向拖动无需抵消越界距离；标题和工具栏不触发目录拖动。较矮窗口打开目录时暂时收起大工具栏，保留底部返回工具栏的箭头，防止横屏或键盘挤压导致重叠。顶部显示相同的章节、页码、总百分比，底部保留进度条。

点工具栏的“设置”，在第五行“翻页模式”右侧加减按钮切换：

| 模式 | 手机操作 | 与 Linux 共用的效果 |
| --- | --- | --- |
| 左右滑动 | 单指左右拖动、松手 | 页面平移；短拖动回弹 |
| 仿书翻页 | 单指左右拖动、松手 | 纸背、折边与阴影的 2D 卷页 |
| 上下平滑滚动 | 上下拖动，松手可继续惯性移动 | 前后页拼接、部分页位置与跨章缓存 |

点按直到手指抬起才激活按钮、链接或图片；正文点击不会翻页。长按后拖动选择文字，松手使用共享“复制 / 高亮 / 笔记 / 取消”操作条。复制交给 Android 系统剪贴板；搜索和笔记通过 `BaseInputConnection` 接收包含中文组合输入的系统键盘文本，提交和删除仍作用于共享输入框。HTTP/HTTPS 链接仍先在共享面板确认，确认后才调用 Android 浏览器 Intent。

系统返回先停止当前移动或关闭面板/选区，再返回书内链接来源，最后关闭图书回到首页；文件选择器和键盘遵循 Android 系统自身的返回行为。外接键盘保留主要 F2–F8、翻页键和 Ctrl+C/V 操作。

## 亮色与暗色

首页右上角点击月亮/太阳图标切换暗色/亮色主题；阅读中点底部“设置”，第一行“主题”用加减按钮切换。默认亮色，选择会保存，返回首页、换书和重启后保持。主题不自动跟随系统变化。Linux 使用同一配色和设置模型，支持 F6 和书库左下角按钮；两台设备的设置不会通过网络自动同步。

正文、顶部进度、目录、工具栏、设置/搜索/标注面板、选区操作条和外链确认都来自共享主题。Android 首页、加载/错误卡片、进度条及页面空白区也使用 Rust 返回的颜色；状态栏/导航栏使用对应背景，并单独设置浅色或深色图标。关闭系统 Force Dark，避免对已经绘制好的暗色页面再次反相。系统文件选择器和键盘外观不由 ReadAll 控制；图片和作者显式背景不做反相。

JNI 状态协议携带主题名和固定顺序的 ARGB 配色，`AndroidTheme.kt` 不复制一份 RGB 常量表。纯配色查询不访问磁盘；首页保存/加载走 IO 线程，与阅读器使用同一个 `files/reader-state/library-v1/settings.conf`，只更新主题并保留其他字段。首次在首页选择主题也保持手机默认字号 20、边距 16；旧 `paper`/`sepia` 可读为亮色，显式保存时写成 `light`/`dark`。Android SharedPreferences 只镜像最近的主题名用于启动界面，不充当第二份阅读设置。

`ThemeSmoke` 经真实 JVM/JNI 验证首页保存→阅读器打开→切换主题→返回/重开、1080 像素输出、旧名称兼容与阅读位置保持；Rust 覆盖两种主题下的工具栏/目录/设置像素及文本对比度。本轮主题的 Android 系统栏与视觉效果尚需真机验收，未自动安装应用。

## 连续滚动与响应性

目录缓存当前可见行及多出的一行，滚动小于一行时移动缓存的设备像素，不重新绘制每一行文字；换到新行、主题或尺寸变化时再更新缓存。列表偏移保留小数，没有按行吸附或松手回跳。标题和工具栏不跟着列表移动；反向拖动、到顶/到底、关闭面板和后台切换会停止或限制对应惯性，不误翻正文。

正文上下滚动在按住时直接跟随目标位移，不再叠加一层追赶手指的缓动；松手后使用按时间衰减的速度。Linux 滚轮/翻页键原有的缓动保留，三种翻页模式和阅读位置存储不变。目录与正文的惯性独立，手指重新按下或返回操作可停止它们。

移动阅读线程按约 16 ms 的帧节奏合并脏更新，在一帧内处理多个 MOVE 但只合成/发布一次。计时从合成开始算，绘制耗时计入预算，不在绘制结束后再额外等待 16 ms。尺寸、目录及设置等控制命令仍及时提交；手势开始/结束/取消不丢弃，同方向移动合并但保留反向转折。

`Surface` 的不可变快照共享像素存储，只有后续绘制才分离，避免发布/缓存时反复复制高清整页，也不会改写显示中的旧帧。Kotlin 像素复制只短暂获取句柄，不在多兆字节复制期间占用状态/输入方法的同步锁；JNI 捕获只读帧后再复制，旧帧、关闭、只读缓冲区及容量检查继续保留。没有降低原生像素输出分辨率，也没有取消文本抗锯齿。

新增 Rust 回归与 `SmoothScrollSmoke` 覆盖小于一行的移动、局部行裁剪/命中、惯性停止、正文跟手、批量绘制、只读快照隔离、高清 JNI 像素传递和复制/输入并发。`ReadAll.present` Trace 标记可用于之后的设备分析。这里的帧节奏是调度目标，不是实测真机 FPS；首次进入未缓存的大章节/图片仍可能短暂准备，正文仍沿用分页拼接并保留页间留白。手机视觉与帧率需覆盖安装后验收。

## 阅读中的加载提示

已有页面显示后，翻页、滑动、跨章、字号或窗口重排等普通加载不再弹出中间的“绘制页面与文字”进度卡片。保留当前画面，只在安全区域的右下角显示 22 dp 的小型旋转圆圈，无文本、百分比、遮罩或取消按钮，不改变阅读区域尺寸。圆圈不接收触摸或通用指针事件，滑动、选字及工具栏点击仍交给下面的阅读界面。它随亮色/暗色主题使用对应强调色，由 Android 单独驱动动画，不要求 Rust 为圆圈重绘整页。

短于约 200 ms 的工作不显示圆圈，避免正常滚动频繁闪烁；圆圈出现后至少保留约 160 ms，工作结束后自动隐藏。切换图书、返回首页、取消、错误及切到后台会立即清理圆圈状态。首个原生帧已经生成但尚未交到 ReaderView 时仍视为首次加载，保留阶段进度和取消/重试入口；加载失败也继续显示明确错误，不会变成无限转圈。

这是加载反馈的显示调整，不代表取消了实际的章节解析/文字绘制等待，也不影响全书进度条。`LoadingFeedbackSmoke` 以确定时钟验证首次画面、短工作、长工作、显示/隐藏延时、生命周期重置，以及已有页面时绝不恢复中间加载面板的规则；APK 使用真实 SDK 编译，手机上的视觉与触摸效果仍需覆盖安装后验收。

## 帧调度与资源

Rust 阅读线程接收有界有序输入，翻页/目录拖动的同方向连续移动合并到最新位置，反向拖动保留转折点，避免到边界后反向位移被吞掉；选区移动仍按相邻同类事件合并。开始/结束/取消保留顺序和预留容量。标题滚动不阻止空闲预读；预读遇到新操作可以在检查点中断。只发布最新完成帧，界面合成复用 Linux 的相邻页缓存，不逐帧解码图书图片。

Android 使用 Choreographer 申请显示回调，读取和像素传递在独立工作线程；最多一个像素复制任务，复用直接缓冲区与已经退出显示的 Bitmap。ReaderView 使用软件画布避免将仍被 RenderThread 使用的 Bitmap 交回缓冲池。阅读逻辑尺寸和像素缓冲尺寸独立传递：逻辑字号/工具栏仍与 Linux 一致，但字形轮廓、UI 图标和页面合成在设备像素密度下绘制，正常手机不再先画低分辨率整页再放大。触摸和惯性位移按显示帧的逻辑/像素比例回算；密度或窗口变化时重新准备对应字形缓存和帧。窗口旋转、IME 显示和后台切换会停止当前触摸、重排或暂停动画并保存内容锚点。实际设备帧率仍需真机测量，缓存未命中或大章节排版仍可能暂时等待。

已使用 API 36、Build Tools 36.0.0、NDK 29.0.14206865 和系统 Rust 1.97.1 完成 ARM64 原生库交叉编译、Kotlin/D8 编译、APK 签名与对齐校验。产物为 `target/android/readall-android-debug-arm64-v8a.apk`，调试预览版。用户已确认前一基础 APK 在手机上可打开 EPUB；本轮共享界面改造已做宿主回归与 APK 构建验证，尚未进行本轮真机视觉、触摸和帧率验收。

## 清晰度与连续滚动页缝

旧 Android 版把约 393 像素宽的逻辑页面直接放大到 1080 像素宽，导致正文、标题和工具栏一起变糊。现在 `Surface` 分开记录逻辑尺寸与实际像素尺寸，字宽和分页仍在逻辑坐标计算，字形则以原始字体轮廓重新光栅化到设备像素；不是对小位图插值或做锐化。JNI 协议同时携带这两组尺寸，Bitmap/直接缓冲按实际像素分配。在通常的 1080p 手机可用区域内保持 1:1 显示；极大窗口超过 4M 像素时才按预算缩小输出，不能把该兜底称作原生分辨率。原书中本来低分辨率的图片不会因此凭空增加细节。

另一个问题是滚动拼接固定忽略源页顶部 32 个逻辑像素，而旧正文可能从 16 像素边距开始，使下一页首行被切掉上半部。原生阅读会话现将顶部 32、底部 4 像素的界面区域在排版阶段预留出来；正文、图片、盒背景和命中范围共同使用该内容区域。小边距可能引起重新分页，但仍以原文/图片锚点恢复进度，不改原书或清空阅读记录。页条拼接仍保留分页留白；复制使用设备像素边界与同一整数像素步长，避免小数密度在页缝累积舍入缺口。

回归覆盖 0/8/16/31/40 边距、普通/高密度首行和图片、逐扫描行拼接、密度变化后的缓存更新与位置恢复，并通过真实 JVM/JNI 导出 1080×2330 的中文页面、工具栏、目录和设置。密度修正尚未完成本轮手机视觉与帧率验收。

## 文件与状态

系统选择器采用 `ACTION_OPEN_DOCUMENT`。只读取获授权的 `content://`，不猜测它对应的系统路径，也不申请 INTERNET、外部存储或“所有文件访问”权限。读取过程在 Kotlin 管理的 IO 线程执行，限 128 MiB，按内容 SHA-256 命名私有缓存，不使用可被操控的显示文件名拼路径。云文档由用户所选提供程序处理，本应用不主动下载网络资源。

书库索引位于 `files/bookshelf-v1/shelf-v1.bin`，图书副本在 `files/bookshelf-v1/books`，缩略图在 `files/bookshelf-v1/covers`；进度、书签和设置仍在 `files/reader-state`。升级时仅将旧版已记录的最后一本 `cache/books` 图书安全复制并登记，原缓存和原书保持不变，继续沿用按内容定位的阅读记录。新导入不再采用“只保留最近四本”的缓存淘汰逻辑。

书库最多 1000 条记录，索引读写均限 16 MiB，单书 128 MiB，受管理的图书副本总量限 2 GiB；达到上限只提示，不自动删除已有书籍。副本丢失时提示重新导入，阅读状态保留。书库采用临时文件、同步和原子替换，损坏/未知索引拒绝覆盖；移除后到达的阅读进度更新不会重新创建该条目。书库百分比只作首页摘要，正文仍使用已有的精确定位与标注存储。

## 工具路径与构建

所有脚本调用都使用绝对工具路径，不写 `.zshrc`，不设置 `ANDROID_HOME`、`JAVA_HOME`、`PATH` 或 NDK 环境变量。Gentoo Java 包装器可能依赖未挂载配置，因此直接使用 `/usr/lib/jvm/openjdk-17/bin/java` 与 `javac`。SDK 与系统 Rust 安装也不会被脚本修改。只有显式 `prepare-rust` / `prepare-kotlin` 准备命令会下载项目内工具链，普通 build/doctor 不自动下载安装工具链。Kotlin 编译通过指定 JDK 的 `bin/java` 启动，不依赖 `kotlinc` shell 包装器。

先在项目根目录运行诊断：

```bash
/usr/bin/python3 apps/android/tools/build.py doctor --sdk /opt/android-sdk
```

必需组件是 SDK `platforms/android-36/android.jar`、Build Tools（`aapt2`、D8、`zipalign`、`apksigner`）、JDK 17、Kotlin 2.1.21、Android NDK，以及与所用 Rust 编译器匹配的 `aarch64-linux-android` 标准库。只安装 SDK 命令行管理器不代表这些组件全部存在。NDK 可以位于 SDK 的 `ndk/<版本>`，或用 `--ndk` 指定；脚本不会自动接受许可证或下载组件。

Kotlin 工具链首次在本机准备：

```bash
/usr/bin/python3 /home/vv/project/ReadAll/apps/android/tools/build.py prepare-kotlin
```

编译器与配套依赖固定为 JetBrains Maven Central 发布的指定版本，并在 `kotlin_toolchain.py` 中锁定 SHA-256 和字节数。下载仅写入 `target/android/downloads/kotlin-2.1.21/`，校验后原子安装到 `target/android/toolchains/kotlin-2.1.21/kotlinc/`；已准备的目录可复用，不写 SDK/系统目录。工具链已在本轮准备完成。使用外部安装时可传 `--kotlin /absolute/kotlinc`。2.1.21 是兼容性固定版本，不宣称是最新版；构建检查 D8 >= 8.6.17，已验证当前 Build Tools 36.0.0 的 D8 8.10.9。Kotlin 常规编译不联网，预先缓存齐全时也可 `prepare-kotlin --offline`。

Gentoo 系统 Rust 没有 Android 标准库时，不必安装 rustup 或替换系统 Rust。先执行一次：

```bash
/usr/bin/python3 apps/android/tools/build.py prepare-rust --abi arm64-v8a
```

该命令读取指定编译器的完整版本身份，从 Rust 官方 HTTPS 分发清单选择**版本、提交和日期完全匹配**的 Android 标准库，校验清单和压缩包 SHA-256，只提取正常的目标库文件到 `target/android/toolchains/rust-<版本>-<目标>/`。不执行下载包中的安装脚本，不接受路径穿越、链接或超预算文件，不向 `/opt/rust-*` 写入。网络下载只在这个显式命令中发生；已有匹配缓存可复用。

```bash
/usr/bin/python3 apps/android/tools/build.py build --sdk /opt/android-sdk
```

构建自动发现已准备的匹配目录，仅为 Android 目标传入 `--sysroot`；宿主编译脚本/过程宏仍使用系统标准库。也可用 `--rust-sysroot /absolute/sysroot` 指定外部目录。编译器升级后需要重新准备匹配目标，不混用不同版本标准库。

工具不在默认位置时，使用独立参数，不需要环境变量。下列 `/absolute/...` 是需替换为实际安装位置的占位路径：

```bash
/usr/bin/python3 apps/android/tools/build.py build --sdk /opt/android-sdk --ndk /absolute/android-ndk --java /usr/lib/jvm/openjdk-17 --cargo /absolute/rust/bin/cargo --rustc /absolute/rust/bin/rustc --font /absolute/LXGWWenKaiLite-Regular.ttf
```

SDK 平台可用 `--api`、Build Tools 可用 `--build-tools` 指定。`--abi x86_64` 用于 x86_64 模拟器。Rust Android 标准库缺失时会明确报错，不替换系统 `/usr/bin/rustc`。字体优先使用 `--font`，否则复用桌面构建的捆绑字体缓存；不会单独下载或向用户分发字体文件。

构建流程：Rust `cdylib` → AAPT2 → Kotlin/JVM（应用源码，目标字节码 1.8）+ javac（仅 SDK 生成的 R.java）→ D8 → APK ZIP → zipalign → debug 签名 → 签名与对齐检查。Android 编译使用 SDK `android.jar` 作为平台 API，不以桌面 JDK 新 API 替代手机 API；宿主逻辑测试限制为 Java 8 API。APK 只加入 Kotlin 标准库与 JetBrains annotations 运行依赖，编译器、完整反射实现及编译器的 coroutine 依赖不打包；对应 Apache-2.0 许可与署名放入 assets。所有输出都在根目录 `target/android/`。ARM64 APK 目标路径：

```text
target/android/readall-android-debug-arm64-v8a.apk
```

原生库每个 ELF `PT_LOAD` 段检查至少 16 KiB 对齐，同时检查 APK 内 `.so` 的 ZIP 对齐。这是构建检查，不代替在 16 KiB 页设备上运行。调试密钥只生成在 `target/android/debug.keystore`，权限 0600；公开的标准 debug 密码只用于本地测试，不能当作正式发布签名。`build-report.json` 记录产物 hash、ABI 和工具路径。

只有显式 `install` 会写入设备；构建命令不安装、不启动手机应用。安装前检查现有 ADB server、授权状态、唯一设备和 ABI：

```bash
/usr/bin/python3 apps/android/tools/build.py install --sdk /opt/android-sdk
```

多个设备时加 `--serial`。安装使用 `adb install -r`，不会卸载或清空数据，不修改手机的其他设置。

## EndlessVibe 隔离环境

宿主已安装 SDK，但插件显示 `/opt/android-sdk (not visible)` 时，不应重复安装或修改 shell 环境。需要由服务管理员把 SDK 目录映射进现有 bubblewrap 配置：在原 `[execution]` 的 `readonly_mounts` 列表内追加以下一项，保留所有既有项，然后重启服务。

```toml
{ source = "/opt/android-sdk", target = "/opt/android-sdk" }
```

不是关闭沙箱，也不要挂载整个 HOME、凭据目录或 USB 设备。SDK 若包含符号链接到其他工具目录，还需要只暴露它实际引用的最小工具链目录。读取 SDK 与执行构建只要求只读挂载；SDK 安装/更新仍在宿主执行。

设备访问通过宿主本机 ADB server，不暴露公网监听。在 Gentoo 桌面终端启动并确认手机授权：

```bash
/usr/bin/adb start-server
/usr/bin/adb devices -l
```

诊断只查询已存在的 `127.0.0.1:5037`，不会在看不到 USB 的沙箱内自动启动另一台 ADB server。

## 宿主机验证

```bash
/usr/bin/python3 -B apps/android/tools/test_build.py
/usr/bin/python3 -B apps/android/tools/test_rust_target.py
/usr/bin/python3 -B apps/android/tools/test_kotlin_toolchain.py
/usr/bin/python3 apps/android/tools/build.py host-test
```

`host-test` 使用真实 JNI 动态库和 JDK `-Xcheck:jni`，不是模拟 JNI。测试原创 EPUB/字体样本、RGBA 像素传递、无效参数、过期帧拒绝、工具栏展开/收起、共享目录、三种模式、UTF-8 输入、系统效果队列、重排、主题、书签及关闭后恢复；并执行 Kotlin TouchRouter 手势和 Java 索引兼容性测试。Rust 测试额外检查 Linux 和移动端 presenter 的正文、几何与操作一致，验证矮屏目录布局、长按选区、取消、列表滚动不翻页和模式持久化。样本和状态全部位于 `target/android-host`，不会读写用户真实最近阅读或手机文件；每次运行有独立目录。该测试需要 JDK 与已准备的 Kotlin 编译器，不依赖 Android SDK，也不能验证 Activity、SAF、Android Bitmap 或 APK 包装。每次清理专用测试 class 输出，避免旧 Java 字节码掩盖迁移缺失。

缓存只读的受限构建环境可通过 `--vendor /absolute/vendor --offline` 使用已校验的依赖目录；这只是 Cargo 的命令参数，不修改全局配置或环境变量。普通宿主机无需该选项。

## 后续真机验收

ARM64 APK 已通过真实 SDK/NDK 构建，签名 v2/v3、ELF 三个 LOAD 段 16 KiB 对齐、APK ZIP 对齐、JNI 导出和基础包结构已检查。下一步用授权设备测试安装、中文首屏、GIF/SVG、跨章、目录、旋转、后台恢复、读取取消和错误界面。重点验收本轮共享工具栏/目录、三种翻页模式、长按选区、中文输入法、剪贴板和链接交互；目录拖动新增 Rust 方向/边界/合并事件回归及真实 TouchRouter→JNI→目录点选验证，本轮仍需手机手势实测；不把宿主机测试结果当成手机帧率或视觉验收。

## 官方参考

- Kotlin 命令行编译器：https://kotlinlang.org/docs/compiler-reference.html
- Kotlin/JVM 与 Java 互操作：https://kotlinlang.org/docs/java-to-kotlin-interop.html
- Kotlin 与 Android D8 版本兼容：https://developer.android.com/build/kotlin-support
- Kotlin 2.1.21：https://github.com/JetBrains/kotlin/releases/tag/v2.1.21

- Android SAF：https://developer.android.com/training/data-storage/shared/documents-files
- JNI：https://developer.android.com/training/articles/perf-jni
- Choreographer：https://developer.android.com/reference/android/view/Choreographer
- InputConnection：https://developer.android.com/reference/android/view/inputmethod/BaseInputConnection
- AAPT2：https://developer.android.com/tools/aapt2
- D8：https://developer.android.com/tools/d8
- APK 签名：https://developer.android.com/tools/apksigner
- ZIP 对齐：https://developer.android.com/tools/zipalign
- 16 KiB 页支持：https://developer.android.com/guide/practices/page-sizes
- NDK 独立工具链：https://developer.android.com/ndk/guides/other_build_systems
