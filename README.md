# ReadAll

Rust 自研电子书阅读器，目标平台为 Linux、Windows、Android。文档解析、排版、绘制和阅读交互由项目自身实现，不使用 WebView 或现成 PDF/EPUB 引擎。

## 当前状态

项目处于 P0 基础阶段，**当前只有诊断命令行，没有原生 GUI，也不能阅读 PDF/EPUB**。不得把文件签名检测当作对应格式已经支持。

已实现：有大小上限的文档输入、平台无关的随机读取接口、UTF-8/UTF-8 BOM/UTF-16 BOM 文本解码、换行规范化、原始文件 SHA-256 内容标识、可序列化且校验文档/字符边界的文本定位。

当前外部 crate 依赖为零；仅使用 Rust 标准库及工作区内部 crate。操作系统和标准库自身不属于“零依赖”承诺。源码禁止 unsafe；以后平台 FFI 的必要例外应限定在独立适配层，并单独审查。

## 构建与运行

```sh
cargo test --workspace --offline
cargo run -p readall --offline -- inspect tests/fixtures/sample.txt
cargo clippy --workspace --all-targets --offline -- -D warnings
cargo fmt --all -- --check
```

Cargo 声明 Rust 1.85 / edition 2024 作为最低目标；这不是已经完成所有工具链与系统验证的声明。Linux、Windows 和 Android 实机验证分别推进，不把宿主机编译等同于三端验收。

## 模块边界

| 模块 | 职责 |
| --- | --- |
| `readall-core` | 文档输入约束、格式模型、文本解析、内容位置；不依赖文件路径或窗口对象 |
| `readall-platform` | 本地文件访问；后续承接窗口、系统输入、Android URI 和像素呈现 |
| `readall` | 当前的诊断 CLI，后续的原生应用入口 |

文本读取默认限制原文件 32 MiB、解码后 64 MiB，调用方可配置。SHA-256 用于内容身份，不提供数字签名验证。读取能发现长度变化，但不承诺对正在修改的文件取得原子快照。

文本位置格式为 `txt-v1:<原始文件SHA-256>:<规范化UTF-8字节偏移>`。v1 去除编码 BOM，将 CRLF/CR 规范化为 LF；偏移不是 UTF-16 文件的原始字节位置，也不是字符序号或页码。原文件内容改变后，旧位置拒绝恢复。阅读进度自动持久化尚未实现。

UTF-16 必须带 BOM；GBK 等旧编码和 UTF-32 尚未支持。无效编码、NUL/终端控制字符会报错，而不是有损替换。`ZIP` 签名只说明可能为 EPUB，尚不验证 ZIP/EPUB 结构。

## 后续顺序

1. 完成 P0：绘制指令与 CPU 像素缓冲区、原生窗口/输入适配、三端最小启动验证。
2. TXT 阅读：真实字体度量与字形绘制、断行分页、稳定位置恢复。
3. EPUB：受限 ZIP、包结构、目录、XHTML/CSS 阅读子集、自研排版。
4. PDF：对象与交叉引用、页面/资源、绘制指令、字体与图像；按功能建立兼容性矩阵。
5. 原生书架、搜索、书签、高亮、笔记及可靠持久化。

不预先宣称完整 Unicode 排版、完整 EPUB/PDF 兼容或跨平台发布可用。新增格式必须有正常、损坏和资源超限测试。
