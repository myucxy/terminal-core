# 终端全量能力整合状态（2026-10-10）

本页记录本轮中断恢复后的实际边界。早期设计文档中的“未实现”是当时基线，不能代替本页的实现状态；下列构建及单元测试也不能代替产品验收。

## 本机安装与续测（2026-10-11）

本节覆盖下方旧批次的安装和冻结历史状态；整体 22 组完整验收仍未完成。

- Windows 最新签名 DEV Setup 0.3.3.132 已安装至 `D:\Dev\Tool\TermLink`，安装器退出 0，既有签名连续性与原生 DLL 签名前后载荷一致性通过。安装包 SHA-256 为 `F0559D47DCA54363590146B4FB8AB83E773DE0DA0A15A20E0B74C7DB9E390F30`；签名后的 DLL 哈希为 `69718E3AFD120FC010355AF5A1756F9A604362029C299A33DFBCEA706216CFF7`，与未签名 K2 库的差异仅为合法 PE 签名变动。
- 可见模拟器 `TermLinkReleaseUpgrade_API36`（emulator-5556）已原地安装最新 `TermLink DEV`，包 `dev.termlink.harness.recovery`，0.3.3/132，登录保留。APK SHA-256 为 `63E954339F2BF9F6081FE43B1658D5A9701A894AABC41B09726895C2483BF3E3`；ZIP 内四种 ABI 库与锁文件一致。
- Frozen BEGIN 的协议等待根因已修：REST 控制租约切换不再清零当前已认证附着的 terminal generation；旧输入 outbox 清理和租约 fence 保留。定向测试及最新 network **400/400** 通过，新 APK 已包含修复。`r4frozen8` 的 BEGIN、两次 PAGE、RELEASE 均通过；其末尾失败来自原生窗口模式不读取 Host stdin 的测试脚本错误。
- 脚本改为现有 `show_ime` 加真实安卓键盘输入后，`r4frozen9` 的 **21 个断言全部通过**：BEGIN/两次 PAGE/RELEASE 中 live ACK 保持 34，释放后安卓输入经 Windows 回显，live ACK 推进至 75。测试 Host 已退出，模拟器保留。证据为应用仓 `artifacts/tail-first-e2e/r4frozen9/evidence.json`；这不覆盖 E02 的并发输出、过期、取消和故障矩阵。
- 新安装双端 `r4installedcold` 真实 Relay 文件图像捕获/fetch/native apply/durable ACK 通过，capture source seq 39、最终 ACK 50、magenta 像素 6804（基线 0）；9 组协议 smoke 通过，其中 C12 实际新增字形前景像素 **1050**，已验证此前空白修复。它们仍属于 smoke，不代表每组完整语义验收。前一次未冷启动的 `r4installed` 传输/ACK 成功但图像不可见，界面状态相关原因仍需独立回归，不能丢弃这条失败证据。
- 用户已将普通客户端切至 `termlink2`/admin。本机原缓存设备的 signed GET 返回 `DEVICE_REVOKED`；同账号登录复用了被撤销 DeviceId，不能靠重复登录恢复。先备份本机三个账户文件，再迁入已停止隔离后台的有效 config/DPAPI credentials，保留普通 identity store 并由启动流程合并，保留 PC ID/会话历史及服务端撤销记录；通过现有 LoginAccount 刷新有效身份的过期授权后，普通后台 Relay Connected、signed GET `/v1/pcs` 成功且 HOME-PC online。备份位于工作区私有 `.workspace-backups/local-admin-device-recovery-20261011`。被撤销设备的产品恢复流程及准确 UI 提示仍是新增待完善项，当前只完成本机恢复。
- 普通后台 `r4localadmin` 在启动时退出 125：本机 TermLink 私有历史根/父级继承了其他 SID 的写权限，被 Broker 校验准确拒绝。已将五个 TermLink 私有目录的原 SDDL 保存到上述备份，仅保留当前用户/SYSTEM/Administrators 的必要权限，历史文件未删除、安全校验未关闭。随后普通后台 `r4localprivate` 真正通过：source seq 39、最终 ACK 50、图像像素 6804、9 组 smoke、C12 新增字形像素 1050，Host 正常退出 0。证据为应用仓 `artifacts/broker-product-e2e/r4localprivate/evidence.json`。其他机器的私有目录初始化/迁移权限仍须产品代码完善与回归，不把本机 ACL 恢复当作通用修复。
- `.env` 的 `TERMLINK_PUBLIC_URL`、在线 client-config 和 OIDC discovery 均为 `https://termlink2.kget.cc:8910`；Docker inspect 管理查询在 7 秒内未返回，不能宣称已直接核验容器 Env，未重启 Docker、未改部署或发布。

本轮仅本机 DEV 安装与 Relay 续测；未提交、推送或正式发布。

## 最新进展（续测）

本节及[产品验收矩阵](PRODUCT_ACCEPTANCE_MATRIX_20261010.md)的最新结果覆盖下方旧批次快照，不将不同批次的测试和二进制混为一组。

- 最新核心 754、golden 10、FFI 71，共 **835 项通过**；fmt、Clippy `-D warnings` 通过。任意视口 DRCS 保持标准尺寸精确匹配，在 106×32 等尺寸选择最近的已加载 face；resize、snapshot、frozen 资源回归通过。`Screen::validate` 的逻辑行 head 查重改为 HashSet，消除历史校验的平方复杂度，保留完整历史和逐 cell 校验。
- 五平台库已重新导出并通过 5/5 校验，包含 DRCS 与上述历史校验修复。DLL=`c2030d122ea312667d0cab9c0018f38d579ee17d77efbac6286f1296c249d006`，x86_64 SO=`b2e49bc494de159589e9c49d5f6ad0a04d6c42fbfd6929ce222b6257dd18298a`。来源仍 dirty DEV。Windows `final-r4` 三组件 publish 成功、隔离 Registry 已原地升级且 Relay Connected；安卓 APK=`1EEE1CC8836E2D95955F7EB1227719FAEA4DD9BA2EC0A3692180915B8A680979`、入包四种 ABI 与锁文件一致，已原地安装到 `.recovery` 模拟器测试包，版本均保持 0.3.3/132。
- Android SUBSCRIBE 与 negotiated Host History 请求已补 Broker/Host History 能力，network **399/399** 通过。真实 Relay `r4stage` 已完成 Kitty 文件捕获→Android fetch/native apply→durable ACK：capture 1、source seq 33、最终 ACK 43、magenta 像素 6804（基线 0）、Host 退出 0。TL-H130 已解决。
- 同 run 的 C03/C04/C06/C09/C10/C11/C12/C13/C14 是 9 组 ACK/hash/截图 smoke，尚非完整语义验收；C12 空白由任意视口 DRCS 修复处理。新增 C12 实际字形像素断言脚本已静态自检，待新双端库真实重跑。
- Windows 增强 PowerShell 新建回执漏 `terminal_command_id`，已修复并通过 FileOperationsValidation（含增强及四种旧 Shell），已进入 `final-r4`；安卓 UI 真实新建产品链路仍待测。
- `r4frozen5` 成功打开 106×32 会话、snapshot bootstrap、live ACK 37，BEGIN 返回 `FROZEN_HISTORY_FAILED`。根因是 frozen bootstrap 对已有 live client 的 session lane 重复注册。Android 改为借用 live client，并在所有 live 退休路径先退休 frozen owner；仅关闭自身拥有的 client，FrozenHistoryBrowseTest 5/5 通过，修复已安装。
- 最新 `r4frozen6` 在 OPEN 阶段返回 **TL-H131**，尚未执行 BEGIN/PAGE/RELEASE。Android 等待附着回执 15 秒超时；Windows 此前约 0.4 秒完成同会话快照选择，但缺少 SUBSCRIBE/attach 的关联阶段日志，无法确定超时位置，不能直接归因为 native 性能。证据为应用仓 `artifacts/tail-first-e2e/r4frozen6/evidence.json`。
- 新五库的 `r4frozen7` 已完成：打开成功，约 55.8 秒，106×32、`REMOTE_SNAPSHOT`、live ACK 35、Connected/subscriber ready/surface 均正常；BEGIN 仍返回 `FROZEN_HISTORY_FAILED`，安全日志精确定位到 `phase=profile`、`awaitTerminalProfile` 的 15 秒超时。未进入 frozen RPC/PAGE/RELEASE，不能宣称重复注册修复后 E02 整体通过，也不能以这一次打开成功证明 H131 已彻底消失。测试 fixture 正常清理，模拟器保留；证据为 `artifacts/tail-first-e2e/r4frozen7/evidence.json` 与 `recovery-android-20261010-frozen7-bootstrap-logcat.log`。正在核对 live client 就绪状态与协议状态等待的一致性。
- 5200 行模拟器性能探针对照：旧 SO feed 约 9.2 秒、render 约 5.5 秒；DRCS/V5 快路径 SO feed **8704.33 ms**、render **5179.15 ms**；最新 HashSet 库 feed **8311.95 ms**、render **4649.10 ms**，1/1 通过，累计桶值不含初始 render。分别减少约 4.5%/10.2%，仅为本次模拟器对照、未做统计重复，改进有限；Broker 初次打开约 37.7 秒，不能称性能验收完成。最新日志为 `recovery-android-20261010-history-fastpath-perf{,-buckets}.log`。

**22 组整体仍未完成验收；未提交、推送或正式发布。**

## 历史批次基线（以最新进展为准）

- 最新核心测试 748 项、golden 10 项、FFI 71 项通过；Rustfmt 和 Clippy `-D warnings` 通过。包含 G01 不支持媒介诊断及 C02 同调用输入错误诊断；后者以 `TL-INPUT-DIAG/1` 精确记录错误类别、按键、Unicode 码点和修饰键，不复制粘贴内容。多捕获预处理复用真实 parser，具备原子提交、取消及发布失败回滚。
- 五平台 DEV 原生库重新编译并通过应用仓锁文件 5/5 哈希校验。来源 revision 为 `e6e849172c6fca0de01ae0799eae97ad9483e687`，`source_dirty=true`，不可作为正式发行库。Windows DLL SHA-256 为 `9c1156a80cefa1dc501cfb958b632cf83ef6c1d3b99426e82b31fe489d7ca87b`，x86_64 Android SO 哈希前缀为 `ced273e160cd4650`，其余精确哈希见应用仓 `terminal-core.lock.json`。JNI 四个输入诊断方法已补注册并通过源码契约检查；导出脚本构建前后校验 Git 跟踪及未忽略源码的路径与内容。
- Windows `final-r2` 的 Agent、SessionHost、Desktop 三组件 DEV `0.3.3.132` 已独立 publish，产物中的 DLL 与本批 `9c1156…` 哈希一致。NativeTerminalCore、SessionHost 全量、FileOperations、TerminalSurface、Desktop.Headless、WindowsAdapter 和 Heartbeat 兼容回归通过；隔离 Registry `final-r2` 为 Connected。真实 ConPTY BridgeInput 故障注入后，测试 Agent 管道收到 `Running(5) → Ended(8, 125) → Removed(9)`，该证据验证 Host 到测试管道的故障清理，不等于完整 Relay。历史前批共享内存 real Job、非零 O/S、真实 Claude 启动/退出证据也不能代替真实 Relay 全链路。
- Relay 冻结历史和 Delta 不透明转发测试通过；16 个 Delta 分片不推进终端重放序号。服务端 R4 的 Go 全库 32 个包中 1077 项通过、57 项因环境条件跳过、零失败；额外 PostgreSQL/Redis 空库集成 3/3 仍通过。R4 DEV 镜像 ID 为 `sha256:5b144d79599b4c55abd39d947a800f221dcd2a01f2a350f5c78763511ae46fb4`，已部署到本机隔离容器；尚未推送 Docker Hub 或正式发布。
- Windows 受限文件 broker 已有按目录句柄读取、拒绝重解析/硬链接、同句柄延迟删除与竞态测试。真实 Output 已接入 Host 捕获、Agent 缓冲和生产 WAL 前置校验；DEV 代码已启用 `terminal_broker_captures_v1` 与 `host_history_v1` 能力，`powershell-7-enhanced` 会请求使用。正式客户端尚未升级，真实 Relay 产品链路尚未验收。
- Windows S7 Host History Release 验证通过，Agent Release 构建 0 警告/0 错误。真实 native 跨语言合成向量将 365896-byte base 恢复为 365912-byte target，密封 Delta 为 5480 bytes，目标输入 epoch 为 7（base 为 0），canonical target 逐字节一致。
- Android 本批新 SO 的模拟器 JNI **59/59**、S7 产品 **2/2**、H02 真 Keystore 仪表 **2/2** 通过；network **398/398**、API **49/49**、native JVM **201 项通过/2 项跳过**、app 定向 **54 项通过**。新增 command exact mapping/GET 请求头测试 **3/3** 通过，安装到模拟器后设备页恢复显示。这些是模拟器/模块证据，真实 Broker、完整 Relay 链路和真机行为仍须分别验收。

## 正在整合及尚未通过的门禁

| 范围 | 已有实现 | 剩余门禁 |
| --- | --- | --- |
| E02 远端冻结历史 | Host lease/RPC、独立加密、Android 请求与两页只读展示；BEGIN 锚点变化及缩页重试有界；新 SO 的通用 JNI 59/59 通过 | E02 产品场景、真实 Relay BEGIN/PAGE/RELEASE 与 live ACK 不变仍待验收 |
| E03/S7 Delta | core 全状态/图形资源差分与 journal、FFI/JNI/托管绑定；8 MiB 加密对象与 512 KiB 分片；Windows 原子 current/previous 选取与 Host 验证；新 SO 的 Android 产品恢复模拟器 2/2 通过 | 真实 Relay、完整故障与内存压力验收未完成 |
| G01 文件图像 | Host 实际 Output、绝对 TMP 路径与精确 ACK 隔离验证通过；Android 锁外资源获取、有界队列、live/suffix、持久缓存与预算接线已编码并完成多层测试。真实 Relay `r4stage` 捕获→资源获取→native apply→durable ACK 已通过 | 最新五库仍须重跑 G01 产品与真实 Relay，取消/重连及容量压力待验；TL-H130 为已解决的历史失败，最新打开阶段 TL-H131 尚待定位；正式客户端尚未升级 |
| G01 Windows 共享内存 | Host 分配命名映射，CurrentUserOnly 管道核验客户端 PID/Job，PTY 只查本会话登记名并复制私有字节；core 接受已验证的 `s` 捕获，已导出本批五库。real Job 与非零 O/S 的 Windows 隔离验证通过 | 真实 Relay、Android 新 SO 跨端回放和恶意/过期映射边界仍待验收；未经登记的任意 Win32 映射和 WSL/POSIX 映射不提供透明兼容 |
| S8 全链路 | 各层定向回归和隔离产物 | 实际双端、旧版混用、取消/重连/恢复、资源压力、升级回滚及逐组能力证据尚未全部完成 |

### Delta 必须保留的产品语义

`TLDELTA1` 只包含核心状态，不含产品的本地输入 epoch。新的 `TLDSEAL1` 加密头为 **336 字节**，认证目标 `observedLocalInputEpoch` 和 `initialTranscriptBeforeLineId`；不得把 base 的 epoch 复制到 target。Windows 从目标快照 artifact 取值，Android 拒绝小于可信 base 的 epoch，当前 Graphics transcript anchor 必须为 1。

Delta 请求上下文为会话 UUID、PC device UUID 和固定 state contract 的域分离 SHA-256。最大密封对象为 8 MiB，最多 16 片；分片 ACK 不等于终端 durable ACK。目标内存预算在解密和 native apply 前预留，完整目标及后缀经过现有快照缓存/renderer 绑定事务后才推进终端 ACK。认证失败或损坏不能伪装成普通 full-snapshot fallback。

应用协议与跨语言向量分别位于相邻应用仓 `protocol/protobuf/PRODUCTION_REALTIME_V1.md` 与 `protocol/test-vectors/state-delta-sealed-v1.json`。向量验证 AEAD 字节一致性，不替代 native Delta 有效性测试。

Windows 生成阶段限制为全进程一次，canonical base+target 合计不超过 64 MiB，超限明确选择完整快照。生成门禁在网络传输前释放，不能持有它等待分片 ACK。`state-delta-product-v1.json` 是另外一份可实际 native apply 的合成产品向量，Android 正使用它检验完整路径。

### G01 持久接线选择

原 Begin/Chunk/Commit 终端事件组会碰到 Android 32-event/3-MiB 窗口与 durable ACK 的相互等待。当前选用一条 BrokerOutput 事件引用先持久化的不可变捕获对象，大字节通过现有 Host History 独立分片 ACK 传输。对象按可信 session store、事件序号、UUID、长度和摘要绑定，原始终端路径不参与对象读取；配额满时拒绝新捕获，不提前裁剪已有引用。

sidecar、TLCAPV1 捕获对象、TLBROV1 引用、TLBCSEA1 AEAD 已实现并定向验证；应用契约见 `protocol/protobuf/BROKER_CAPTURES_V1.md`。Window Host History 验证通过，Agent Release 0 警告/0 错误；Relay 同时检查双方 broker 与 host_history capability，65 个分片仍不推进终端 ACK。容量拒绝可用全失败内联对象重放，无需再申请 sidecar 空间。

Android 接收循环已改为锁外获取资源，以 32 项/3 MiB 的认证明文队列保持顺序；控制/资源响应仍可继续处理，持久缓存及 native 应用完成后才推进终端 ACK。连接代次、密钥和 renderer 代次防止过期结果误入新会话。共享存储的 Local mutex 当前要求同一 Windows 登录会话；目录 rename 的断电元数据持久顺序尚未验证，缺失对象须明确拒绝恢复。

独立复核发现的三个 Windows 恢复缺口已修复：先挂接 runtime 再恢复、每个 Broker 验证先于合成结束帧、精确记住 pending Broker 序号并在重连时复核后继续。损坏捕获仅隔离对应历史并返回 `BROKER_CAPTURE_RECOVERY_UNAVAILABLE`，保留原始数据，不阻断其他会话。前一批 Windows 恢复/重试隔离回归已通过；本批新库和真实 Relay 的组合验证仍受上表门禁约束。

### 本轮环境恢复

Android app 仪表构建遇到拆仓前绝对路径残留；单独禁用缓存和隔离 Gradle user home 不足，清理各模块生成的 build 目录后已能产出 APK。随后默认 Gradle 版本 0.1.26/32 被模拟器已装版本拒绝；改为独立 `.recovery` 包、显式 DEV `0.3.3/132` 后，S7 产品仪表测试已通过，保留既有正式及 DEV 登录数据。

本机当前运行 R4 DEV 隔离容器，沿用克隆数据库 `termlink_recovery_20261010`；R2 容器已停机且可回滚，原正式容器和数据库未删除。R4 上的公网签名 GET 已验证：新请求包含 enhanced 能力，`LegacyCommands` 不包含 enhanced，双方 `online=true`；隔离 Registry `final-r2` 为 Connected。Android command exact mapping/GET 请求头修复经 3/3 测试和模拟器安装验证后，设备页已恢复。最新真实 Broker 尝试仍返回 `SESSION_OPEN_TL_H130`：Android `SUBSCRIBE` 与 `HOST_HISTORY_REQUEST` 漏传 `host_history_v1`、`terminal_broker_captures_v1` 能力，transport 正在修复。此前 `SESSION_UNAVAILABLE` 与本次失败均不可计为 Broker、协议 smoke、冻结历史或完整 Android→Relay→Windows 通过。

本轮 C 盘临时目录曾耗尽，造成 Clippy 链接失败；将构建命令的 `TEMP/TMP` 局部指向 `D:\Dev\Env\temp-termlink` 后 Clippy 与五库编译通过。约 450 MiB 生成 fixture 已迁移到 D 盘并核验哈希；Docker 限定重启后 CLI 恢复。C 盘再次降至约 7 MiB 后，将闲置 Docker updater 的 323348400 字节文件迁往 D 盘并核验，空余回升至约 329 MiB；仍须留意空间，未改机器级环境变量。

Windows Desktop 独立 publish 曾漏带 native DLL，现已在 Desktop 的 Windows target 补入锁定 DLL 的输出/发布复制声明；`final-r2` 三组件 DEV publish 与 DLL 哈希核验已完成，未靠手工复制补齐。

## 发布判断

**22 组整体任务尚未完成，当前不具备全量发布资格。** 只有本机隔离 DEV 服务端已部署；原正式 Windows 客户端未被替换，未提交、推送、上传或正式发布任何仓库及制品。本轮仅验收 Relay Only，LAN/NAT 不适用。逐组门禁见 [产品验收矩阵](PRODUCT_ACCEPTANCE_MATRIX_20261010.md)。

后续更新本页时须区分源码实现、单元/绑定验证、真实产品链路和正式发布，不能用其中一项勾选其余项。
