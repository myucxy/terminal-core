# G01 Windows Kitty broker 接线提案

日期：2026-10-10。

**状态：设计提案，未实现、未验证，不表示 G01 已完成或已通过验收。** 本文来自只读代码复核；不改变源码、协议、能力声明或总计划。安全边界、事件契约、配额及兼容策略由主代理最终决定。

**平台边界：Kitty 的 `t=s` 在 Windows 使用命名共享内存对象；本项目尚未实现经过授权、限额和生命周期校验的 Windows 共享内存 broker，因此当前不支持该媒介。** 不能仅凭打开同名映射或磁盘文件就宣称兼容。本文提出的是 Windows 文件／临时文件 broker 接线，以及对未实现媒介的确定性拒绝。
协议依据：[Kitty graphics protocol, “The transmission medium”](https://sw.kovidgoyal.net/kitty/graphics-protocol/#the-transmission-medium)。

## 目标与建议

建议采用以下流水线：

```text
core 纯预处理原始 Output
  → 受限目录句柄下读取并捕获不可变资源字节
  → 有背压的持久化事务组
  → core 原子应用原始 Output 与捕获结果
  → authority 发送确定性 PTY 回复
```

不能直接在 ACK reader 的 authority 回调里读取文件并等待后续 ACK，也不能在处理完原始 Output 后把图片结果追加到流末尾。同一批输出内的“显示图片 → 删除图片 → 光标移动／文字”必须保持原始顺序；跨 Output 分段的 APC 和宏展开也必须继续使用 core 的解析器。

不在 C# 另写 Kitty APC 解析器，不允许重放端重新读取原路径，不扩大现有单帧 IPC 上限来容纳整张图片。

## 已确认的断点

以下为复核时的代码状态；行号会随后续改动变化。

| 位置 | 已有能力与缺口 |
| --- | --- |
| `crates/termlink-terminal-core/src/kitty_graphics.rs` 的 `Content::BrokerReference`、`TransferDecoder::push` | 已验证引用的 Base64、UTF-8 和 NUL；只保存引用。`O` 没有消费者，没有宿主范围读取契约。`S` 当前参与压缩 PNG 的长度处理，读取长度与解压预算尚未明确分离。 |
| 同文件的 `decode_pixels` | 已支持受限 RGB/RGBA、PNG、zlib 解码，并注明 broker 数据须预先进入有序流；它是 Rust 能力，尚非现成的宿主回注 API。 |
| `crates/termlink-terminal-core/src/terminal_graphics.rs` 的 `apply_kitty` 路径 | 遇到 `BrokerReference` 直接返回 broker 未连接。`a=q` 转为 ENOTSUP 回复；普通 `a=t/T` 进入错误路径，可使 SessionHost authority 失败。 |
| `crates/termlink-terminal-core/src/lib.rs`、FFI、头文件、JNI/.NET | 现有事件只有 Output、Resize、Advance、Gap、HostGeometry、HostTheme，没有资源请求提取和捕获结果回注入口。 |
| `host_effect.rs` 与应用 `NativeTerminalHostEffectFrameV2.cs` | V2 只有 PTY reply、Resize request、Platform action，不存在 BrokerReference effect。 |
| 应用 `win-agent/src/SessionHost/TerminalCoreAuthority.cs` | 只应用已经 ACK 的事件；当前没有 broker 的准备、组装和最终应用路径。 |
| 应用 `win-agent/src/SessionHost/SessionHost.cs` 的 `AgentPipeBridge` | 默认 pending 预算为 4 MiB，溢出会转成 Gap；不能一次发布 16 MiB 资源。 |
| 应用 `win-agent/src/Agent/SessionAgent.cs` 的 `EncryptedEventBuffer` | 默认持久缓冲为 16 MiB，Append 后逐帧 Trim；最大资源加分片／加密开销会超额，可能在 Commit 前裁掉 Begin。 |
| Producer、Relay、Android | 没有资源事件解析和组装。现有 Producer 对未专门处理的事件默认 Advance，因此仅新增 IPC 类型会造成图片语义丢失。 |

应用仓库位于相邻的 `../termlink/`；不得把其源码复制进 core 仓库，也不得由应用构建脚本读取或编译 core 源码。

## 提议的 core／ABI 契约

以下名称和结构均为提案，不是已经存在的 API：

```text
prepare_output_broker_v1(base_revision, raw_output)
  -> NeedResource(request) | Ready

request:
  request_ordinal
  source_offset_end
  canonical_control_digest
  medium = f | t | s
  reference_utf8
  normalized_read_range

prepared_supply_broker_v1(request_ordinal, captured_bytes | error_code)
  -> NeedResource(next) | Ready

apply_output_broker_v1(sequence, raw_output, ordered_capture_results)
  -> existing HostEffectFrameV2
```

准备阶段复用 core 的 candidate／atomic prepare 机制，操作独立候选状态，不更新 authority、不发送 PTY 回复、不开放文件系统回调。若通过 ABI 暴露准备句柄，必须同时规定其释放、取消、失效和内存预算；不能跨 FFI 长期持有指向 live terminal 的 Rust 可变借用。

请求身份至少包含 ordinal、原始 source offset 及规范化控制参数摘要。不能只用 offset：宏展开的多个请求可能共享同一原始字节位置。预处理必须支持跨 Output 的 parser 状态，并限制每次预处理的资源请求数量。

读取结果是不可变字节或稳定的错误枚举。最终 apply 校验结果数量、顺序、控制参数绑定、长度和摘要，将对应 BrokerReference 转为图像或确定性外部错误，再调用已有 `decode_pixels`／`GraphicsState.apply_kitty`。整段原始 Output 只提交一次，不能让引用后的指令先执行。

已有事件 ABI 的单事件 1 MiB、批次 4 MiB 限制不能被绕过；资源回注需要独立、有明确资源预算的新入口。预处理的 PTY effects 不可提前交付；只有最终 durable apply 产生的 effects 可以由 authority 执行，镜像／历史重放仅验证并丢弃外部 effects。

### 错误语义

- 授权拒绝、文件不可用、不支持的媒介、超额等外部结果必须进入捕获记录，避免重放时依赖现场文件系统。
- Kitty `q` 的静默规则继续由 core 统一处理，不由 broker 手工拼接 APC 回复。
- 对外错误不得回显路径、资源内容或原始 Win32 错误文本。当前 canonical reply 允许 EINVAL、ENOTSUP、ENOSPC；若引入其他错误码，需要同步更新 core 与两端 HostEffect 解码器。
- broker 外部失败与非法图像数据／core 协议错误应分别定义。是否对普通 `a=t/T` 的外部失败返回 Kitty 错误后继续会话，由主代理核定；不能未经决定沿用当前“broker 未接线”导致的整会话失败行为。
- `O/S` 必须在实施前按 Kitty 官方规范核定，特别是压缩 PNG 的读取长度与解压预算。当前实现不足以证明二者已经正确分离，不能直接照搬现有 `S` 用法。

## 提议的 IPC 与 durable 回放契约

建议在同一有序事件流中引入事务组，具体枚举值待统一分配：

```text
BrokerCaptureBegin:
  transaction_id, raw_output, bounded ordered result manifest

BrokerCaptureChunk:
  transaction_id, resource_ordinal, offset, bytes

BrokerCaptureCommit:
  transaction_id, manifest_digest
```

manifest 绑定原始输出、请求身份、每个结果的状态、长度和内容摘要。Begin/Chunk 只组装资源并推进序号；Commit 使用自身序号原子应用原始 Output 与全部捕获结果，产生现有 HostEffect V2。这里不先提交一个裸路径 Output，再用后续事件补图像。

1. 单片建议 512 KiB，完整 IPC body 必须小于现有 1 MiB 上限。
2. 单资源捕获字节和解码后的 RGBA 均不超过 16 MiB；此外必须规定整组捕获总量、请求数量、并发组数及超时，避免多个小请求绕过预算。具体 aggregate 配额待主代理决定。
3. 使用独立、有界的发布 worker；每片等待 durable ACK 后再发下一片，保持 pending 在预算内。不得一次向当前 `Publish` 塞入整组资源。
4. 所有影响终端状态的事件经过同一发布顺序。在事务期间不得插入 Resize、Geometry 或其他 Output 改变预处理基准；候选状态需要明确的基准版本检查。
5. ACK reader 只能验证、应用已有帧、发完成通知；不得等待后续 ACK。等待不能持有 `terminalWriteSync`、authority 锁或发布队列锁。现有所有同步 Publish 调用点均需复核，不能仅给新增 worker 加锁。
6. 同一事务的片段必须校验连续 offset、资源身份、总长、最终摘要。重连时相同序号必须对应相同内容；冲突重发不得覆盖原记录。重复片段不能延长事务总时限。
7. Begin 前预留完整 durable 容量；Trim 以完整事务组为单位，不能裁掉尚未完成组的前缀。现有默认 16 MiB 不足以保留一个 16 MiB 资源及开销，需要 Graphics 专属配额或在开始前明确拒绝。
8. 组装期间不得发布依赖中间状态的可恢复 snapshot；Commit 后再开放。恢复入口不能落在事务中间而假装拥有完整资源。
9. 崩溃、缺片、Gap 或无法完成的组不能应用部分图片、发送成功回复或删除临时文件。未完成组的会话终止／历史不可用表达与恢复清理规则，必须和现有 Gap、Interrupted、SessionRemoved 流程一并定义。

需要把资源分片的 durable 保留范围贯穿本地 event buffer、history WAL、加密 Relay 输出和 Android 重放；任何一个层面只保留 Commit 都不够。

## Windows 受限文件 broker

### 授权和根目录

- 权限来自本地配置或启动时授权，固定允许的根目录句柄。终端输出、OSC 7、远程输入和文件内元数据不能增加权限。
- 优先使用会话专属传输目录；不能默认授权整个用户目录或当前工作区。是否向子进程提供专属临时目录环境变量，由产品策略决定，不修改系统／用户级环境变量。
- 根目录的建立、ACL、所有者、卷身份与生命周期需要独立检查。连接／重连不能扩大原会话权限。
- 只处理本 Windows 宿主上已授权的文件。SSH 远端输出的路径不能被当作读取远端文件的授权，也不能猜测映射路径。

### 打开和捕获

- 按目录句柄相对打开并拒绝重解析；可考虑封装 `NtCreateFile` 的 `RootDirectory + OBJ_DONT_REPARSE`，具体 Windows API 封装须经安全复核。
- `GetFullPath` 加字符串前缀检查不足以满足该边界；`FILE_FLAG_OPEN_REPARSE_POINT` 单独使用也不能证明中间路径没有被重解析。
- 对实际文件句柄检查普通磁盘文件、非目录、非 reparse、允许的卷／根目录、所有者策略及 `NumberOfLinks == 1`。拒绝 UNC、设备路径、ADS 和不符合规范化策略的路径。
- 使用拒绝写入／删除共享的读句柄，按同一句柄完成范围读取。对 O/S、文件长度、offset + length、EOF 和预算做溢出与边界检查。
- 捕获前后核对文件信息；文件身份和读取结果不得通过重新打开路径来核验。资源字节捕获后保持不可变，摘要覆盖实际进入 durable 流的字节。
- 硬链接数和 reparse 检查必须基于句柄，必要时在读取后及删除前复核；不能只依赖 `File.GetAttributes(path)`。

### 临时文件删除

- `t=t` 从首次打开时持有所需 DELETE 权限，但不提前设置 delete-on-close。
- 只有对应 Commit 已 durable 且成功应用，才在同一文件句柄上设置删除 disposition，随后关闭该句柄。
- 不使用 `File.Delete(originalPath)`，不为清理重新打开原始不可信路径。
- 读取、持久化、应用、取消或断线失败时保留原临时文件。崩溃后的清理不能凭旧路径删除可能已被替换的文件。
- 已提交后的删除失败只记录本地清理诊断，不能重复应用图片、发送虚假失败或撤销已提交状态。

### Windows 命名共享内存

Kitty `t=s` 在 Windows 对应命名共享内存对象。本项目尚无安全的名称授权、对象身份、大小、所有权、并发写入和生命周期检查，因此应产生可持久化、可重放的 unsupported 结果。直接调用 `OpenFileMapping` 或按名称读取磁盘文件不能替代这些检查。

因此即使文件／临时文件路径完成，本 Windows 实现也不能笼统声明“Kitty file/temp/shared-memory 全支持”或据此把 G01 全量验收标成通过。平台缺口应保留在能力和验收说明中。

## 可复用代码与不能直接复用的边界

- core：`Control::parse`、`TransferDecoder`、`decode_pixels`、`GraphicsState.apply_kitty`、candidate／atomic prepare、canonical Kitty response。
- SessionHost：`ProtocolResize.cs` 的独立 worker 模式证明 ACK 回调不能等待由自己触发的新 ACK；可参考其调度思路，不能照搬其事件语义。
- 应用 `AiSessionArtifacts.OpenArtifact` 已有打开后句柄最终路径检查，可参考 P/Invoke 和错误清理，但缺少本方案所需的完整按句柄遍历、硬链接限制和同句柄删除。
- `InputDeduplicator.AtomicWrite`、DPAPI event buffer、history WAL 的受保护原子写入和冲突重发校验可复用；需要新增事务组容量和保留语义。
- 现有图像、Render V5、Snapshot 2.22、History V2 数据表达可继续用于完成提交后的状态；是否需要升级 immutable semantic profile 或增加严格能力协商，由主代理决定，不能让旧端静默接受新事件。

## 必要文件范围

| 层 | 必要修改位置 |
| --- | --- |
| core | `crates/termlink-terminal-core/src/kitty_graphics.rs`、`terminal_graphics.rs`、`lib.rs`；新增捕获结果契约及对应测试。 |
| ABI | `crates/termlink-terminal-ffi/src/lib.rs`、`include/termlink_terminal.h`、`android/jni/termlink_terminal_jni.cpp`；显式新增 API、状态和限制，不放宽旧 ABI。 |
| .NET／Kotlin 绑定 | 应用 `NativeTerminalCoreV2.cs`、新的 broker 契约绑定；Android `NativeContracts.kt`、`JniNativeBridge.kt`、`NativeTerminalEngine.kt` 和 API 层。 |
| SessionHost | 应用 `SessionHost.cs`、`TerminalCoreAuthority.cs`；新增受限 broker 与有界发布 worker。 |
| IPC／持久化 | 应用 `LocalIpc.cs`、`SessionAgent.cs`、`ProductionTerminalHistoryStore.cs`、`ProductionTerminalHistoryProducer.cs`；事务校验、保留、恢复、snapshot 边界。 |
| Relay／产品重放 | 应用 `ProductionAgentRuntime.cs`、`protocol/protobuf/termlink_realtime.proto`；Android network、session、engine、history 解码及应用入口。 |
| 协商／文档 | SessionHost/Agent 及远端能力门禁、持久 profile 校验、能力与验收说明；总计划链接由主代理维护。 |

## 验证入口与验收场景

以下测试尚未实现或运行：

- core／FFI：准备阶段无 live mutation 和无提前 PTY effects；结果绑定错误、超额、释放、取消；跨 Output APC、宏展开、多引用以及同帧“图片后删除”的原子顺序。
- `NativeTerminalCoreValidation`：新 ABI 及托管绑定严格校验，旧 ABI／Legacy 路径不变，图片快照和历史回放一致。
- `SessionHostValidation`：真实 ConPTY child 发出 `t=f/t=t`，受限读取得到图片，提前／延迟 ACK 不死锁；删除只发生在 durable + applied 后，且针对同一文件句柄。
- 文件安全：目录／文件替换竞态、junction/symlink/reparse、硬链接、ADS、UNC、设备路径、所有者变化、写入冲突、读取截断及 offset 溢出。
- 容量／故障：16 MiB 边界、整组上限、预算预留、组内 Trim、防洪、缺片、冲突重发、ACK 丢失、重连、超时、Agent/Host 崩溃和清理失败。
- `ProductionTerminalHistoryValidation`：原文件删除或内容改变后，持久化回放仍得到完全相同的图像与错误；未完成组不产生错误 checkpoint。
- Android JNI／network／session：资源分片预算、认证能力门禁、去重、组装、同序应用、history-only 和重连；镜像端绝不读取 Windows 原路径或执行删除。
- Windows 命名共享内存：当前对 `t=s` 明确报告 unsupported；不得把仅能解析引用、未经授权的 mapping 或文件替代测试记作共享内存支持。

## 实施前仍须确定

1. Kitty O/S 的规范语义与压缩 PNG 长度预算如何分离。
2. 普通传输的外部 broker 错误、协议错误及会话失败之间的正式策略。
3. 允许目录、所有者、临时目录授权及 Windows 原生打开封装。
4. 事务 aggregate 配额、持久缓冲最小容量、组内 snapshot 禁止及恢复/裁剪规则。
5. 新能力与 immutable profile 的兼容门禁，以及所有 Relay/Android 消费端的上线顺序。

这些是待审设计决策，不是已完成能力。本文未执行源码修改、构建、原生文件安全实验或端到端测试。
