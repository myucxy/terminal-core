# S7 全状态 Delta 设计提案（2026-10-10，未实现）

本文是针对当前代码的只读核查结论及供主维护者决策的设计提案，**不代表 Delta 已实现、S7 已完成、产品 profile 已切换或具备发布资格**。本文没有对应的新增实现、构建、性能测量或真实 Relay/设备验收结果。

依据为 [总实施计划](FULL_CAPABILITY_IMPLEMENTATION_PLAN_20261009.md) 的 S7/E03 与 T2，以及 [图形资源契约](GRAPHICS_RESOURCE_CONTRACT_20261010.md)。核查时已有实验 Snapshot 2.22、Render V5、History V2；旧 C ABI `tl_terminal_checkpoint_export_delta(checkpoint, out_delta)` 仍固定返回 `UNSUPPORTED`。产品保持既有 `2.21_v2` 身份。

## 1. 范围与可恢复状态

S7 要求的是合法 checkpoint 边界上完整终端状态的增量，必须满足：

```text
B = base.next_source_sequence
T = target.next_source_sequence
restore(snapshot@B) + apply(delta[B,T)) == restore(snapshot@T)
export_snapshot(apply(snapshot@B, delta[B,T))) == canonical_snapshot@T
```

第二个等式要求完整 canonical Snapshot 字节相等，再通过后缀事件继续运行检查恢复后的行为。网格相等或 Render V5 相等不能替代此要求。把原始 Output/Resize/Advance 等事件再封装成包只是事件回放；把完整 Snapshot 放入 Delta fallback 字段也不能算增量实现。

完整状态包括主/副屏及保留历史、DEC pages、光标及保存光标、行属性、modes、tabs、margins、字符集及保存字符集、REP source、SGR stack、宏、keyboard 状态、OSC/颜色/标题/hyperlink、宿主 geometry/theme、shell 标记、宿主 signals，以及 DRCS/图像/placement 的内容、引用和分配高水位。即时 OS/PTY 副作用不是恢复时重新执行的动作；它们沿用既有 live/replay、durable 去重和 ACK 规则。

`snapshot::export` 和 `Terminal::ensure_presentation_eligible` 当前拒绝 failed-closed、parser 未完成及 synchronized output 未结束的状态。Kitty 未完成上传也不具备 checkpoint 资格。此提案沿用门禁，不保存半个 DCS/APC/UTF-8，不把诊断、解析临时态或运行时缓存宣称为持久状态。

## 2. 已核实的编码性质

### 2.1 Snapshot section 可作为完整差分边界

实现位置为 `crates/termlink-terminal-core/src/snapshot.rs`：

- Snapshot 头为 104 bytes；目录项为 32 bytes，含 tag、flags、section version、offset、length、logical count。
- section 按 tag 严格递增，连续布局并按 8 bytes 对齐；reserved/padding 必须为零。import 拒绝未知必需能力、不匹配 section 版本及不规范目录。
- 21 类已知 section 覆盖上述可恢复状态，包括 `0x500 GRAPHICS_STATE`。实际 section 集合和格式版本由状态选择，不能假定所有旧快照都固定有 21 项。
- `collect_graphemes` / `collect_renditions` 每次排序去重并重新编号。一个表项的增删就可能改写大量未编辑 cell 的 ID。
- `encode_rows` 使用可变长 RLE，并携带 logical line、cell offset、line rendition、sealed revision 等。新增一个 run 或淘汰前部历史行可能移动后续字节。

因此，同 section、同固定偏移的 block patch 在正确性上可表达全状态；section 边界还能避免前部 section 长度变化平移整个 Snapshot。但它不保证“少量文本编辑只传少量字节”，表项重编号和 RLE 位移可能使 rows patch 接近完整 rows section。不能据此宣称 CPU 或网络代价与 dirty cells 数成正比。

### 2.2 图形必须增加资源边界

`graphics_state/codec.rs` 中 `TLGS1` 顺序为：

```text
header 64
TLGRS1 resource frame
TLDRCS1 font bank 192
SIXEL palette 768
Kitty identity entries: 24 × count
placements: 168 × count
```

`graphics_resources.rs` 中 `TLGRS1` 为 48-byte 头，随后按 ID 递增保存 `32-byte resource header + raw pixels + 4-byte zero alignment`。删除前部资源会移动后面所有像素；直接对整个 `0x500` 按同 offset 分块，会重传最高接近 32 MiB 的未变化像素。

最小实用方案应当把 `0x500` 内的资源按内部 resource ID 单独比较；其余 TLGS1 元数据可继续分块。无需因此变更 Snapshot 2.22、TLGRS1 或 TLGS1 的既有编码。完全不拆资源的通用 block diff 可以作为正确性原型，但不足以证明资源 Delta 的性能要求。

## 3. 建议的 Delta V1 结构

下述是候选契约；magic、结构大小、数值、状态码、限额及具体 ABI 尚未冻结。

包头包含：格式版本、required feature bits、不可变 session/stream context 和状态 profile、base/target exclusive `next_source_sequence`、base/target canonical Snapshot 长度与强摘要，以及整个 Delta 的强摘要。context 必须区分会话与恢复分支；相同序列号不能单独证明来自同一状态。

普通 section 记录携带目标 tag/version/flags/logical count/length，以及按偏移排序、不重叠的固定块替换：未列出的块从已验证 base 继承；新增 section 的字节必须全部给出；删除 section 通过显式 tombstone 表达。目标 section 长度决定截断，不能保留 base 尾部。目标头与目录是有界元数据，可依据目标描述重新生成 offset、alignment、CRC；不得信任不受检查的任意 offset 写入。

建议初版考虑 4 KiB block，相邻 dirty blocks 在 wire 上合并成连续记录。块大小仍需真实样本验证，不作为本文已测量结论。

对 graphics section：

1. TLGS1 header、DRCS、palette、Kitty directory、placements 按各自元数据边界差分，保留完整目标计数和高水位。
2. TLGRS1 resources 按内部 ID 表达 Keep/Add/Delete。Keep 仅引用已验证 base 中的同一不可变资源；Add 携带 metadata 与 pixels；Delete 携带资源 ID tombstone。
3. 重建时按目标 ID 顺序产生 canonical TLGRS1/TLGS1，重新计算已有 CRC，再进入完整 Snapshot 的强摘要与 import 检查。
4. 所有 section、resource 操作必须唯一、有序、数量受限。拒绝重复 ID、重叠块、越界长度、非零 reserved、未知 required feature，以及不能从 base 或本包解析的引用。

这样只改文本不会重传像素，删除图 A 不会重传未变图 B，替换图像仅发送新不可变资源。文本表项重编号造成的放大仍是已知限制，后续是否引入稳定文本表或按逻辑行差分需另行决定。

如果编码后的 Delta 不小于完整目标 Snapshot，导出应返回明确的 full-resync/无收益结果，由调用方显式走 Snapshot 传输。不存在把完整 Snapshot 套进 Delta 后记为增量成功的分支。

## 4. 资源身份、generation 与 tombstone

现有代码已提供可复用的稳定身份：

- `GraphicsResources::insert` 从非零 `next_id` 单调分配，并检查溢出。资源像素由 `Arc<ImageData>` 不可变共享。
- `remove`、`clear` 不回退 `next_id`。`GraphicsState::reset` 保留内部资源和 placement 分配高水位，不复用旧身份。
- DRCS 的 `clear` 保留 `next_generation`；替换 atlas 创建新资源；font generation 及其下一代计数已进入 Snapshot。
- Kitty 的外部 image/placement ID 可被协议用户复用，不能用作资源 Delta 的唯一身份。应使用内部 `ImageResource.id` 与内部 `Placement.id`，并保留协议 ID 映射。

在同一 context 内，base 与 target 都存在的 resource ID 必须对应完全相同的尺寸、格式、像素。若不一致应判为错基线或坏数据，禁止解释成原地更新。资源替换为 Add 新 ID，更新引用，必要时 Delete 旧 ID；仍被旧 cell/字体/placement 引用的 atlas 不可提前删除。

tombstone 是 base 活资源集合减 target 活资源集合。base 到 target 之间创建又删除、且 base 从未见过的资源无需传 pixels，但目标 `next_id` 高水位仍需更新，保证不复用身份。placement 与 section 的删除同样必须在目标元数据中明确反映。

所有 Add/Delete 都属于待安装 Delta，不能让迟到的资源消息直接修改 live resource store。只有整个目标状态通过检查才一次安装；旧包按 context/base/target/hash 拒绝。这样删除不会被迟到 Add 复活，也无需在 live store 无界保留所有历史 tombstone。journal 仍需按其承诺的保留区间保留足够的删除信息。

删除只解除 live store 引用，冻结 checkpoint/history 的 Arc 保持有效；最后一个 Arc 释放才归还已有进程像素预算。

## 5. 序列、强校验与原子安装

当前 `TlTerminalCheckpoint.revision == next_source_sequence`。事件以恰等于 next 的 source sequence 提交，每次成功后加一，因此 Delta `[B,T)` 替代 source events `B..T-1`；安装后的第一条原始事件是 `T`。不将 inclusive 最后事件序列与 exclusive next 混用，也不在安装后再次应用这段原始输出。

建议处理顺序：

1. 检查完整 Delta 的结构、版本、required features、context/profile、所有长度/计数/乘法/排序/重叠及整体预算；检查 Delta 自身强摘要。
2. 当前 checkpoint 若已是 `(context,T,target_hash)`，返回成功及 `ALREADY_APPLIED`，不产生第二次安装或副作用。不能因为 target revision 相等就跳过包完整性检查。
3. 否则当前必须恰为 `(context,B,base_hash)`；拒绝错误基线、乱序或分叉。base hash 覆盖整个 canonical Snapshot。
4. 在 detached buffer 中组装目标 canonical Snapshot，验证长度和 target 强摘要，再调用现有 `Terminal::import_snapshot_v2`。
5. import 完成后才发布 detached Terminal/交换 live handle，并推进对应持久状态及 ACK。任一步失败均保留原 live 状态及原 ACK。

`snapshot::import` 已有 `preflight_a → preflight_b → decode_terminal`。图形解码还会在像素物化前验证资源 metadata、font/placement/glyph references 及 anchors，应复用这些检查，不能为 Delta 写另一套宽松安装器。

core 的 detached 构造与宿主 WAL、持久 ACK、UI handle 切换不是同一原子操作；必须接入既有 detached snapshot 安装事务，使崩溃恢复能识别提交前后状态。此文不声称一个 native pointer swap 已解决跨进程持久化原子性。

强摘要建议为 SHA-256。base/target 摘要直接覆盖完整 canonical Snapshot bytes，便于外部复核；Delta 自身摘要应带固定 domain 标识，并覆盖 context/profile、头、操作和 payload，排除或规范置零自身摘要槽。具体字节契约必须冻结并有跨语言向量。SHA-256 不替代消息认证，E2EE 继续承担认证职责。

安装不重发原始 Output 的 PTY 回复或 OS 动作。host signals 的持久状态由 Snapshot 承载；消费端继续使用既有持久去重规则。

## 6. 固定 checkpoint journal 的承诺边界

最小方案是记录**实际捕获的合法 checkpoint revisions**，而不是承诺可从任意 source event sequence 导出 Delta。

- 首个 checkpoint 形成独立 canonical baseline；后续捕获比较 section/resource blocks，记录 dirty blocks、资源 Add/Delete/tombstone 和目标元数据。未变块与不可变资源可通过 Arc 共享。
- 只承诺从仍保留的 `(revision,hash)` 到仍保留的目标 checkpoint 导出。非 quiescent event 边界不捕获；两个合法 checkpoint 间的 Delta 仍表示期间全部持久状态改变。
- 原始 event replay 不构成此 journal。所有中间事件如何改变状态由 canonical target 确定，journal 存的是状态差异或足以重建该差异的 checkpoint block 集合。
- 同时限制保留字节、checkpoint 数和保留时间。字节预算计入 baseline、dirty data、资源引用所保留的 payload、索引和元数据；共享内存按实现明确计费，不把有引用的像素当作已释放。
- 淘汰 base 时必须处理依赖链，或把最老剩余 checkpoint 物化为独立 baseline；不能留下无法重建但仍可查询的 revision。
- journal 丢失、base 已淘汰、hash 不匹配、source Gap/failed-closed、重新 import 导致来源分支变化时，返回明确 full-resync 路径。
- journal 缺基线是同步层 Gap 结果，不能调用 `Event::Gap` 毒化本来健康的终端。现有 `Event::Gap` 会 latch failed-closed，语义不同。

当前 FFI checkpoint 不带来源 identity。可在内部复制 `TlTerminal.history_owner` 作为本机 lineage 标记，防止把新 import handle 的 checkpoint 接到旧 journal；跨网络 context 仍需来自不可变会话/stream profile，不能传输本机 owner 当全局会话 ID。

如果最终要求自动记录每次成功 commit，而非上述显式 checkpoint capture API，则应覆盖 `apply_events_atomic` 和两种 `PreparedApplyWithHostEffects::commit_with` 的成功提交点。prepare 阶段、host-effect publish 失败及回滚都不能追加 journal。只给一个 FFI apply 函数加 hook 会遗漏其他提交路径。该扩展不属于本文最小固定 checkpoint journal 的承诺。

## 7. 候选 API 与实现文件

旧 `tl_terminal_checkpoint_export_delta(checkpoint,out)` 没有 base 参数，继续保留明确的 `UNSUPPORTED` 语义。新能力使用显式版本 API；以下名称和签名是提案，不是当前已导出符号：

```c
tl_terminal_delta_journal_create_v1(config, out_journal);
tl_terminal_delta_journal_capture_v1(journal, checkpoint, out_state_info);
tl_terminal_delta_journal_get_info_v1(journal, out_retention_info);
tl_terminal_delta_journal_export_v1(
    journal, base_revision, base_sha256, target_revision, options, out_delta);
tl_terminal_delta_journal_destroy(journal);

tl_terminal_checkpoint_apply_delta_v1(
    current_checkpoint, expected_context, delta, delta_len,
    out_terminal, out_apply_info);
```

`out_terminal` 沿用必须为 null 的 ownership slot，返回 detached handle；`out_apply_info` 包含目标 revision/hash 和 `APPLIED` / `ALREADY_APPLIED`，幂等结果是否返回 handle 需在最终 ABI 中明确，不能由调用方猜测。journal mutation 要求独占；checkpoint 保持既有只读并发契约。版本化结构的 size/reserved 和状态码按现有 ABI 规范固定。

建议实现范围：

| 文件 | 最小责任 |
|---|---|
| 新 `crates/termlink-terminal-core/src/delta.rs` | wire codec、全状态差分、target 重建、强摘要校验 |
| 新 `crates/termlink-terminal-core/src/delta_journal.rs` | checkpoint block retention、dirty records、淘汰/Gap |
| `crates/termlink-terminal-core/src/snapshot.rs` | 暴露同 crate 内的已验证 section view 与必要的 canonical reassembly 接口 |
| `crates/termlink-terminal-core/src/graphics_resources.rs` | 借用 resource record view；可后续支持验证后 Arc 复用 |
| `crates/termlink-terminal-core/src/graphics_state/codec.rs` | 暴露 TLGS1 子对象边界，不改变既有编码 |
| `crates/termlink-terminal-core/src/lib.rs` | 导出新 core API/error；选择自动 journal 时才涉及全部 commit 点 |
| 新 `crates/termlink-terminal-ffi/src/delta.rs` | opaque journal ownership、ABI 参数和 buffer 处理 |
| `crates/termlink-terminal-ffi/src/lib.rs`、`include/termlink_terminal.h` | 注册/公开显式版本 API |
| `android/jni/termlink_terminal_jni.cpp` | 新 C ABI 的 JNI 桥接 |
| core Cargo manifest/lock 与第三方说明 | 固定强摘要依赖及可复现供应材料 |

测试至少覆盖：所有已引入状态的 Snapshot 字节相等、后缀继续运行、resource Add/Delete/替换/reset、旧 glyph 仍引用 atlas、错 context/base/hash/feature、截断/溢出/重叠、重复幂等、迟到包、journal 淘汰、内存拒绝及安装失败不改变原状态。跨语言适配须检查结构大小、所有权和状态结果。

## 8. 强摘要依赖与离线供应

核查时 core/ffi `Cargo.lock` 只有两个 workspace crate，没有第三方摘要依赖。`crates/termlink-terminal-core/src/crc32c.rs` 是逐位 CRC32C，用于现有格式损坏检测，不能作为强基线身份。

建议选择 RustCrypto `sha2` 的固定版本并提交 lock、校验来源及相应许可证/供应材料；本文不替主维护者猜定版本，也没有下载或安装依赖。现有 `scripts/Build-ToTermLink.ps1` 使用 `--locked --offline`，五平台构建前必须准备固定的依赖缓存或受控供应目录，确认 GPL 分发所需材料随 core 构建资料提供。不能在应用仓增加随构建联网取依赖的步骤。

Windows PowerShell `Get-FileHash` 目前仅用于产物校验，不能作为 Rust/native 五平台 Delta 的运行时实现。优先复用维护成熟的摘要库，不为保留零依赖而自行新增未经验证的密码学实现。

## 9. 性能、进程内存与协议成本

以下为代码结构推导，**不是 benchmark 结果**。

### 9.1 CPU 与传输

- 现有 Snapshot 导出仍收集/排序所有 cells 的 grapheme/rendition，约 `O(C log C)`；完整编码、SHA-256 和 block 比较约 `O(S)`。最小方案并不变成 `O(dirty bytes)` CPU。
- 图形嵌套 TLGRS1/TLGS1/Snapshot CRC 会重复扫描 pixels；现有 CRC32C 逐位实现的成本也应测量。不能把全图摘要扫描当作零成本。
- 4 KiB blocks 下，128 MiB Snapshot 最多 32,768 块；若每块存 32-byte hash，仅摘要约 1 MiB，另有索引/Arc/长度等开销。
- 资源分离后未变 pixels 不上 wire，但普通文本表重编号和 rows RLE 位移仍可能使 Delta 很大。无收益时显式 full resync。

### 9.2 进程内存

- 最简单实现同时持有 base bytes、Delta bytes、重建 target bytes，暂存峰值约 `B + D + T`。若都接近 128 MiB，保守估计接近 384 MiB，另加 live/detached Terminal、Snapshot 编码临时数组、checkpoint/journal/history 持有的内存。
- 现有 canonical graphics Snapshot 的静态保守界低于 128 MiB，但不能因此忽略大 buffer 和多 checkpoint 的总量。现有图形像素预算不自动涵盖普通 `Vec<u8>` 中重复编码的 pixels。
- 当前 import 对目标全部图片重新物化。即使 Delta 没传未变 pixels，base 与 detached target 同时存在仍可能多占最多 32 MiB 像素，并受现有 256 MiB process image budget 限制。预算不足必须原子失败。
- 可在后续优化 `ResourceFrame::materialize`，在 metadata、ID、digest 均匹配后复用旧 Arc，降低 target 像素复制峰值；不能只凭 ID 复用。
- journal 需独立按总保留 bytes admission，不能复用“单个 Snapshot 最大128MiB”作为总内存上限。Android ARM32/ARM64、多会话、冻结历史与重连并发应有峰值测量与拒绝路径验证。

### 9.3 传输及持久恢复

core 的 128 MiB Snapshot limit 不代表 RPC、E2EE、WebSocket/Relay、WAL 或客户端已能安全处理同尺寸 Delta。最终需要固定每包/每片/每次恢复总量、待安装并发数、超时、取消和重试预算。

若使用分片，片段必须绑定 context、base/target hash、总长和位置，按完整包组装验证后安装；不得逐片推进 terminal revision 或 durable ACK。Server 只路由有界不透明消息，不解释终端 section/resource 内容。消息迁移保持向后兼容，验收仅安排现有 Relay Only 路径。

## 10. 产品兼容与完成门禁

新 state profile 必须显式协商并固定在新会话元数据中。不得因安装新 DLL/SO 或收到 Delta 就把旧 `2.21_v2` 会话、旧 WAL、旧 Snapshot 静默升级成图形/Delta profile。仅支持新 profile 的状态不能通过删字段导出到旧协议。

主应用仓只实现 C#/Kotlin/RPC/持久安装的消费者适配，继续只消费五个受 lock 固定的 native 库。Rust/C ABI/JNI/CMake/编译脚本留在 core 仓，由 `scripts/Build-ToTermLink.ps1` 统一导出一 Windows DLL 与四 Android ABI SO，并最后写 lock；不能重新把 core 源码纳入主应用构建。

本提案只有在实际实现、全状态与故障向量通过、五库及适配器一致、RPC/crypto/WAL 预算闭环、真实 Relay 恢复与 Android 设备内存/资源表现完成后，才可作为 S7 完成证据的一部分。单元测试或构建五 ABI 不能替代真实设备验收。发布授权和发布动作仍单独处理。
