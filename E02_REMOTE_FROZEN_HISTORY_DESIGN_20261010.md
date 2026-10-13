# E02 远端冻结历史分页 RPC 设计提案

日期：2026-10-10。状态：**设计提案，尚未实现，也未通过产品验收**。

本文件记录对当前 core/app 工作树的只读设计复核，不代表新增协议、加密实现、产品浏览入口或远端验收已经完成。
已有 immutable History V1/V2、FFI、Windows/Android decoder 和本地冻结滚动是实施基础；
本提案补齐的是 Windows 持有冻结捕获、通过 Relay 向 Android 分页交付的产品链路。
相关基础见 [S5 实施记录](S5_IMMUTABLE_HISTORY_20261010.md) 和
[完整能力实施计划](FULL_CAPABILITY_IMPLEMENTATION_PLAN_20261009.md)。

## 1. 最小完整方案与现有语义

在现有 HostHistory 加密消息内追加冻结捕获、分页、释放操作，复用 512 KiB 对象分片和逐片 ACK，
新增独立页面加密格式及浏览状态。保持 Relay 独占 1 MiB 帧限制、8 MiB 队列与不解码密文的边界；
不恢复 Server 对象存储，不扩展 LAN/NAT 验收，不把浏览页面作为 VT 事件或终端恢复快照。

当前应用 `ProductionHostHistoryService` 只有 LatestSnapshot、TranscriptPage、EventRange。
旧 `TerminalTranscriptPageV1` 是 Portable 文本投影，绑定快照源序号 S；它使用旧样式、调色板、行 ID 和分页规则。
Native/Graphics 快照的外部 transcript sentinel 为 1，且没有对应的 Portable transcript 页。
History V2 则是冻结主屏的原生渲染投影，包含 V5 帧、图片和 DRCS 资源，不可塞进旧 transcript 格式。

Android 当前上拉产品路径为 `loadTranscriptBefore()` → `loadHostTranscriptPage()` → 旧 transcript 合并；
本地 native capture 滚动只浏览手机已保留的历史。两者都不能证明远端冻结分页已接通。

## 2. 捕获身份、租期与资源边界

| 当前事实 | 协议／Host 必须补齐的约束 |
|---|---|
| Native CaptureId 是进程内递增编号，OwnerId 是原生 terminal 实例身份 | 使用随机 32-byte opaque token；Host map 绑定 session、recipient、key epoch、semantic profile、快照 S 和 native capture。OwnerId 不上网；CaptureId 仅供 decoder 校验，不能作为授权凭据 |
| Native lease 固定 300000 ms，读取不续期 | Host registry 按单调时钟到期；响应提供剩余租期供 UI 参考，客户端时间不构成授权 |
| 每 owner 最多一个 capture，进程最多 64 个／256 MiB；Graphics 单 capture 最多 128 MiB | 在创建前准入并保留核心上限；建议每设备／会话最多一个浏览租约，不通过反复导入 terminal 绕过 Host 配额 |
| Native capture 可独立于源 terminal 存活 | Host 必须显式释放、定时清理，并处理失败、取消与关闭；不能仅依赖 finalizer 或等下一次 native 调用清理 |
| Store 的新 outbox 会替换旧 S | Begin 创建真实冻结 capture，后续页只读同一捕获；不能每页重新读取 latest outbox |

Begin 请求需要幂等：同一身份的同一 request ID 重试不能反复创建 capture。普通会话结束不改变已有冻结内容；
删除历史、设备撤销、key rotation、退出账号必须取消相关请求并释放 capture。取消一个分页请求与释放浏览租约是两个动作。
断开 Relay 后可将捕获保留至原租期截止，以允许同一身份重新鉴权继续；不能续期或跨 epoch 复用。

最小捕获来源建议为：已验证的持久 native snapshot → detached import → CaptureHistory → 立即释放导入 terminal。
该路径覆盖运行中和已结束会话，无须扩展 SessionHost IPC；同时存在 snapshot/import/capture 的峰值，必须先做内存准入。
Begin 可要求精确 S；旧 S 已被 Store 替换时返回 ANCHOR_CHANGED。若允许选择 latest，必须返回实际 S 并开启新浏览 generation。

## 3. Revision、页预算和锚点

Capture revision 对应 NextSourceSequence，即快照源序号 **S+1**，不是 S，也不是分页请求的 Envelope seq。
返回并校验 S 与 revision 的关系。Line ID 是身份锚点，不保证数值递增；不得继承旧 transcript 的大小比较规则。
首次分页 sentinel 为 uint64 最大值，Kotlin 必须保留对应 `-1L` 位模式；末页以 has_more_before 为准，
不能强制套用旧 transcript 的 next_before_line_id=0 规则。

| 层级 | 现有硬上限／提案 |
|---|---|
| Native 页请求 | max_lines 1..128；max_cells 1..65536；max_utf8_bytes 1..4 MiB |
| TLHPV1 页面 | 16 MiB |
| TLHPV2 页面 | 64 MiB |
| 冻结页加密对象 | 提案：分别采用页面硬上限 +4096 bytes 容器空间；V2 最多129个现有512 KiB分片 |
| Relay | 保持帧小于1 MiB、队列8 MiB；逐片 ACK，不一次灌满整个对象 |

图片字节不计入 UTF-8 budget；当前 core 没有调用方指定的 encoded-byte budget。
V5 多个帧片段可能重复携带相同图片资源。最小接线可以在 wire limit 失败时降低 max_lines 重试；
若单个完整逻辑行仍超限，必须明确返回 budget/limit，不能截断逻辑行、删除图片或跳过该行。
如果产品要求小于64 MiB的分配前严格限制，需要另行扩展 core API，不能声称现有三个预算已经做到。

RPC 必须传递所请求的三个预算，并在 PAGE_BUDGET 返回 required_cells、required_utf8_bytes。
按 object kind + format 限制对象、分片计数与内存，不使用 Graphics snapshot 的128 MiB限额泛化所有历史对象。
客户端校验 token、revision、requested anchor、格式及重复分片元数据后，才允许页面进入对应浏览 generation。

## 4. 建议的新协议字段

保留现有 Envelope 消息类型、已有 protobuf 字段编号和旧枚举值；以下均为待实施的追加字段，尚未分配最终编号。

- HostHistoryRequestKindV1 追加 FROZEN_BEGIN、FROZEN_PAGE、FROZEN_RELEASE。
- HostHistoryObjectKindV1 追加 FROZEN_PAGE；V1/V2 由独立页面 format/version 区分。
- HostHistoryRequestPayload 追加嵌套 FrozenHistoryRequest：token、capture_id、capture_revision、max_lines、
  max_cells、max_utf8_bytes、page_format_version；复用外层 snapshot_seq 和 before_line_id。
- HostHistoryResponsePayload 追加嵌套 FrozenHistoryMetadata：token、capture_id、capture_revision、lease_remaining_ms、
  state_contract、head_truncated、required_cells、required_utf8_bytes。S、requested anchor、has_more_before、
  next_before_line_id、format、对象 digest 与 chunk 字段复用外层契约。
- Begin 返回捕获 metadata 和首个页面，包括合法的空页面；Release 返回专门允许的空 OK。
  当前 Android 空响应校验必须按操作显式接受这些结果，不能放宽所有响应。
- 使用稳定错误码区分 STALE_CAPTURE、STALE_REVISION、STALE_ANCHOR、PAGE_BUDGET、CAPTURE_LIMIT、
  UNSUPPORTED_FORMAT、ANCHOR_CHANGED；过期不等价于“历史已读完”。

## 5. 混版本协商与加密域

现有 state negotiation 证明会话解析配置，不证明对端实现冻结 RPC。建议新增 host_history_frozen_v1 能力，
再明确声明 TLHPV1/TLHPV2 页面支持。Graphics 必须选择 TLHPV2 与 V5；旧 peer 明确不支持新操作，
不能降成文本页或旧 transcript。Legacy 会话原路径继续可用；能力协商不得改变已持久化的会话 profile。

新增 C#/Kotlin FrozenHistoryCrypto，使用独立 key derivation/AAD prefix、format 与 container version，
不要扩大 SnapshotCrypto/TranscriptCrypto 的现有对象含义。认证上下文包含双方设备、session、epoch、
token、capture ID、revision、S、profile、requested anchor、请求预算、format/version 及对象长度；
页面内的对应字段须与外层选择 metadata 一致。CRC 只负责损坏检测，不替代 AEAD。

外层 request_id 继续用于一次传输的关联；页面对象身份应绑定 capture、anchor 和预算，
同一冻结页重试不能因重新生成 metadata 混入另一个 capture。密文缓存只在已绑定身份及租约生命周期内持有，
不将随机 token 作为持久授权，不存储原始指针，不要求 Server 解码任何新字段。

## 6. 产品浏览、恢复与释放

新增独立 frozen browse state／renderer 输入，复用现有 HistoryV2 decoder 和 V5 资源合成。
不能把 V5 转成旧 TerminalRowV1，不能把冻结页 prepend 到 live VT parser。
浏览定位至少包含 profile、capture、revision、lineId、cellOffset、navigationGeneration；晚到页只能命中同一 generation。

- 同一 token 未过期的网络重连：重新鉴权后继续。
- Host 重启、TTL 到期或资源释放：明确 stale，清理待合并页，再 Begin；不能使用旧 transcript 的自动 anchor rebase。
- 新 capture 不保证旧 line/cell anchor 仍存在；必须明确处理淘汰、截断及无法定位，不伪造偏移。
- 实时恢复仍为 Snapshot S 加有序事件 S+1…；FrozenPage 不推进 durable ACK，不创建 renderer checkpoint，
  浏览 ACK 继续使用独立 HostHistory chunk 控制。
- 页面只覆盖捕获时仍保留的完整主屏历史，不承诺无限 WAL 档案，不包含未封口的可变尾行。
  若需要捕获时可变尾行的完整浏览，还须明确复用 native frozen viewport API 的独立范围，不能假装 sealed page 已包含它。

## 7. 预计文件范围

应用文件位于同级 termlink 仓；本文件不复制应用源码，也不授权修改其他实施者正在维护的文件。

| 范围 | 预计文件 |
|---|---|
| 协议与共享向量 | protocol/protobuf/termlink_realtime.proto、PRODUCTION_REALTIME_V1.md、protocol/test-vectors 下新增冻结页向量 |
| Windows capture 服务 | 新 ProductionFrozenHistoryService.cs／registry；ProductionHostHistoryService.cs、ProductionHostHistoryResponseBuilder.cs、ProductionAgentRuntime.cs、capabilities；Store 精确 snapshot selection helper |
| 双端加密 | 新 FrozenHistoryCrypto；对应 protocol/crypto tests |
| Android 传输 | ProductionRealtimeSessionClient.kt：新 API、metadata 校验、按格式限额、token 生命周期 |
| Android 产品与显示 | ProductionProductRepository.kt、ProductState.kt、独立 frozen viewport state；terminal-engine-api/native 外部冻结页展示入口与 V5 renderer 复用 |
| Server | 不新增密文解析；补现有 HostHistory opaque routing、方向与预算回归 |
| Core | 最小方案复用现有 history API；仅严格更小分配前预算等新增要求才另立 ABI 变更 |

## 8. 必需验证与完成标准

以下为待实施门禁，没有因编写本文件而运行或通过：

1. 同 token/anchor 页面逐字节一致；冻结后输出、resize、ED3、alt/reset、图片删除替换、DRCS 重定义不改变捕获。
2. 跨 session/设备/epoch/token 拒绝；Native OwnerId 不泄露；Begin 重试幂等；取消、重复释放、租期到达无泄漏。
3. 固定五分钟 TTL，读取不续期；断线重连、Host 重启、设备撤销、删除历史、key rotation 正确失效。
4. 非单调 line ID、uint64 latest sentinel、head-truncated、空页、重复/缺失/过期 anchor；无孤立宽字符或逻辑行截断。
5. 细胞/UTF-8预算不足返回精确需求；wire budget缩页与单行超限；capture总预算和失败前准入。
6. V2页面64 MiB与容器开销边界、129分片、元数据篡改、乱序/重复、慢消费者、取消、晚到旧generation；Relay限制不变。
7. 双端 crypto golden、AAD全部身份/anchor字段篡改、旧Transcript/Snapshot/Frozen格式交叉拒绝。
8. 新Windows+新Android、新Windows+旧Android、旧Windows+新Android；旧功能保留，新格式不得静默降级。
9. 页面呈现保留图片/DRCS/裁剪/透明度；浏览不改变实时ACK、恢复checkpoint或宿主副作用；过期重开不混页。
10. 实际Relay链路、多设备与长时间资源回收验收；协议测试与真实产品测试分别留证，未测项保持未完成。

实施前仍需固定 Host registry 与客户端页面缓存的具体数量/字节预算，并完成锁顺序、取消与准入的代码级设计。
本提案不改变总计划的阶段进度，不声明 E02 产品完成，不执行源码修改、构建、提交、推送或发布。
