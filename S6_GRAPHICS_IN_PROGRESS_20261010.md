# S6 图形开发进度

2026-10-10。**S6 五库已导出，双端图形绘制、快照加密、持久会话 profile 与历史恢复主要链路已接通并通过定向验证。生产启动默认仍为 `2.21_v2`，尚未默认启用 Graphics；真实 Relay、ARM64、混版本和长稳门禁未完成，不能声明 S6 产品完成或可发布。**

以下为同日分阶段记录；早期“未导出/未接线”是当时状态。当前契约和验证边界以文末“产品接线与内存保护复核”为准。

首轮新增组件：

- `graphics_resources.rs`：不可变 RGBA/A8、单图/会话/进程预算、Arc 共享冻结像素、单调 ID、删除/clear 不复用 ID、TLGRS1 严格编码/解码。
- `sixel.rs`：有界双遍扫描，先测量再分配；标准 RGB/HLS、调色板、repeat、raster、透明/背景、CR/nextline、aspect ratio。错误带 payload offset 和原始 byte；未知参数/超限明确拒绝。
- 原始尺寸不预先放大；aspect ratio由placement/render消费。首轮尚无滚动/擦除/页/历史生命周期，后续接线见文末。
- 旧profile仍保持原来的SIXEL/DRCS fail closed、APC忽略行为。首轮仅显式Rust图形开发入口处理Kitty与DRCS下载；后续显式C ABI/JNI入口不改变已有产品会话profile或对外capability。

后续本地实现（同日）：

- 修复 `graphics_zlib.rs` 动态 Huffman 黄金样本中的手抄错误，Python 重新生成确认三个原始样本；不是通过放宽校验消除失败。
- `graphics_zlib.rs`：受调用方输出预算约束的 RFC1950/1951 stored/fixed/dynamic、重叠回溯、窗口/树/Adler32/尾随数据检查。
- `graphics_png.rs`：PNG 灰度/调色板/RGB/灰度 alpha/RGBA，全部标准位深、五种滤波、Adam7、PLTE/tRNS、连续多 IDAT、CRC32；静态 profile 拒绝 APNG、未知 critical chunk。元数据不解压、不执行。
- 图形 scratch 和像素使用同一进程预算，先登记再分配，异常释放。输入 PNG 32MiB，RGBA 16MiB，行缓冲有独立确定上限。
- `scripts/Generate-GraphicsFixtures.py` 使用 Python 标准库生成 35 个自有 PNG/原始 RGBA oracle，提交文本 fixture，测试不依赖联网或外部图片。
- 复用 `base64.rs` 严格实现给 OSC52 和 Kitty，保留原 OSC52 文本与授权规则。新增 `UnsupportedDecodedPayload` 区分解码后字节坐标与原始控制流坐标；没有伪造解压前字节位置。
- `kitty_graphics.rs`：typed 控制头、32-bit 参数、RGB/RGBA/PNG/deflate、4096-byte 分块与16K chunk/32MiB上限、m/q继承、删除中止传输、quiet回复；无I/O的受限 broker request。静态 transmit/display/query/delete，动画/Unicode placeholder/relative placement 扩展明确拒绝。真实 broker 未实现，不能把生成 request 算作 file/temp/shared-memory 已支持。
- `drcs_bank.rs`：两个soft set、80/132 rendition、自动空槽、最新designation优先、部分替换/擦除、未定义与空白分离、不可变 Alpha8 atlas、TLDRCS1 codec。旧cell/历史引用由调用者提供给统一GC；此阶段尚未接charset/cell，现已接通。
- `graphics_layout.rs`：基于 ordered HostGeometry 的有理数 cell 单位布局；裁剪、自然尺寸、单轴等比、双轴留白、X/Y偏移与SIXEL aspect。无可信cell geometry时明确拒绝，不能按手机本地DPI猜测。
- `graphics_state.rs`：资源、palette、font、Kitty image/number、单调placement ID，替换/失败回滚、按ID删除、冻结状态、匿名资源回收；统一GC保留字体/placement/活跃cell引用。`TLGS1` 包含完整状态与资源，严格CRC/长度/保留位/引用/有理数检查。
- `Terminal::new_with_graphics_profile`为最初的Rust开发入口，后续已补显式C ABI/JNI图形API。Snapshot 2.22必需位`0x8_0000_0000`、section `0x500`包含TLGS1；强制保存行锚点，验证page/line/cell引用，reset保持profile与单调内部ID。
- 旧 Snapshot 2.0–2.21 仍保留64MiB及原canonical bytes。2.22 开发profile单独允许128MiB；旧 C/JNI import 明确拒绝2.22，旧 Render V1–V4拒绝该profile。当前产品 `2.21_v2`、五库与lock未更新，未修改应用传输/crypto预算，未开放任何产品图形开关。

验收限制：早期图形state模块测试使用直接调用，后续已有Terminal::Output → parser → state → Snapshot/Render V5/History V2的合成输出测试，仍不等同于真实PTY/Relay/屏幕通过。Windows受控绘制证据见文末；Android绘制、真实手机、升级/混版本及长稳仍待验收。没有导出五库、替换主客户端、提交或发布。

本地证据：应用 `artifacts/validation/s6-resources-20261010.log`（4 资源专项）、
`s6-sixel-20261010.log`（4 新解码专项）、`s6-resources-full-20261010.log`（605 core + 10 golden + 59 FFI）、
`s6-resources-clippy-20261010.log`（workspace/all-targets -Dwarnings）。均已通过；未导出含这些新组件的五库。

设计记录：[图形资源契约](GRAPHICS_RESOURCE_CONTRACT_20261010.md)。首轮记录的接入问题（后续实现与剩余门禁见文末）：

1. 当前 Snapshot 64MiB 的独立最坏界为 66,070,235 bytes，只有约 1MiB 余量；不能直接追加 32MiB 像素。
   需设计受版本/profile 保护的更大 snapshot 对象或分离资源对象，并同步 crypto/history/两端解码预算；禁止只增 core 常量。
2. DRCS glyph 需显式资源引用，不用 Unicode PUA 偷换真实文字；引用贯穿 cell/copy/reflow/snapshot/render。
3. placement 需主副屏/DEC 页、滚动历史锚点、擦除/裁剪/删除和冻结 capture；其资源和 tombstone 接 S7 delta。
4. Kitty PNG/deflate、分块及受限 Windows broker，必须限制压缩、解码及临时资源预算，禁止 portable core 直接读任意路径。

该阶段证据（应用 ignored artifacts/validation）：

- `s6-zlib-20261010.log` 2专项；`s6-png-20261010.log` 4专项含35原始像素oracle。
- `s6-kitty-20261010.log` 5专项；`s6-drcs-bank-20261010.log` 7项含原DRCS解码。
- `s6-layout-20261010.log` 3项裁剪/等比/偏移/geometry/上限。
- `s6-snapshot-full-20261010.log` 为 Snapshot 首轮全量记录；最新结果见下面续作记录，不得用早期组件通过记录替代最终结果。
- `s6-snapshot-clippy-20261010.log` workspace/all-targets -Dwarnings通过。

全量测试中的legacy V2.6目录断言已排除新增0x500段（legacy导出根本不含该段），保留旧bytes/profile检查。
自查中还纠正了“行ID必须非零”的错误假定：本core初始行ID为0；图形锚点允许0，禁用u64::MAX。
两处损坏测试曾将合法payload位变更且重签CRC当成必然非法，已改为真正非法的reserved/范围位；没有削弱解码约束。

参考文档仅用于行为，自有实现和测试；未复制第三方代码/图片。SIXEL 和 Kitty 参考链接见契约；
DRCS 精确文档为 https://vt100.net/docs/vt510-rm/DECDLD.html （已下载到 ignored validation 目录）。

## 续作：图形输出、渲染帧与接入复核（完整接线前的记录）

- 修复 Render V5 range lint，未降低 clippy 门禁。
- Render V5包含完整V4 text、只被当前视图引用的TLGRS1像素及按z/image/placement排序的图像绘制记录。支持负row的上方裁切投射，实时/历史窗口不会修改权威。此阶段尚缺DRCS glyph与滚动clip，现已补入；格式仍为开发契约。
- TLGRS1 新增借用式资源目录预检。Render V5 和 TLGS1 会先验证全部坐标、比例、资源/字体引用，再复制像素；坏引用测试确认图像分配次数为零。无需重复复制像素或重复计算资源 CRC。
- 快照和渲染共用有界行索引，修复双宽行中不可见列被误认为合法锚点的问题；索引处理宽字提前换行及无序 logical ID，替代每张图片线性扫描全部历史行。
- APC 仅在图形 profile 捕获 G 指令，独立限制控制头 1024 bytes/每段 Base64 4096 bytes。CAN/SUB、非 ST ESC、双 ESC 均不提交半个 APC；新的 ESC 仍保留 profile。控制头进入诊断，图片/文件载荷隐藏。
- Kitty 分块前缀改为不可变链；事务 clone 为常量操作，完成时一次复制组装。自定义迭代释放，16384 段在 128 KiB 测试栈上完成与取消，不递归释放到栈溢出；冻结旧前缀保持有效。
- 实验 Terminal 入口已分派静态 Kitty upload/query/display/delete，依赖已有 ordered geometry；接入 cursor C=1/默认移动、Host Effect V2 单次 ACK、C1 回复和安全的旧宿主投射过滤。出屏光标采用规范允许的夹紧策略，不因放图隐式滚屏。
- 多 APC 上传未完成时阻止 Snapshot/Render；prepared apply 取消不会提交像素、回复或序号。明确 delete/RIS 可中止上传，普通 ED2 不误中止分块。
- 删除补齐 a/A、c/C、p/P、q/Q、x/X、y/Y、z/Z；坐标按 1 起算，当前页与隐藏页隔离，大小写区分保留/释放未引用数据。ED2 清可见 Kitty placement，1047/1049 清备用屏 placement，文本局部擦除保留图片。
- 配额满时优先淘汰最旧且无 placement/font/cell 引用的上传；全部被引用时原子拒绝。外部冻结 Arc 不失效，不以放宽 256 资源或 32 MiB session 上限消除错误。
- 此阶段DECDLD只接通parser → font bank → Snapshot，当时未接charset/cell；后续已补真实Cell及Render V5字形引用，不能沿用只下载成功的测试作为显示证据。
- 有效 Kitty query 的坏像素/资源超限/未接入 broker 返回准确 EINVAL/ENOSPC/ENOTSUP，继续运行；不回显文件路径。失败的分块 query 释放未完成上传，Host Effect 回复只允许规范数字身份及有界可打印消息。未知有状态控制参数仍保留准确 token 并拒绝，不把它们静默算作成功。
- SIXEL初始16色寄存器按VT340手册chapter 2 table 2-3的RGB百分比确定；扩展寄存器16..255初始黑色，等待程序显式定义。重置恢复默认，冻结历史保持旧palette；此阶段未接模式/光标/实际分派，后续已完成接线。

续作测试证据（均位于应用 ignored `artifacts/validation/`）：

- `s6-apc-20261010.log`：3 项 APC 分块/取消/隐私/长度测试。
- `s6-apc-diagnostic-20261010.log`：APC 被 ESC/BEL 中断后，准确标记新 ESC 指令，旧私有载荷不进入诊断。
- `s6-render5-admission-20261010.log`：4 项 Render V5 与分配前校验测试。
- `s6-viewport-delete-20261010.log`：3 项图形删除测试及 1 项既有 viewport 回归。
- `s6-kitty-journal-20261010.log`：6 项 Kitty 测试，含最大分块和小栈释放。
- `s6-anchor-index-20261010.log`：行索引专项。
- `s6-terminal-dispatch-20261010.log`：11 项终端图形入口专项。
- `s6-eviction-20261010.log`：配额淘汰、冻结引用及失败回滚专项。
- `s6-query-rejection-20261010.log`：可选图形查询失败回复及继续运行专项。
- `s6-sixel-palette-20261010.log`：VT340 默认 palette/程序定义/冻结与 reset 专项。
- `s6-dispatch-full-20261010.log`：该阶段通过 **665 core + 10 golden + 60 FFI = 735 项**，0失败；不是当前源码的最终验收。
- `s6-dispatch-clippy-20261010.log`：workspace/all-targets，`-D warnings` 通过；本轮已修改的 tracked Rust 文件 `git diff --check` 通过。

该阶段尚缺SIXEL分派、DRCS Cell、滚动裁剪与跨端帧接线；这些项目的后续进展见下一节，Windows broker及产品传输/profile/真实设备门禁仍未完成。

默认色行为参考 https://vt100.net/docs/vt3xx-gp/chapter2.html ，下载正文留在
`artifacts/validation/sixel-color-spec-20261010.txt`；原 HTML 67239 bytes，
SHA-256 `43c93703998162fa47cba0fcf8dd831ecf0efd46f18ce24c021114c8c0a3433b`。
仅按协议数值自行实现百分比转换，未复制第三方代码、图片或字体。

## 完整接线与统一验收基线

- SIXEL已通过Output → parser分派，背景/未选寄存器前景消费当前SGR、OSC及DECAC颜色；显式`#`继续使用独立VT340 palette。透明像素、不透明sixel完整六点背景、P1/raster比例、有序HostGeometry及准确header/payload诊断已覆盖。纯palette下载不要求像素geometry；失败/CAN/SUB/prepared取消不提交palette、资源、cursor或滚动。
- DECSDM仅在图形profile中可设置/查询，状态保存在TLGS1：默认及`?80h`启用从文本cursor绘制并滚动，`?80l`从页左上绘制、裁到页边界且保留cursor/wrap。光标依据SIXEL当前x/y和有序cell尺寸计算；图形布局和有界CPU/像素预算不依赖镜像端DPI。
- DRCS已接原始designation、94/96集、GL/GR/SS、单列Cell和REP；copy/reflow/滚动历史/快照保留显式A8矩形引用。字体替换后旧Cell、REP及冻结capture继续引用旧atlas；统一GC收集所有页、主副屏及历史中的活跃引用，不以Unicode PUA代替字形。
- `terminal_graphics_motion.rs`消费实际滚动日志，处理局部边距裁片、历史淘汰、ED3、resize/reflow及行属性改写；origin与clip可保留原锚点离开窗口后的可见图片部分。SIXEL和Kitty采用各自擦除策略。最新性能cache与ECH等hooks已追加，尚须最终回归，不能据此前通过日志宣称所有生命周期边界已验收。
- Snapshot仍为实验2.22：grapheme table v2含Cell/REP glyph引用，charset section v4含原始designation，required section `0x500`含TLGS1。TLGS1为64-byte头、168-byte placement；新增i32 origin_x/y和allocation相对半开clip，头offset 44保存DECSDM。像素物化前检查资源、字形矩形和placement锚点，旧profile继续严格拒绝新必需状态。
- 当前保守容量界：TLGRS1为33,563,440 bytes，TLGS1为34,258,736 bytes；Snapshot 2.22为111,470,186 bytes，低于128MiB。旧2.0–2.21的独立界仍66,070,235 bytes，限64MiB且canonical bytes不变。详细公式见契约及`snapshot.rs`容量断言。
- Render V5为64-byte头、required features=7、144-byte image、32-byte glyph。image带有符号column/row和clip；glyph引用A8 atlas，目标须为单列、非continuation的单字符cell。帧仅携带实际绘制的RGBA/A8资源，全部引用、坐标、比例和V4目标通过后再复制像素；帧上限64MiB、最多4096 image draws。
- History V2为96-byte `TLHPV2`头、required features=1、40-byte logical-line目录与长度前缀V5 fragments。V2页上限64MiB、图形capture上限128MiB；请求仍最多128 logical lines/65,536 cells/4MiB UTF-8。V1页/capture保持16/64MiB；FFI registry仍64 captures/合计256MiB。冻结主屏capture持有不可变图形资源，后续重下载、删除、ED3/resize不修改其像素。
- 已有显式C ABI/JNI图形create/import/checkpoint/Render V5/History V2源码入口及独立.NET/Kotlin帧解码器；旧入口不自动切换profile。Windows受控Skia绘制验证为**2092项断言、8个Rust fixture通过**；Android绘制尚待测试。这些证据不替代五库导出、真实JNI/ARM64设备或Relay产品联测。

最新已核实统一日志为应用`artifacts/validation/s6-integrated-motion-20261010.log`：**713 core + 10 golden + 63 FFI = 786项通过，0失败**。该日志发生在性能cache和新增ECH hooks之前，属于整合基线；早期clippy通过记录也不代表追加修改后的最终门禁。

剩余工作包括Windows file/temp-file/shared-memory受限broker、S7资源Delta/tombstone、RPC/crypto/持久历史对象预算与immutable产品profile、Android绘制验证、真实PTY→Relay→双端恢复、ARM64真机、升级/混版本和长稳门禁。当前产品身份保持`terminal_state_2_21_v2;width=1;output=0;resources=1`，未启用图形profile，未导出五库、安装客户端、提交或发布；**不能声明S6产品完成或可发布**。


## 产品接线与内存保护复核（2026-10-10 后续）

S6 最终核心代码基线为 **721 core + 10 golden + 63 FFI = 794 项通过**，
`clippy --workspace --all-targets -- -D warnings` 通过。日志在应用
`artifacts/validation/s6-final-{core,clippy}-20261010.log`。
ECH/矩形编辑/双宽行擦除、重锚和 anchor index cache 已纳入该基线。

`Build-ToTermLink.ps1 -Development` 已成功导出 Windows DLL 和四 ABI SO，
`Verify-NativeArtifacts.ps1` 验证五库哈希通过；锁文件仍 `source_dirty=true`。
它们是 S6 开发库，不包含后续正在开发的 S7；不能把更新文档或新增 S7 源码
说成已重新编译入库。未替换主客户端、提交、推送或发布。

| 验证 | 实际结果与证据 |
|---|---|
| Windows semantic profile | 独立 382 条断言通过；新旧握手/profile 持久化及未知状态拒绝 |
| Windows 非静音 Kitty 应答 | 62 条 managed + 4 条真实 DLL 断言通过；旧 V1 仍拒绝 APC |
| Windows Graphics startup | 11 个真实 ConPTY child 场景通过；等非零 Geometry 被 durable ACK 且应用后才 resume；实际 Avalonia 窗口集成仍待验收 |
| Windows Graphics History | 102 条断言通过，native→WAL/DPAPI→seal→HostHistory→open→V5；width2/output1 错配拒绝 |
| Windows 旧 History | ProductionTerminalHistoryValidation 完整通过，含 shadow golden |
| Windows Surface | 2100 条实际 Skia 像素/行为断言通过，含 8 个 Rust 真实 V5 fixture |
| Android 绘制/解码/Kitty | 84 项通过，0 跳过，显式设置 TERMLINK_GRAPHICS_FIXTURES |
| Android 实际 JNI | emulator-5556/API36/x86_64 上初次 6 项通过、0 跳过；不是 ARM64 或 GPU Canvas 验收 |
| Android 内存/网络 | 6 项模型预算 + 80 项网络测试通过，0 跳过 |
| Android 历史仓库 | 27 项通过，包含 8 个内存所有权/预检测试及 7 个 graphics 格式预算测试 |
| Android 产品 | 19 项定向通过，包含快照 profile、Room 与错误分类；随后新增 profile mismatch 分类测试须另记最终结果 |

相关日志：`graphics-session-startup-20261010.md`、
`graphics-surface-cache-20261010.log`、`graphics-canvas-cache-kitty-20261010.log`、
`graphics-real-jni-20261010.log`、`graphics-memory-history-app-final-20261010.log`。
曾因主机提交内存耗尽失败的 Surface 运行不算通过；上表为清理本轮构建残留后
串行重跑的结果。曾失败的测试编译和 JUnit 非 void 返回也已修正，未隐藏失败。

固定会话 profile 保持旧 `terminal_state_2_21_v2;width=1;output=0;resources=1`，
新增 `terminal_state_2_22_v1;width=1;output=0;resources=2`。Android 仅以认证后的
attach 结果创建 engine，并在重连/同代次刷新时拒绝变更。Windows Registry、
HistoryStore marker、producer 和 runtime 使用实际会话 profile，不自动迁移旧历史。
Graphics Snapshot 独立格式 `TERMLINK_TERMINAL_CORE_GRAPHICS_SNAPSHOT`，
container=3/format=1，独立 key/AAD domain；协议对象上限 128 MiB+4096、257 片。

Android 正常 heapgrowthlimit 实测 192 MiB。协议上限不等于设备可接受大小：
新增全进程 `TerminalBufferBudget`，按 `8 × objectBytes + 1 MiB` 预约当前
Room/protobuf/AEAD/JNI 多副本空间；总预约不超过 `min(maxHeap/2, 512 MiB)`，
且保持至少 32 MiB Java heap 余量。读取网络首片或 Room BLOB 前检查，完成对象
持有预约至 close/destroy，取消/失败/换代归还，每客户端最多 4 个待完成请求。
Room 先读取非 BLOB 元数据，在事务内完成准入与读取；不为绕过限制开启 largeHeap。
远端安装复用已经认证、校验并缓存的对象，避免再次读取一份 BLOB。

资源拒绝显示 `TL-H138 / TERMINAL_HISTORY_MEMORY_LIMIT`，固定会话协议不一致显示
`TL-H137`，不误报服务器不可用或磁盘满。此策略提供提前拒绝保护，**不代表已实现
192 MiB 手机上任意 128 MiB 快照恢复**。大型对象磁盘/流式处理、Room CursorWindow
实际大 BLOB 行限制和多设备峰值仍需产品压力验收；不得降低这项待办的口径。

最后复核另发现 Android 已知合法 snapshot 的 width/output 可能与 engine 构造值
不一致，已补充“先 native 验证，再检查既定 profile，交换 handle 前拒绝”的实现；
新增单元和真实 JNI 用例正在定向复测，不与上面的初次 6 项混为一谈。

未完成范围继续包含：[G01 Windows broker 设计](G01_WINDOWS_BROKER_DESIGN_20261010.md)、
[S7 全状态 Delta](S7_STATE_DELTA_DESIGN_20261010.md)、
[E02 远端冻结历史分页](E02_REMOTE_FROZEN_HISTORY_DESIGN_20261010.md)、默认 Graphics
启动与真实 Relay、H01/H02/H03 OS 验收、ARM64/IME/无障碍、混版本/回滚、性能及 2h/24h soak。
