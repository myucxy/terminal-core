# 全量终端能力实施、测试与复核计划

最新整合状态见 [2026-10-10 整合记录](INTEGRATION_STATUS_20261010.md)。以下阶段叙述保留实施过程；是否完成以最新分层证据和剩余门禁为准。

日期：2026-10-09。状态：S1 首批代码与本地验证已实施，整个 S1 和 22 组能力尚未完成。

当前进展见 [S1 诊断与编辑实现记录](S1_DIAGNOSTICS_EDITING_20261009.md)。
后续查询、SGR 栈、1007 和 ConPTY 传输证据见 [S1/S2 开发记录](S1_S2_QUERY_STACK_SCROLL_20261009.md)。
诊断 V2、DEC 报告恢复、性能及 Unicode 数据验证见 [后续开发记录](S1_S4_REPORTS_UNICODE_BASELINE_20261009.md)。
双宽/双高行、Snapshot 2.13、V4 行属性与 Android 实际绘制见 [C04 开发记录](S2_LINE_RENDITIONS_20261009.md)。
其中 C02 诊断为部分实现；C05 剩余指令已通过核心和双端绑定验证，产品 Relay 验收待完成。
Windows 原生窗口、鼠标路由、手机尺寸调整及 C14 颜色接入进展见 [最新开发记录](S2_COLORS_NATIVE_DISPLAY_20261009.md)。
传统字符集、VT52/C1、宏和展开诊断进展见 [S4 开发记录](S4_TRADITIONAL_MACROS_20261010.md)；仍须产品链路收口。
DEC 页、Snapshot 2.17/2.18、选定宿主真实尺寸及双端绑定进展见 [页与有序尺寸记录](S4_PAGES_HOST_GEOMETRY_20261010.md)；完整 profile 协商和产品 Relay 验收未完成。
终端附着能力、重连输入/重发门禁及 managed ConPTY EOF 修复见 [附着与恢复记录](S1_ATTACHMENT_RECOVERY_20261010.md)；该解码协商不代替 immutable session profile。
E04 有序宿主主题、OSC 颜色归属、Snapshot 2.19 与双端显示/历史验证见 [主题记录](S2_HOST_THEME_20261010.md)。
E02 冻结文字历史、令牌/资源预算、严格页解码和两端滚动见 [S5 记录](S5_IMMUTABLE_HISTORY_20261010.md)；标记/图形/RPC 与全量验收未完成。
E02 远端冻结分页的后续接线见 [RPC 设计提案](E02_REMOTE_FROZEN_HISTORY_DESIGN_20261010.md)；该提案尚未实现，不改变本计划进度。
Host/Agent 精确协议握手、DPAPI 固定会话描述、双历史仓库恢复门禁及 Android 错误分类见 [S1 持久协议记录](S1_PERSISTENT_SESSION_PROFILE_20261010.md)；旧数据不自动迁移，产品升级矩阵仍待验收。
DECSCPP/DECSNLS 的真实窗口调整、Host Effect、ordered result 与双端/Relay 验证见 [C09 有序请求记录](S4_ORDERED_PROGRAM_RESIZE_20261010.md)；当前协议身份为 `terminal_state_2_19_v3`。
OSC 7/133、Snapshot 2.20、双端目录/命令定位及真实 PowerShell 7 验证见 [H01 Shell 集成记录](S3_SHELL_INTEGRATION_20261010.md)；最新产品协议身份为 `terminal_state_2_20_v1`，全量阶段仍未完成。
H02 已推进至 Snapshot 2.21 / `terminal_state_2_21_v1`，双端开关、持久去重、声音/进度与受管旧外层投射保护已接入；产品及真机验收未结束，见 [开发中记录](S3_HOST_SIGNALS_IN_PROGRESS_20261010.md)。
H03 当前为 `terminal_state_2_21_v2;width=1;output=0;resources=1`，OSC52 有界文本写入、双端逐次确认、live receipts 与 Windows 非阻塞授权已接入并本地测试；真实 Relay/OS 剪贴板/真机仍未验收，见 [剪贴板开发记录](S3_CLIPBOARD_IN_PROGRESS_20261010.md)。
S6 实验入口已接通SIXEL/Kitty输出分派、DRCS charset/Cell/REP、滚动裁剪与重锚、Snapshot 2.22（grapheme v2/charset v4）、TLGS1 168-byte placement、Render V5（144-byte image/32-byte glyph/required features=7）和96-byte头History V2。图形Snapshot/capture限128MiB，Render V5/History V2页限64MiB，产品RPC/crypto预算与profile尚未切换。已核实整合基线为713 core+10 golden+63 FFI，早于最新性能cache/ECH修改；Windows Skia为2092项断言、8个Rust fixture通过，Android绘制待测试。Windows broker、S7资源Delta、真实Relay/ARM64及产品恢复门禁仍未完成；产品保持2.21_v2，不能声明S6产品完成或可发布，见 [S6开发记录](S6_GRAPHICS_IN_PROGRESS_20261010.md) 与 [图形资源契约](GRAPHICS_RESOURCE_CONTRACT_20261010.md)。

最新接线补充：S6 五库已导出并校验（DEV dirty），核心794项/clippy通过；Windows Graphics History102断言、Skia2100断言、Android绘制84项和模拟器初次JNI6项通过。已接固定Graphics会话profile及独立加密域，Android内存/网络/历史/产品定向验证已开展，详见S6文末“产品接线与内存保护复核”。默认生产启动仍Legacy，完整产品及压力门禁未完成。S7已开始core编码，不在当前导出五库内；[Delta设计](S7_STATE_DELTA_DESIGN_20261010.md)、[Windows broker设计](G01_WINDOWS_BROKER_DESIGN_20261010.md)仍分别保留实现/验收待办。

用户要求将上一轮清单中“未实现”和“部分实现”的能力全部补齐。本计划接替旧路线中的“按需选择”安排：
低优先级只表示实施较晚，不表示删除范围。完成条件以具体协议矩阵及平台证据为准，不宣称覆盖无限扩展的所有终端协议。

## 1. 范围与当前基线

- 必做范围：C01–C14、H01–H04、E02–E04、G01（Kitty graphics），共 22 组。
- E01 外层终端回复过滤已完成首批代码和针对性测试，作为全程回归项，不重复统计为新增缺口。
- C01 的几何查询安全降级/18t、C05 的 DECSTR、C03/E04 的准确声明已有 DEV 实现；其余子能力仍须完成。
- 最新实测以 [查询修复记录](QUERY_COMPATIBILITY_FIX_20261009.md) 为准；
  [Windows Terminal 对照](WINDOWS_TERMINAL_CAPABILITY_REVIEW_20261009.md) 是修复前基线。
- 本次规划时 core HEAD 为 `e6e849172c6fca0de01ae0799eae97ad9483e687`，应用 HEAD 为
  `d6fe9c6f4b21d9babc3c3b8f52ed6364455f5ab4`，两仓库均有未提交修改；HEAD 本身不包含全部 DEV 修复。
- 已有测试记录：Rust/core/golden/FFI 526 项、Android 单元 147 项、模拟器 instrumented 22 项、
  .NET 验证、故障 WAL 36/36、隔离真实 Claude 启动/退出。本轮制定计划没有重新执行这些测试。
- 当前修复库为 `source_dirty=true`；主客户端未升级、公网 Relay 完整产品联测未完成，不属于正式发行库。

以下状态必须分别记录：设计完成、核心通过、绑定通过、双端功能通过、Relay 恢复通过、发布完成。
不能用其中一个代替其他状态。新增项按本批实现记录更新，其余保持“计划”，已有子项按原修复记录保留。

## 2. 不变的架构边界

1. Windows SessionHost/core 维持唯一权威。Android 是镜像及输入端；Server 负责认证、授权、路由与已有存储职责，
   不新增终端解释器，不解密终端内容，也不成为当前网格/历史的业务权威。
2. core 仓库保存 Rust、C ABI、JNI C++、CMake 和库构建脚本；应用仓库保存 .NET/Kotlin 绑定、宿主、View 和协议集成。
   通过 `scripts/Build-ToTermLink.ps1` 导出一个 Windows DLL 和四 ABI SO，连同应用锁文件核验。
3. 不直接接入 Termux 或 Windows Terminal 的核心。参考公开协议/行为并自行实现；Unicode 数据、字体、
   测试向量或库若引入，先核对固定版本、来源、校验和、许可证及署名。
4. 输入编码、控制租约、durable ACK、E2EE、事件顺序、Gap 和 detached snapshot 安装仍按现有契约执行。
   不把鼠标/键盘等协议分别硬编码到按钮，不改 Android 原 CLI 页面布局。
5. 只执行 Relay Only 验收。LAN/NAT、打洞及直连切换记为“不适用”，不能记为测试通过。

## 3. 必须先解决的共同设计

### 3.1 协议矩阵与诚实的能力声明

每一组在编码前列出：序列/selector、参数和缺省值、状态变化、应答、宿主动作、重置/恢复、资源上限、
拒绝行为、测试 ID。以现有对照所列具体行为及固定的协议版本为完整范围；不能只写“支持 DSR/Kitty”。

可选只读探测使用准确失败回复或协议允许的忽略策略；未知有状态指令、坏快照、丢序号和越界继续保护一致性。
拒绝影响限定在准确的会话/renderer incarnation；不得以保持连接为由继续展示不可信状态。
一次操作失败不能重置全局连接、杀死其他会话或推进未成功应用的 ACK。

应用创建会话时协商 profile；width/资源等影响布局的 profile 在该会话内固定。
已有会话进入新模式必须有有序转换事件及完整检查点，不能由后来连接的设备静默改写。
旧客户端不能表达的新必需状态，在解析前明确拒绝附着或提示升级；不能宣称可无损回退。

### 3.1.1 不支持协议的准确诊断（用户补充，纳入 C02/S1）

遇到不支持的协议、指令或参数，必须同时给出准确原因和对应的不支持序列/字符，
不能只有 `Unsupported`、`FailedClosed`、TL-H122 或“服务器不可用”等笼统信息。
此项是全部协议能力的共同验收要求，纳入 C02，不另增能力组计数。

| 诊断内容 | 要求 |
|---|---|
| 原因 | 区分协议族未实现、具体指令/selector/参数未支持、版本或 feature 不兼容、格式损坏、资源超限；拒绝授权、丢序号等保留自己的类别 |
| 原始序列 | 提供可读转义表示和对应原始字节十六进制；保留私有前缀、中间字节、分隔符、缺省参数及终止符，不用规范化重写后的序列冒充原文 |
| 不支持的位置 | 标明具体 selector/参数/字节及原因；能够定位到单个字符时显示该字符及有效 Unicode 的 U+ 码点；无效 UTF-8 用字节表示，不替换成 U+FFFD 丢失证据 |
| 来源 | 标记 Windows 权威、Android 镜像或接口解码层，以及实时输出/历史回放/输入编码等方向；记录 core revision、协议/profile 版本和会话诊断标识 |
| 源坐标 | 记录序列起止的 event sequence 与事件内 0 起算字节偏移，结束偏移不包含结束字节；另记实际报错位置。跨事件序列不能只截取最后一帧 |
| 实际影响 | 明确“可选查询已忽略，继续运行”“当前镜像停止恢复”“当前会话已终止”等实际处理结果，不将未发生的终止、网络断开或恢复写成既成事实 |
| 根因保留 | 保存首次失败的结构化诊断，后续 FailedClosed/断线提示引用同一诊断，不能用二次错误覆盖原始不支持序列 |

表示示例（合成未知 selector，非当前故障或新支持声明）：

```text
原因：窗口控制参数未支持（CSI，参数 999，终止字符 t）
序列：<ESC>[999t
字节：1B 5B 39 39 39 74
位置：事件 42，字节 [8, 14)
```

这里的 `<ESC>` 是普通文本，不是真实 ESC 字节。CR/LF/TAB/NUL、C1、双向控制符等同样明确转义；
将序列作为纯文本显示/复制，禁止把诊断重新送入 PTY、终端 parser 或当作 HTML 执行。
普通字形缺失、字体渲染问题与“不支持终端协议”分开诊断，不能把不会绘制的字符误报为未知控制序列。

core 在错误产生处捕获有界诊断，经 additive versioned FFI/JNI/.NET/Kotlin 契约传递；
事务回滚后仍可只读取得失败证据，但诊断本身不提交失败事件、改变网格或推进 ACK。
分块 parser 保存有界序列预览及源坐标，不为诊断保留完整无限输出。超过预览预算明确标记截断/可知长度，
保留指令头和错误附近的准确片段；未捕获的字节明确写“不可用”，不能根据规范猜补。

OSC/DCS/APC 可能含剪贴板、路径、链接、文本或图像载荷。默认显示准确的控制头、selector/参数及不支持的 token；
无关载荷在转义文本与十六进制两处同时隐藏，注明隐藏范围，不连带输出前后命令或整个终端历史。
诊断跟随既有加密及会话授权链路，不把内容明文发给 Server 日志或自动公开上报。
去重/限流不丢首次根因，重复同类诊断聚合显示次数；正常降级的可选探测不反复弹窗。

验收新增：逐字节及跨事件分块后序列/源坐标准确；同帧多条指令只定位实际失败项；
私有前缀、空参数、冒号/分号、C1、非 ASCII/无效 UTF-8、超长/截断及敏感载荷显示正确；
原子回滚和后续 FailedClosed 不覆盖根因；双端错误内容一致；复制详情不执行控制字符；
忽略/终止/恢复的实际行为与提示一致，模拟器可查看并复制相同诊断。

### 3.2 像素、主题与显示的一致来源

定义 versioned geometry profile，分清逻辑行列、图形用虚拟 cell 像素和客户端实际 DPI/字体像素。
几何值由 SessionHost 选择的展示宿主提供并转成有序事件，core 不读取本机屏幕，不接受多个客户端抢写。
“按手机窗口调整”仍按收起输入法后的有效窗口、字号和一/两行 dock 计算行列。
14/16/18t 的回复必须自洽；15/19t 等物理显示查询仅在所选宿主具有可信测量时提供，
无屏宿主按明确 profile 返回协议允许的不可用行为，不能伪造手机或 Windows 物理像素。

主题同样有唯一会话来源。2031/主题查询只在有真实主题事件通道时声明；Android 本地换主题不会直接向 PTY 重复应答。

### 3.3 Windows 可见窗口是图形功能的前置依赖

当前 Windows 经外层终端投射，Android 经 `NativeTerminalView` 绘制。不能假设外层终端支持全部 DRCS、
SIXEL、Kitty、页或字素行为，也不能把外层执行 OSC 的效果当成 core 已支持。

- S0 先实测当前 ConPTY 对目标控制序列的保留/改写，包括生成进程原始输出与 SessionHost 接收到的字节。
  若序列在 ConPTY 内丢失，必须先验证可用的透传能力及其输入兼容；仅增加 parser/Canvas 不解决传输缺口。
- Windows 完整 profile 计划复用现有 Avalonia 客户端，新增消费 core Render/资源帧的终端展示窗口，
  复用同一个 SessionHost、会话 ID、控制与输入编码；不得另起第二个权威解析器或第二个 shell。
- 保留外层终端展示路径及原工作流；它只声明经实测可展示的 profile。图形能力不得在不支持的宿主上误报支持。
- ConPTY 透传、Windows 展示、Android 绘制三者未闭环时，对应能力保持“部分实现”，不能用合成输入注入代替产品通过。

### 3.4 宿主事件与回放的边界

新增 versioned Host Effect/state 契约，不向已冻结的旧结构强塞新字段。
PTY_REPLY 只由权威核心写回；cwd/命令标记/进度是可恢复状态；响铃/通知/剪贴板是受授权的即时动作。
即时动作绑定 session、source sequence、offset、ordinal、目标端及控制 epoch，以事件 ID 去重。
历史或检查点恢复只更新展示状态，不重新响铃、复制或弹通知。

剪贴板等 OS 动作与持久记录无法组成跨进程原子事务：明确采用至多一次尝试，先登记再执行；
崩溃窗口可能丢失一次提示，不能为了补发而重复复制。不能声称 OS 副作用具备严格 exactly-once。
OSC 52 先实现已列范围的写入；不新增远端读取本地剪贴板。

### 3.5 状态版本、资源和数据迁移

保留现有 ABI/JNI 名称、结构大小和旧 profile 的 canonical bytes；新增接口/feature 使用可发现的版本。
不是所有新状态都能输出旧 Snapshot：无损表达不了时明确要求新版本，禁止丢字段导出。
旧 width profile 的历史继续按旧规则恢复；新 profile 首批固定 Unicode 16.0.0（与本次 WT 对照版本一致），
按该版本 UAX #29 及对应数据实施，不使用随系统字体变化的宽度规则，不能重排既有日志。

新增 page、line rendition、SGR stack、macro、DRCS、image/placement 都纳入状态生命周期：
创建、替换、删除、reset、resize、alt screen、checkpoint、history eviction、session close。
图片/字形先定义共用有界资源、稳定 ID、generation 和引用，再实现图形协议；Delta 最后补齐这些资源操作。
文件/共享内存图像输入由 Windows 宿主受限读取并固化为可回放字节，core 与 Android 均不按输出路径打开本机文件。
分块输入未结束时继续遵守 quiescent checkpoint 门禁，不保存不可恢复的半个 DCS/APC。

## 4. 分阶段实施顺序

阶段是可验收的开发批次，不强行合成一次大提交。每阶段完成受影响回归，未通过不得扩大默认 capability。
阶段 ID 为 S0–S8，与旧路线的 P/M/Q 编号区分。

| 阶段 | 工作范围 | 依赖 | 退出条件 |
|---|---|---|---|
| S0 基线与契约 | 冻结 22 组协议子项、保留当前修复、记录测试/性能基线；确定 geometry、profile、事件去重、资源预算和 Windows 展示/ConPTY 路径 | 当前代码与修复证据 | 每组有测试 ID 和明确完成定义；已识别 Windows 传输/展示限制；不把旧候选包当新核心 |
| S1 常用兼容 | C02（含不支持序列的准确诊断）、C05 剩余编辑，C01 文本/几何契约与可信宿主报告，C07 扩展查询；E01 回归 | S0 | 查询不再错误中断正常 shell；诊断原因/原始序列/字节位置准确；回复单次、参数/分块正确；真实 PowerShell→Claude→shell 与隔离恢复通过 |
| S2 文本与模式 | C03、C04、C06、C14、H04、E04；开始 Windows 原生帧展示，保持外层路径兼容 | S0；涉及新查询沿用 S1 | 字素/光标/复制/选区/双高行一致，旧历史稳定，鼠标/键盘及主题无重复或越权 |
| S3 Shell 与宿主动作 | H01、H02、H03，版本化 Host Effect 和必要的 Relay 加密消息 | S1 的回复分类、S0 的宿主契约 | 真实 cwd 与命令标记可用；进度/通知/剪贴板授权与回放去重闭环 |
| S4 状态及传统扩展 | C08、C09、C10、C11；将页/宏/字符集纳入完整快照 | S2 的文本状态/展示基础 | 页切换/跨页复制、报告恢复、VT52/字符集和有界宏通过跨语言及双端展示 |
| S5 历史与资源基础 | E02；共用字形/图像/placement 基础、资源帧和 Windows/Android 合成 | S2、S4；标记展示依赖 S3 | 分页冻结与锚点稳定；资源创建/删除/恢复可视化；内存和失效 token 有界 |
| S6 字形与图形 | C12、C13、G01；完成 C01 与图形像素的一致性 | S5；实际 PTY 可保留协议 | DRCS/SIXEL/Kitty 在真实 Windows 与 Android 显示、滚动、缩放、删除、恢复一致 |
| S7 增量同步 | E03；对所有已引入状态/资源完成 Delta 与 Relay 恢复集成 | S2–S6 | Snapshot+Delta 与完整状态相等；丢包/重复/错基线安全重同步；不双重应用原始输出 |
| S8 全链路收口 | 22 组逐项复核、旧版混用、长时间稳定性、真实设备、升级/回滚演练 | S1–S7 | 所有必做项均有完整证据；无未关闭的高风险缺陷；发布资格与发布动作分开记录 |

S1 的故障修复可以独立形成候选，不必等待图形功能完成；是否正式发布另按用户发布指令执行。
History/Delta API 本身不应迫使 Server 解释终端内容；若消息尺寸/路由能力需改 Server，先做向后兼容迁移。

## 5. 22 组逐项验收矩阵

下表列专有验收；每行还必须执行第 6 节共用检查。不能将“返回 OK”作为唯一断言。

| ID / 阶段 | 必须实现的范围 | 专有测试与完成证据 |
|---|---|---|
| C01 / S1、S6 | 尺寸族完整策略，18t 字符尺寸、14/16t 统一像素 profile，15/19t 及参数变体的可信来源/不可用语义 | 行列与像素计算一致；字号/DPI/横竖屏/IME/dock 变化；无屏、多端、未获控制权不能改 authority；query 与 resize 同序 |
| C02 / S1 | 全部已登记可选查询/提示的兼容分类；未知有状态指令与坏数据仍严格处理；3.1.1 的准确错误及不支持序列展示 | 合法探测后可继续输出/输入；无效、超限、gap 不提交半状态；错误不波及其他会话；双端显示原因、转义序列/十六进制、失败项及跨帧坐标，保留首次根因 |
| C03 / S2 | 固定 Unicode 版本的完整 UAX #29 字素分段及终端宽度 profile；正确的 2027 语义 | 国旗、肤色、ZWJ、VS、组合字符、Indic、孤立组合符；每个 UTF-8/事件断点；换行/选区/复制/reflow/旧快照一致；官方数据须有许可 |
| C04 / S2 | ESC #3/4/5/6/8、行级 rendition、有效列与裁剪、对齐图案 | 上/下半行、奇数列、光标、擦除、滚动、选区、resize、双端截图及 Snapshot 后缀恢复 |
| C05 / S1 | ESC 6/9、CSI ?5W；保留已完成 DECSTR，完整 reset 交互 | margin 边缘、tab 设定/清除/恢复、wrap、主副屏、光标/保存状态；DECSTR 不清空应保留历史、不错误重置输入协议 |
| C06 / S2 | SGR push/pop 与别名、选择性属性、固定栈预算 | 空栈、满栈、多层、truecolor/扩展下划线、保存/恢复和 reset；不丢未指定属性 |
| C07 / S1 | DA3、终端参数、private DSR、显示范围、矩形 checksum 的已登记 selector | 预期回复逐字节校验、真实能力声明、矩形坐标/保护区/字符编码；分块/混合回复归属及外层去重 |
| C08 / S4 | CSI $u/$w、DCS $p/$t 对应的颜色/光标/tab 状态报告与恢复 | 报告→修改→恢复相等；坏参数/重复字段/截断不部分恢复；与私有 Snapshot 明确分离 |
| C09 / S4 | 有界 page store、active/visible page、NP/PP/PPA/PPR/PPB、页尺寸、跨页 DECCRA | 不同页的光标/内容、跨页矩形/重叠复制、页上限、resize、主副屏、Snapshot/资源归属 |
| C10 / S4 | VT52、明确的 C1/GR 切换、UPSS 与冻结列表的扩展字符集 | UTF-8 默认不误认高位字节；模式进出、直接光标定位、G0–G3/GL/GR/SS、分块、历史恢复及字符映射 oracle |
| C11 / S4 | DEC 宏定义/调用、替换/删除、编码、固定深度/指令/字节预算 | 嵌套与递归预算、取消、非法编码、reset、快照；超限不中断其他会话、无无限循环、无半提交宿主效果 |
| C12 / S6 | DRCS 解码、glyph bank、designation、覆盖/擦除、资源与双端绘制 | 字形像素黄金样本、字符集切换、尺寸/DPI、替换、删除、Snapshot/历史引用与资源回收 |
| C13 / S6 | SIXEL 颜色、repeat、raster、透明/背景、placement、滚动/擦除及资源生命周期 | 流式/取消/截断、颜色与 alpha、margin/alt/resize、超大 repeat/解码上限、双端图片恢复 |
| C14 / S2 | OSC 21、17/117、DECAC 与现有标题/调色板整合 | 标题真实改变；选择区颜色/重置实际显示；主题与保存恢复互不覆盖；有序宿主标题更新 |
| H01 / S3 | OSC 7/133、cwd/prompt/command/output/end、退出状态、按命令定位/打开目录 | PowerShell 7 启动会话真实 cd；中文/空格/URI 编码、非本机 host/UNC/越权路径；退出状态与阶段跨重连恢复；日志伪造标记不执行命令 |
| H02 / S3 | BEL、OSC 9 进度、DECPS 的核心事件与平台通知/声音适配 | 前后台/静音/限流/权限拒绝、进度完成与取消；旧输出重放不响铃；用户未允许的声音不强行播放 |
| H03 / S3 | OSC 52 写入的解码、长度和宿主授权；明确不开放读剪贴板 | 合法/空/非法 Base64、超限、控制权切换、后台限制、拒绝路径、不同终端隔离、回放/重试不重复复制 |
| H04 / S2 | 1007 状态/查询/恢复及备用屏幕滚轮转方向键 | 主屏历史/备用屏幕/鼠标报告的完整组合；Android 单指原操作不变；滚轮按钮和本机滚轮分别验证；不双发 |
| E02 / S5 | immutable capture、sealed logical line、exclusive anchor、分页/token/过期/截断 | 同 token/anchor 字节一致；并发输出不改变 capture；无重漏/孤立宽字符；reflow/ED3/alt/reset/淘汰和错误 terminal 明确返回 |
| E03 / S7 | dirty journal、base/target revision、稳定资源 ID/tombstone、Gap/full-resync | 全状态 Snapshot 等价；重复幂等、乱序/损坏/未知 required feature 拒绝；晚到资源/删除、journal 淘汰、断线后重同步 |
| E04 / S2 | 2031 主题订阅/查询/变更及准确声明、权威主题来源 | 初始/切换/禁用/恢复、未知主题、两个客户端主题不同；仅当前权威宿主产生 PTY 回复 |
| G01 / S6 | Kitty 图形 transmit/display/query/delete，ID/placement、chunk、压缩、裁剪/z-order，以及所列传输介质的宿主适配 | direct/file/temp-file/shared-memory 逐项记录；PNG/RGB/RGBA、分块/替换/删除/quiet 回复、真实 PTY；无任意路径读取、共享内存越权或解压炸弹 |

G01 的文件、临时文件、共享内存不是 portable core 的文件系统能力。Windows 宿主需完成对应介质的
受限 broker，并把实际数据纳入同序可回放链路。平台确实不提供的机制按平台 profile 明确标注，
不得把“返回不支持”算作该介质已实现；若缩减上述既定范围，应显式记录为未完成，而不是悄悄从表中删除。

## 6. 每组固定测试流程

### T1 核心行为与参考结果

先建立能暴露缺口的测试，再实现。每组覆盖正常/缺省/边界/非法/未知 selector、逐字节分块、随机分块、
多序列同事件、CAN/SUB/终止符断开、随后普通文本、资源上限。不能只按实现的分支写同构断言；
测试 oracle 来自固定协议语义、自有黄金输入/图像及许可明确的数据。

Unicode 覆盖固定版本 GraphemeBreakTest 及终端宽度专项。参考 Windows Terminal 时固定 revision、
配置和字体，记录行为差异；其自身缺失项不作为 TermLink 的否决 oracle，不依赖 Termux 差分。

### T2 状态、快照和资源原子性

对每组在合法 checkpoint 边界验证：

```text
replay(events[1..N]) == restore(snapshot@S) + replay(events[S+1..N])
apply(snapshot@S, delta(S..N)) == snapshot@N   # S7 起对所有新增状态执行
```

比较网格、光标、modes、页、行属性、charset、tab、palette、hyperlink、命令标记和资源内容/引用；
即时宿主动作另按 live/replay 规则校验，不要求回放再次执行。必须覆盖 reset、ED3、resize、alt screen、
截断历史、资源删除、检查点失败、崩溃后恢复。旧 profile 向量保持字节稳定。

### T3 FFI、JNI、.NET 与帧解码

执行 Rust/FFI、NativeTerminalCoreValidation、Android api/native 单元和真实 JNI instrumentation。
新增版本覆盖短头、错长度/CRC、整数溢出、未知 feature、空指针、错误终端 ID、同库释放、重复释放防护策略。
核对 Windows DLL 与四 ABI SO 的 hash、来源及 lock；四 ABI 编译成功不等于四 ABI 真机都已运行。

### T4 Windows/Android 展示和输入

Windows x64：SessionHost、真实 ConPTY、外层回复过滤、自有展示、鼠标/键盘/剪贴板。
Android：API 36 模拟器和 ARM64 真机；中文 IME、硬键盘、选择复制、长历史、单指滚动、左右滚轮、
字体大小、主题、横竖屏、IME 收起/弹出、dock 一/两行、“按手机窗口调整”。
文本布局比较格子和光标；图形比较受控软件像素/资源数据，平台字体抗锯齿使用单独容差，不能用模糊截图容差掩盖错列。

### T5 真实 Relay 联合测试

在隔离新会话执行 Android → Relay → Windows：PowerShell 7 普通输出、启动 Claude/Codex、输入、退出到 shell；
再做 Windows 本地输入→安卓同步、控制权切换、二次附着、离线重连、检查点及历史恢复。
可用工具分别测 pager/editor/TUI；缺少安装/登录不能冒称实测通过，协议测试和真实应用测试分列。
图形必须由真实进程通过实际 PTY 发出，而不只向 core 测试接口注入。

注入 Relay 延迟、丢连接、乱序/重复的测试层消息、慢消费者、队列背压、登录失效、会话关闭与恢复竞态；
校验无重复输入、无错推进 ACK、无旧会话闪现、无跨会话泄漏、无重复宿主效果。
服务端页面/目录/AI 功能保留受影响的回归，Server 不因大图形/快照阻塞其他会话。

### T6 混合版本、升级与回滚

矩阵：新 Windows+新 Android、新 Windows+旧 Android、旧 Windows+新 Android；Server 版本按是否改协议另加新旧组合。
正常旧功能可用；新必需状态明确协商/拒绝，禁止降级解析不完整图像、页或字素状态。
升级保留账号、密钥、会话元数据、旧 WAL/Snapshot；新 profile 会话无法被旧版本读取时，
必须在回滚前说明并隔离，不许清库或覆盖原始记录。已有外层窗口路径也须验收。

### T7 压力、模糊和资源验证

S0 测量并冻结 baseline，不把下列目标写成已测成绩：

- 纯文本同样输入的解析吞吐、快照耗时、内存较基线不得出现未解释的 >10% 退化；至少预热后 5 轮，报告中位数与 p95。
- 文字交互采用固定设备/字号场景，60Hz 目标下 p95 UI 帧耗时 ≤16.7ms；记录设备无法达到时的原因及优化，不能省略坏结果。
- 10 万逻辑行分页，达到配置上限后明确截断；持续输出/分页不阻塞输入；不以突破硬上限完成压力测试。
- 至少 1000 次 resize/主副屏切换，100 次图像创建/替换/删除回收周期，50 次隔离会话创建/关闭，无持续增长句柄或资源。
- 阶段候选先做 2 小时混合输出/重连 soak，最终候选做 24 小时；期间错误、峰值 RSS、资源回收、输入延迟留档。
- 有状态 parser、Snapshot、Delta、DRCS/SIXEL/Kitty 做确定种子变异和模糊测试；阶段 smoke 与最终长测分开。
  必须包含压缩炸弹、超大尺寸/计数、深递归、资源缺失和 allocation failure 注入；工具缺失按 D:\Dev 策略处理。

每个资源在实现前固定常量表：每会话/全进程预算、单项尺寸、页/栈/宏/图片数量、capture/token 生命周期、
chunk 和解压上限、CPU/指令预算。数值由 S0 测量和协议限制决定，S1 起不得留下“无限”或未检查的待定项。

## 7. 复核流程与证据

每阶段按顺序执行，不把全量测试仅留到最后：

1. 设计复核：检查协议矩阵、依赖、权威边界、版本/回滚、预算和专有用例是否齐全。
2. 代码复核：从入口追到 parser→state→Snapshot/Render→FFI→宿主/View→消费者，检查遗漏调用点。
   特别检查旧 decoder、新模式 reset、历史回放、租约 epoch、外层 OSC 副作用绕过和两端重复应答。
3. 测试复核：先局部再本阶段必需的完整套件；检查失败/跳过/未运行，构建通过不能代替运行通过。
4. 独立复核轮次：实现完成后另做一次基于最终 diff 和验收矩阵的审查；这是独立步骤，不虚构第二位审查者。
   若由同一执行者复核，记录 self-review；若以后使用其他审查者，记录真实身份与版本。
5. 故障注入和真实产品验证：完成 T5–T7 的适用部分；发现新改动后重跑受影响门禁。
6. 更新能力状态：只有核心、绑定、展示、恢复和相应设备证据完整才标记“产品完成”；未覆盖列明原因。

每项记录至少包括：ID/子项、协议版本/参考来源、core/app revision 与 dirty 状态、DLL/SO hash、
工具版本、测试命令、期望/实际、日志路径、截图/受控录屏、设备/系统版本、性能与资源上限、限制和回滚结果。
本机原始日志放应用 `artifacts/validation/terminal-capabilities/<阶段>/<运行标识>/`（ignored）；
可提交的验证摘要不含真实私有会话、账号、路径或凭据，使用脱敏自有 fixture。

出现数据丢失、越权、跨会话影响、重复输入、无法恢复、native 崩溃或 capability 虚报，阶段验收直接失败。
其他未通过的功能仍标记未完成；不能以“低优先级”关闭用户已经纳入的需求。

## 8. 构建、发布资格与接续

工具沿用 `D:\Dev\DEV_ENV_MANIFEST.json`，构建前运行 `Ensure-DevEnv.ps1`，Rust 固定 1.97.1。
共用 obj 的 .NET 构建/验证串行；Android 用仓库 Gradle、既有 JDK/SDK/NDK。
DEV 数字版本取当次核验的最近正式发布，保持不递增；本计划不预分配正式版本。

正式分发前按应用 `docs/RELEASE_RUNBOOK.md`、`docs/RELEASE.md` 完成：core 已提交源码重建 clean 五库，
应用锁文件和代码固定，签名连续性、测试/回滚、GPLv3 对应源码交付及实际上传状态核验。
只为实际变更的平台构建/发布；Docker 如涉及改动，按用户规则本机编译直推 Docker Hub。
制定此计划不执行提交、推送、升级、AppShip 或 Docker 发布，也不代表获得这些外部操作的新授权。

下一实施起点：继续补齐 S0 矩阵、ConPTY/Windows 展示证据及 S1 查询/诊断剩余项；
同时保留当前 Claude 修复可独立验收的边界。平台落地步骤见
[应用集成计划](../termlink/docs/TERMINAL_CAPABILITY_INTEGRATION_PLAN_20261009.md)。

## 9. 另行核查但不冒充已确认缺口的内容

Bidi、复杂文字 shaping、连字、GPU/DPI 和无障碍需要专项核查。S0 为它们建立“已有/部分/缺失/未核查”记录，
避免字素或新展示窗口破坏现有能力；本轮不能把“未核查”写成已完成或随意计入上述 22 组。
Windows Terminal 的标签页/分屏/WSL 启动器不是 portable core 缺口；本计划不替换 TermLink 既有 CLI/AI 产品导航。
