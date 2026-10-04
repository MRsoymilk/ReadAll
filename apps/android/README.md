# ReadAll Android 开发入口

这是 Android 第一阶段的原生宿主工程，复用现有 Rust 阅读代码。不是 WebView，也不是在手机上运行 Wayland 程序。桌面入口改为调用同一个 `readall` 库；暂不物理搬迁所有模块，避免为接入手机而重复一份排版实现。

## 当前范围

`apps/readall/src/mobile.rs` 提供有界消息队列、独立阅读线程、最新页面快照、加载阶段与取消。EPUB、MOBI、AZW3 仍使用原解析器、字体、图片、代码着色与分页；跨 JNI 只传递操作、状态和完成的像素帧。`crates/readall-android` 把句柄与异常边界单独隔离，不通过 Java 保存 Rust 裸指针。取消不在 Android 主线程等待解码线程退出；计入尚未完成退出的线程，最多允许四个阅读线程。

手机界面使用 SDK 自带的 Java/Android API，以便只用已有 JDK、SDK 和 NDK 构建，不要求 Android Studio、Gradle、Kotlin 编译器或额外 AndroidX 依赖。已写入系统文件选择、加载状态、基础横向滑动/按钮翻页、目录跳转、字号、主题、添加书签和继续上次阅读入口。

**第一阶段不代表桌面功能已全部移植。** Android 长按选字、选区手柄、链接/图片点击、搜索/标注列表、仿书动画、连续惯性滚动及系统剪贴板尚未接入。横向手势当前只触发整页切换，没有动画；点击正文不会翻页。最低 API 26，默认编译/目标 API 36，仅提供 ARM64 或 x86_64 单 ABI 调试包。

已使用 API 36、Build Tools 36.0.0、NDK 29.0.14206865 和系统 Rust 1.97.1 完成 ARM64 原生库交叉编译、Java/D8 编译、APK 签名与对齐校验。产物为 `target/android/readall-android-debug-arm64-v8a.apk`，调试预览版；尚未完成手机安装、启动与交互验收。宿主机 JVM/JNI 测试与 APK 构建成功都不能替代真机验收。

## 文件与状态

系统选择器采用 `ACTION_OPEN_DOCUMENT`。只读取获授权的 `content://`，不猜测它对应的系统路径，也不申请 INTERNET、外部存储或“所有文件访问”权限。读取过程在 Java IO 线程执行，限 128 MiB，按内容 SHA-256 命名私有缓存，不使用可被操控的显示文件名拼路径。云文档由用户所选提供程序处理，本应用不主动下载网络资源。

缓存位于应用 `cache/books`；进度、书签和设置位于 `files/reader-state`。原书不被修改或删除。缓存缺失时，“继续”尝试重新使用已保存 URI；提供程序撤销授权后需要重新选择。当前仅保存一个“继续阅读”入口，尚不是桌面完整最近阅读列表。图书首帧准备好后才记为最近打开；旧进度损坏时保留原记录，不自动覆盖。

## 工具路径与构建

所有脚本调用都使用绝对工具路径，不写 `.zshrc`，不设置 `ANDROID_HOME`、`JAVA_HOME`、`PATH` 或 NDK 环境变量。Gentoo Java 包装器可能依赖未挂载配置，因此直接使用 `/usr/lib/jvm/openjdk-17/bin/java` 与 `javac`。SDK 与系统 Rust 安装也不会被脚本修改。只有显式 `prepare-rust` 会下载匹配的 Android 标准库到项目 `target`，普通 build/doctor 不自动下载安装工具链。

先在项目根目录运行诊断：

```bash
/usr/bin/python3 apps/android/tools/build.py doctor --sdk /opt/android-sdk
```

必需组件是 SDK `platforms/android-36/android.jar`、Build Tools（`aapt2`、D8、`zipalign`、`apksigner`）、JDK 17、Android NDK，以及与所用 Rust 编译器匹配的 `aarch64-linux-android` 标准库。只安装 SDK 命令行管理器不代表这些组件全部存在。NDK 可以位于 SDK 的 `ndk/<版本>`，或用 `--ndk` 指定；脚本不会自动接受许可证或下载组件。

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

构建流程：Rust `cdylib` → AAPT2 → javac → D8 → APK ZIP → zipalign → debug 签名 → 签名与对齐检查。Java 8 编译时将 SDK `core-lambda-stubs.jar` 放在 `android.jar` 前面，避免 `LambdaMetafactory.metafactory` 缺失；D8 负责后续语言特性转换。所有输出都在根目录 `target/android/`。ARM64 APK 目标路径：

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
/usr/bin/python3 apps/android/tools/build.py host-test
```

`host-test` 使用真实 JNI 动态库和 JDK `-Xcheck:jni`，不是模拟 JNI。测试原创 EPUB/字体样本、RGBA 像素传递、无效参数、过期帧拒绝、翻页、目录、重排、主题、书签及关闭后恢复。样本和状态全部位于 `target/android-host`，不会读写用户真实最近阅读或手机文件；每次运行有独立目录。该测试不依赖 Android SDK，也不能验证 Activity、SAF、Android Bitmap 或 APK 包装。

缓存只读的受限构建环境可通过 `--vendor /absolute/vendor --offline` 使用已校验的依赖目录；这只是 Cargo 的命令参数，不修改全局配置或环境变量。普通宿主机无需该选项。

## 后续真机验收

ARM64 APK 已通过真实 SDK/NDK 构建，签名 v2/v3、ELF 三个 LOAD 段 16 KiB 对齐、APK ZIP 对齐、JNI 导出和基础包结构已检查。下一步用授权设备测试安装、中文首屏、GIF/SVG、跨章、目录、旋转、后台恢复、读取取消和错误界面。随后独立推进触摸选字/复制、链接/图片查看及三种翻页模式，不把宿主机测试结果当成手机帧率或视觉验收。

## 官方参考

- Android SAF：https://developer.android.com/training/data-storage/shared/documents-files
- JNI：https://developer.android.com/training/articles/perf-jni
- AAPT2：https://developer.android.com/tools/aapt2
- D8：https://developer.android.com/tools/d8
- APK 签名：https://developer.android.com/tools/apksigner
- ZIP 对齐：https://developer.android.com/tools/zipalign
- 16 KiB 页支持：https://developer.android.com/guide/practices/page-sizes
- NDK 独立工具链：https://developer.android.com/ndk/guides/other_build_systems
