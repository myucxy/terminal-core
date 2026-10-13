# terminal-core 与 Windows Terminal 能力对照

复核日期：2026-10-09。本文保留修复前基线；随后完成的首批 DEV 修复和验证见
[查询兼容修复记录](QUERY_COMPATIBILITY_FIX_20261009.md)。尚未正式发布，以下 18 组是原始对照范围，不能当作修复后的剩余组数。

## 1. 结论与统计口径

本轮确认 **18 组差距：14 组核心行为、4 组需要平台共同实现的能力；其中 8 组部分实现，10 组缺失**。
以下 C01–C14、H01–H04 是固定统计范围，每组只计一次。C02 是兼容错误处理差距，其他组是协议/行为族；
它们的工作量不同，不能换算成“还差 18 条指令”或完成百分比。这是已确认清单，不是 Windows Terminal 全功能穷举。

常用文本终端基础已存在。当前最紧迫的是可选探测导致整个会话退出，其次是字素宽度、软复位和 shell 集成。
SIXEL、DRCS、多页和旧终端仿真属于后续扩展，不能与 Claude 启动故障同等排序。

另列的 Windows 外层回复去重、History Page、Delta、2031 状态声明问题，不混进这 18 组。
Windows Terminal 的桌面标签页、分屏、设置、WSL 启动器等不属于 portable core 的职责。

## 2. 基线、方法和证据边界

- core 源码：`e6e849172c6fca0de01ae0799eae97ad9483e687`。本轮仅修改文档；已有许可证工作不改变运行逻辑。
- 应用源码：`d6fe9c6f4b21d9babc3c3b8f52ed6364455f5ab4`；应用锁文件指向上述 core revision。
- Windows Terminal：官方 main 在本次检查时的固定 revision
  [`6676c6f938f7e98ad680f0cecedd3bafb6529734`][wt-revision]，提交时间 2026-10-08 03:46:25 UTC。
  这是源码基线，不代表每个 Microsoft Store 已发布版本都有同样能力。
- 逐项检查 TermLink parser、dispatch、state、输入、Host Effect、Snapshot/Render 契约；
  Windows Terminal 检查分派后的实现与相关单元测试，不能仅凭 enum 名称宣布支持。
- 在独立进程中加载现有绑定/DLL，对 **56 个合成输入**逐例创建新核心，检查状态、回复、光标/标题，
  再输入普通字符检查核心是否继续工作。安装 DLL 和应用锁定 DLL 各运行一轮，56 项结果一致。
- 结果：25 项被接受、31 项 Unsupported；后者的下一条普通输出均为 FailedClosed。
  这些用例刻意针对疑似缺口，比例不能当作兼容率。被接受也不代表执行了该能力，例如 OSC 52 被消费但没有剪贴板效果。
- 未启动、关闭或改写用户终端，未访问剪贴板；本轮没有构建核心、运行完整 Rust 测试、
  Windows Terminal 二进制差分测试或安卓/Windows 端到端验收。

本机可追溯材料位于同级应用仓库的忽略目录，本文已保留关键结果，阅读本文不依赖私有会话内容：

- `artifacts/reference/microsoft-terminal-20261009/`：固定版本上游源码与 MIT 许可证参考副本。
- `.appship/Probe-CoreCapabilityReview.ps1`：合成输入探针；不进入产品构建。
- `artifacts/validation/core-capability-probes-20261009.json`、`core-capability-probes-20261009-locked.json`。
- `artifacts/validation/claude-cli-root-cause-20261009.md`、`claude-cli-diagnosis-20261009*.json`。
- `artifacts/validation/claude-cli-geometry-reply-probe-20261009.json`。

安装 native DLL SHA-256：`1c2f997df44f26def0b2305ab84ecf4bd724d59c0be5e9ca26cb5a0ff277d3f4`。
锁定 native DLL SHA-256：`4463372df279f17f5c7b1f80b6f4cbed3567dd5413cb128563d8c1e400a8e63d`。
安装文件经过 Authenticode 签名，因此不同哈希不单独构成损坏证据；两者均复现查询故障。

## 3. 已有能力，不应重新记为缺失

这里列出已实现的基线，不声明相应协议的所有变体或所有终端行为都已兼容。

| 能力 | 已有内容 | 本地依据 |
|---|---|---|
| 基础 VT 编辑 | 光标移动、插入/删除/擦除、滚动、自动换行、动态 tab stops、REP | `lib.rs`、`screen.rs`、`tabs.rs`、`tests.rs` |
| 屏幕与区域 | 主/备用屏幕、保存/恢复光标、上下及左右 margin | `lib.rs`、`modes.rs`、`screen.rs` |
| 字体属性和颜色状态 | 256 色、truecolor、扩展下划线及颜色、粗/淡/斜体、删除线、反色等 | `lib.rs`、`osc.rs`、`render_frame.rs` |
| 矩形操作和保护 | DECERA/DECSERA/DECFRA/DECCRA、矩形属性、DECIC/DECDC、选择性擦除 | `lib.rs::dispatch_intermediate_csi` |
| 字符集基础 | G0–G3、GL/GR 寄存器、SS2/SS3、DEC 绘图、Latin-1 和 12 类西欧 NRC；实际输出映射仍有边界 | `charset.rs` |
| 输入编码 | 语义键盘、Kitty keyboard、modifyOtherKeys、Win32 input、鼠标、焦点、bracketed paste | `input.rs`、`modes.rs` |
| 常见查询 | DA1/DA2、基本 DSR/CPR、DECRQM、XTVERSION、DECRQSS、限定能力集合的 XTGETTCAP | `lib.rs`、`query_reply.rs`、`host_effect.rs` |
| 常见 OSC | 0/1/2 标题、4/104 调色板、10/11/12 及重置、OSC 8 超链接、标题栈 | `osc.rs` |
| 历史与恢复基础 | 有界 scrollback、soft-wrap reflow、行 ID/revision、Snapshot V2.10 历史持久化、Render Frame V3 | `screen.rs`、`snapshot.rs`、`render_frame.rs` |
| 同步输出 | 2026 状态、逻辑时钟超时及 presentation gate | `lib.rs` |

上表文件均相对 `crates/termlink-terminal-core/src/`。例如 XTGETTCAP `Co` 已回复 256 色，未知的 `TN`
请求返回协议失败回复；这属于“实现了明确子集”，不能写成整个查询通道缺失。

## 4. 已确认差距：核心 14 组

“部分”表示已有相关基础但缺具体行为；“缺失”表示列出的能力没有实现。优先级 P0 最高。

| ID | 能力组 | 状态 | 优先级 | 当前差距与 Windows Terminal 对照 | 实施边界/完成标准 |
|---|---|---|---|---|---|
| C01 | 窗口几何查询 | 部分 | P0 | core 的 `14t` 仅消费，`16t/18t` Unsupported；WT 已回复三者 [W1] | 18t 使用权威行列；像素几何先定义一致语义或精确无回复回退；分块及回放一致 |
| C02 | 可选指令错误处理 | 部分 | P0 | 未知 DECRQM 可回复“不识别”，但若干只读查询/可选模式令整个 core FailedClosed；WT 有 UnknownSequence 分派路径并继续处理 [W2] | 区分可选探测、必要状态和损坏数据；不能取消序号/快照/边界保护，也不能继续使用已不可信网格 |
| C03 | 完整字素分段和宽度策略 | 部分 | P1 | core 是小型宽度表及 combining/ZWJ 规则；WT 有 Unicode 16/UAX #29 分段及多种测量模式 [W3] | 国旗、复杂文字、ZWJ、VS、分块/resize/历史重放一致；更新 width profile 和快照契约；2027 声明与实际行为一致 |
| C04 | 双宽/双高行与对齐图案 | 缺失 | P2 | `ESC #3/#4/#5/#6/#8` 未分派；WT 有行 rendition 和 alignment pattern [W4] | 补行级状态、坐标/裁剪规则、快照及 Android renderer；不只消费 escape |
| C05 | 补充编辑与软复位 | 部分 | P1 | 基础编辑存在，`ESC 6/9`、`CSI ?5W`、`CSI !p` 均 Unsupported；WT 实现 back/forward index、tab reset、DECSTR [W4] | 明确 soft reset 与 RIS 的不同，保留应保留的屏幕/历史，验证 margin/wrap/tab/输入模式恢复 |
| C06 | SGR 属性栈 | 缺失 | P2 | `CSI #{/#}` 及 `#p/#q` 别名无实现；WT 有 push/pop graphics rendition [W5] | 有界栈、空栈/溢出规则、选择性属性、Snapshot 和 reset 语义 |
| C07 | 扩展识别/状态报告 | 部分 | P2 | DA3、终端参数、扩展 private DSR、显示范围、矩形 checksum 缺失；基本 DA/DSR 已有；WT 有对应实现 [W4] | 只报告真实能力，明确支持的 selector/参数，复用有序 PTY 回复；不伪装硬件功能 |
| C08 | DEC 状态报告与恢复 | 缺失 | P2 | `CSI $u/$w`、DCS `$p/$t` 未实现；WT 支持特定颜色表、光标及 tab 状态报告/恢复 [W4] | 有界、原子恢复；与本项目 Snapshot 分开，不能把私有 Snapshot 当成 DEC 协议回复 |
| C09 | 终端多页与页尺寸 | 缺失 | P3 | core 只有主/备用屏幕，DECCRA 限 page 1；WT 有 NP/PP/PPA/PPR/PPB、页列设置 [W4] | 有界 page store、active/visible page、跨页复制和恢复；不是 UI 标签页 |
| C10 | 旧终端仿真及扩展字符集 | 部分 | P3 | core 有有限 NRC 和显式 standalone-C1 profile；缺 VT52、C1 切换、UPSS 及更多字符集映射；WT 有这些路径 [W4][W6] | 保持默认 UTF-8，不自动把任意高位字节解释成 C1；按明确 profile 开启 |
| C11 | DEC 宏 | 缺失 | P3 | DCS `!z` / CSI `*z` 未实现；WT 有 DefineMacro/InvokeMacro [W4] | 定义存储、大小/调用次数/递归预算、快照和输入拒绝规则 |
| C12 | DRCS 下载字形 | 缺失 | P3 | DCS `{` 当前 fail closed；WT 有 DownloadDRCS [W4] | glyph bank、字符集引用、Snapshot、资源帧与真实绘制闭环 |
| C13 | SIXEL 图像 | 缺失 | P3 | DCS `q` 当前 fail closed；WT 有 raster decoder 和图像呈现路径 [W7] | 图像/placement、滚动、颜色、内存上限、恢复、双端绘制；像素定义与 C01 一致 |
| C14 | 标题/颜色扩展 | 部分 | P2 | OSC 21、17/117 静默无效果，DECAC 未实现；WT 有标题别名、选区颜色及 DEC 颜色分派 [W2][W4] | 复用标题/颜色模型，新增选区颜色须平台消费；逐 selector 验证实际效果 |

本地定位：C01/C02/C04–C14 主要是 `lib.rs` 的 dispatch 和 `osc.rs` selector；C03 在 `unicode_width.rs`
与 `screen.rs`；C10 还涉及 `charset.rs`、`parser.rs`。这些缺口不能通过重新打包相同 DLL 消除。

## 5. 已确认差距：跨 core/平台 4 组

这里统计的是 **PTY 输出协议经 core 到宿主的闭环**。不表示 TermLink 完全没有文件浏览、复制、通知或滚轮 UI。
Windows 外层终端可能已经自行执行部分 OSC；这不能等同于 Android 和回放链路也支持。

| ID | 能力组 | 状态 | 优先级 | 当前差距与 Windows Terminal 对照 | 实施边界/完成标准 |
|---|---|---|---|---|---|
| H01 | 工作目录及 shell 命令标记 | 缺失 | P1 | OSC 7、133 在 core 被忽略；WT 有 cwd、prompt/command/output/end 标记 [W8] | core 输出结构化状态/事件，Windows 验证远端路径归属，Android 可打开当前目录/按命令定位；不能仅用创建会话时的 cwd 代替实时 cwd |
| H02 | 终端铃声与任务进度 | 缺失 | P2 | BEL 无效果、OSC 9 进度被忽略、DECPS Unsupported；Host Effect V2 只有 PTY_REPLY；WT 有 bell、taskbar progress、sound 分派 [W4][W8] | 用受限通知/进度事件交给平台，频率限制、静音和回放不重复提示；声音是可选平台能力 |
| H03 | OSC 52 剪贴板写入 | 缺失 | P2 | core 静默消费，没有 clipboard effect；WT 解析写入并调用宿主，查询被排除 [W2] | 编码/长度验证、活动客户端/授权边界、一次性执行；不把本地任意剪贴板读取开放给远端 |
| H04 | Alternate-scroll 模式协商 | 部分 | P2 | `?1007h/l` 目前显式 no-op，DECRQM 返回 0；WT 保存该模式并供滚轮输入使用 [W4] | 建立可恢复模式和备用屏幕下滚轮到按键的策略，结合现有鼠标报告/历史滚动优先级；不能擅自改变非鼠标模式下手机原有单指操作 |

H04 的不支持声明目前是诚实的，属于能力待补，不能因此把先前的 1007 兼容消费修复判为无效。
它与 Android 已有右侧/左侧滚轮控件、触摸作为鼠标开关也不是同一个功能。

## 6. 关键实证

### 6.1 Claude 启动失败及窗口查询族

已诊断会话的第 34 条 Output 同时含 `?2026$p`、`16t`、`?1016$p`、DA1。
失败点是 `16t`；Windows authority 失败后 SessionHost 终止进程树，退出码 125；Android 回放同一条输出报 TL-H122。
仅在内存回放副本去掉 `16t` 后，原 36 条事件全部可应用。没有修改原始历史。

| 输入 | 当前 core | Windows Terminal 固定源码行为 |
|---|---|---|
| `CSI 14 t` | OK，无回复 | `CSI 4;高度;宽度 t`，使用虚拟图形像素 |
| `CSI 14;2 t` | Unsupported | 进入 14 分支；当前未区分 `2` 扩展的几何语义 |
| `CSI 15 t` | Unsupported | WindowManipulation 的 default 分支无动作；未实现完整屏幕像素查询 |
| `CSI 16 t` | Unsupported | `CSI 6;高度;宽度 t`，虚拟 cell 尺寸 |
| `CSI 18 t` | Unsupported | `CSI 8;行;列 t` |
| `CSI 19 t` | Unsupported | default 分支无动作；未实现完整屏幕字符尺寸查询 |
| `CSI ?2026 $p`、`CSI ?1016 $p` | OK，默认分别返回状态 2 | 不应与 16t 一并当作故障输入过滤 |
| `CSI ?9999 $p` | OK，返回“不识别”状态 0 | 证明 TermLink 已有安全否定回复的实现范例 |

WT 官方测试的虚拟 cell 为 **10×20**，用于 VT240/VT340/SIXEL 兼容 [W1-test]。
这不是手机字号/DPI，也不一定是 Windows 字体实际像素。直接硬编码该值并宣称“按手机实际窗口尺寸”不正确。
`15t/19t` 也不是“WT 已完整支持而 TermLink 未支持”，本表仅记录两者的具体行为。

### 6.2 Unicode 和能力声明

- 合成 ASCII `abc`：光标列 3；`中文`：列 4；`e` + U+0301：列 1。
- 国旗 U+1F1E8 U+1F1F3：core 列 4；先发送 `?2027h` 后仍为列 4，且查询返回 `?2027;1$y`。
- WT 的 Graphemes 模式按 UAX #29 将 RI 对组合，宽度上限为 2；其 Wcswidth/Console 模式另有行为。
  本轮是源码对照，不声称已在所有 WT 配置实机测得同样结果。
- TermLink 当前 `grapheme_clustering` 只设置、查询和持久化，打印路径没有据此实现完整分段。
  不能把“2027 可设置”当作完整字素能力。修复须同步终端声明、布局、光标、选区、复制、历史与快照版本。

### 6.3 成功状态不代表能力完整

`OSC 2;synthetic-title` 导出标题正确；`OSC 21;synthetic-title` 返回 OK 但标题仍为空。
OSC 7/133/9/52、BEL 同样可被接受而没有对应 Host Effect；源码确认 V2 effect kind 只有 PTY_REPLY。
补功能时必须验证效果及回放行为，不能只断言 native 返回 OK。

## 7. 单独登记的工程任务，不计入 18 组

| ID | 待办 | 原因和约束 |
|---|---|---|
| E01 / P0 | Windows 外层终端查询回复归属、过滤与去重 | 应用 `OuterTerminalReplySuppressor` 只覆盖 14t 几何回复；探针中 `CSI 4;20;10t` 剩余输入 0 字节，而 16t 的 `CSI 6;20;10t` 剩余 10 字节。放行 16t 后若不同步处理，会产生自动回复混入人工输入/重复回复风险。不是 WT portable core 的同类缺口 |
| E02 / P2 | 公共 History Page API | 内部 scrollback 和 V2.10 已有；缺 immutable capture、分页 token、稳定 anchor、过期/截断语义。TermLink 远程恢复需求，不能按 WT UI 滚动条作等价比较 |
| E03 / P2 | 公共 Delta API | 缺 dirty journal、base/target revision、资源 tombstone 和 gap/resync。全量快照和现有远程事件链不等于 core Delta |
| E04 / P1 | 2031 主题报告能力声明复核 | core 记录并报告 `color_scheme_reporting`，没有主题改变事件/完整报告通道；应实现真实语义或收紧支持声明。本次 WT 基线未找到 2031 实现，不计作 WT 已有优势 |

Kitty graphics 继续保留在现有缺口清单；本轮没有确认该 WT 基线完整支持此协议，故不计入比较差距。
双向文字、复杂字体 shaping、ligature、GPU/DPI、无障碍等仍需独立审查 View/renderer，不能仅凭 core 缺字段
就判定整个产品缺失，亦不计入本次数量。WT 的 OSC 633 部分行为受实验功能开关约束，不能概括为全量支持。

## 8. 推荐实施次序和验收

1. **Q0：C01/C02 + E01。** 先建立尺寸查询支持/回退矩阵，改正可选查询错误边界，同步 Windows 回复归属。
   不在 Android、某个按钮或某个 agent 路径过滤特殊字符串。序号、坏快照和必要状态错误仍严格处理。
2. **常用文本兼容：C03/C05 + E04。** 完整字素策略、DECSTR、明确模式声明；逐项更新稳定状态/快照契约。
   旧 width profile 不应在恢复旧历史时被静默换成另一套布局规则。
3. **跨端交互：H01，然后 H02/H03/H04。** 复用现有文件、输入和通知 UI，扩展有版本的 Host Effect/状态契约；
   实时宿主执行与镜像/历史回放分开，禁止复制、铃声、通知因重连回放重复触发。
4. **按需求扩展：C04/C06–C14。** 先补有实际程序需求的控制族，再考虑宏、多页、旧字符集及图形。
   DRCS/SIXEL 需要资源、快照、内存预算及双端绘制，不适合以“识别后忽略”冒充实现。
5. **自有 API：E02/E03。** 与以上工作分开验收，按历史规模和性能需要推进；不是 Claude 故障修复前置条件。

每组完成至少覆盖：正常/默认/非法参数、逐字节分块、同帧组合、后续普通输出、快照恢复、旧协议字节稳定性，
以及适用的 Host Effect 次序/去重、绑定与渲染验证。新增状态明确 C ABI/FFI/JNI/.NET 兼容边界。
Q0 还必须实际验证手机新建 PowerShell → Claude 启动/输入/退出 → shell 仍可用，以及断线后的后缀回放。
当前仅验收 Relay Only；本次文档登记不等于这些验收已完成。

本轮只参考 Windows Terminal 的公开行为和源码定位，未向产品复制上游实现/测试向量。
上游为 MIT；后续若复制实现，需另行保留对应许可证和署名，并适配 TermLink 的 ConPTY、权威事件、
控制租约、E2EE、Snapshot 和跨端回复边界。

## 9. 官方参考索引

- [W1：窗口查询实现][W1]；[窗口查询官方测试][W1-test]。
- [W2：输出 CSI/DCS/OSC 分派][W2]。
- [W3：Unicode 16 与 UAX #29 字素测量][W3]；[分段测试][W3-test]。
- [W4：终端状态、页、报告、DRCS、宏及宿主调用][W4]；[指令 ID][W4-ids]。
- [W5：SGR 属性栈实现][W5]。
- [W6：字符集映射][W6]。
- [W7：SIXEL parser][W7]。
- [W8：工作目录、任务进度与 shell 标记][W8]。
- [xterm 控制序列说明][xterm]；[Windows Terminal MIT 许可证][wt-license]。

[wt-revision]: https://github.com/microsoft/terminal/commit/6676c6f938f7e98ad680f0cecedd3bafb6529734
[W1]: https://github.com/microsoft/terminal/blob/6676c6f938f7e98ad680f0cecedd3bafb6529734/src/terminal/adapter/adaptDispatch.cpp#L3482
[W1-test]: https://github.com/microsoft/terminal/blob/6676c6f938f7e98ad680f0cecedd3bafb6529734/src/terminal/adapter/ut_adapter/adapterTest.cpp#L3869
[W2]: https://github.com/microsoft/terminal/blob/6676c6f938f7e98ad680f0cecedd3bafb6529734/src/terminal/parser/OutputStateMachineEngine.cpp#L691
[W3]: https://github.com/microsoft/terminal/blob/6676c6f938f7e98ad680f0cecedd3bafb6529734/src/types/CodepointWidthDetector.cpp#L803
[W3-test]: https://github.com/microsoft/terminal/blob/6676c6f938f7e98ad680f0cecedd3bafb6529734/src/types/ut_types/CodepointWidthDetectorTests.cpp#L276
[W4]: https://github.com/microsoft/terminal/blob/6676c6f938f7e98ad680f0cecedd3bafb6529734/src/terminal/adapter/adaptDispatch.cpp
[W4-ids]: https://github.com/microsoft/terminal/blob/6676c6f938f7e98ad680f0cecedd3bafb6529734/src/terminal/parser/OutputStateMachineEngine.hpp#L58
[W5]: https://github.com/microsoft/terminal/blob/6676c6f938f7e98ad680f0cecedd3bafb6529734/src/terminal/adapter/adaptDispatchGraphics.cpp#L464
[W6]: https://github.com/microsoft/terminal/blob/6676c6f938f7e98ad680f0cecedd3bafb6529734/src/terminal/adapter/terminalOutput.cpp#L246
[W7]: https://github.com/microsoft/terminal/blob/6676c6f938f7e98ad680f0cecedd3bafb6529734/src/terminal/adapter/SixelParser.cpp
[W8]: https://github.com/microsoft/terminal/blob/6676c6f938f7e98ad680f0cecedd3bafb6529734/src/terminal/adapter/adaptDispatch.cpp#L3556
[xterm]: https://invisible-island.net/xterm/ctlseqs/ctlseqs.html
[wt-license]: https://github.com/microsoft/terminal/blob/6676c6f938f7e98ad680f0cecedd3bafb6529734/LICENSE
