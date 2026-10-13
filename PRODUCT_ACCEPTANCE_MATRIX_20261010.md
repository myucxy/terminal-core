# TermLink 终端能力产品验收矩阵（2026-10-10）

本文把固定范围 C01–C14、H01–H04、E02–E04、G01（共 22 组）压缩成一张可执行的验收表。实现指针以当前工作区源码为准；未写路径的 Rust 文件均位于 `crates/termlink-terminal-core/src/`，同列应用文件沿用前一个完整目录。“核心已证实”只表示本地 core/FFI/绑定层已有证据，不等于 Android→Relay→Windows 的产品链路通过。

## 2026-10-11 安装与续测补充

本节覆盖下面旧批次的安装和失败状态；完整验收仍按矩阵逐项完成。

### r67–r98 续测结果（2026-10-11）

- r75 Android Debug DEV 已安装到隔离 `emulator-5556`，版本 **0.3.3/132**，保留既有数据覆盖安装。`core-network=218`、`core-crypto=37`、`feature-history=42`、`app=445`，合计 **742 项，0 失败、0 错误、0 跳过**；APK SHA-256 为 `af4802793b93aac5918cbf7bdf8a77750aeec34c1dec9ef5df32049131779ac4`，四 ABI 均 `lock_matches=true`。证据为应用仓 `../termlink/artifacts/validation/release-acceptance-20261011-dev-android-r75.json` 与 `../termlink/artifacts/validation/release-acceptance-20261011-android-r75-test-results/`。该批仍标记 `product_relay_verified=false`；组件/JVM、入包和安装通过不能关闭 Android→Relay→Windows 产品门禁。
- r73 与 r94 的 Windows 三组件均为 **0.3.3.132 framework-dependent raw DEV**，native FFI 哈希与 K2 一致；产物和哈希分别记录在 `../termlink/artifacts/validation/release-acceptance-20261011-dev-windows-r73.json` 与 `../termlink/artifacts/validation/release-acceptance-20261011-dev-windows-r94.json`。两批均 `production_installer_built=false`、`ordinary_agent_restarted=false`、`appship_uploaded=false`，不能写成已替换普通客户端或已发布。
- r87 Windows 原生窗口实窗探针完整通过：真实 child/ConPTY、authority、Avalonia 的几何、主题查询/订阅/切换/退订、文本/Enter、手机 fit/restore、程序 resize 顺序和拒绝结果均通过；日志为 `../termlink/artifacts/validation/release-acceptance-20261011-uia-r87.log`。这是 **W**（Windows 本机）证据，不是 Android Relay 闭环，不能关闭 C01/E04。
- r85 与 r96 的 Broker 运行均已完成基础 9 组语义阶段（C03、C04、C06、C09、C10、C11、C12、C13、C14），每组有截图/semantic probe，Snapshot engine recreation 结果为 semantic equal。r85 后续在 `protocol-extended-c05-soft-pre` 以 `REQUEST_INVALID` 停止；r96 的扩展阶段计数为 **18/24**，止于 C12 DRCS overwrite 的空定义 fixture 误用（`EXTENDED_C12_OVERWRITE_GLYPH_COUNT`），不能按 JSON 中已写出的后续 phase 字段预记通过。r97 正在用修正夹具重跑，本矩阵不预记 r97 结果；当前可追溯进度为 `../termlink/artifacts/validation/release-acceptance-20261011-progress-r98.json`（`run_id=r96`、`release_gates_complete=false`）。
- r91 隔离 Agent 升级时，约 **277 字符**的历史文件路径在文件存在的情况下仍返回 Win32 `CreateFileW` 错误 3，说明长路径前缀缺口在产品文件打开路径仍可触发。加入 `Common.PrivateHistoryDirectory` 的 extended-DOS prefix 后，r93 历史验证通过 **72 assertions**，覆盖继承 ACL、迁移、长路径幂等、reparse/hard-link 防护、真实 `CreateFileW` 允许/拒绝和历史字节保持；日志为 `../termlink/artifacts/validation/release-acceptance-20261011-history-r93.log`。这关闭的是 Windows 本机历史目录子项，不代表 E02/E03 的 Relay 恢复完成。
- r95 隔离 Agent 已成功启动，实际 PID **4836**，并核对了独立 binary root、pipe 和测试身份；r96 以该隔离 Agent 作为 Windows 端开始真实 Broker Relay run。日志中出现 peer-ACK 诊断/解析和 request release 记录，但当前仍是 Broker/Delta 夹具的准备证据；**真实 Delta 多片 Android→Relay→Windows 恢复和 Windows peer ACK 尚未运行**，不能关闭 E03。相关隔离日志位于 `../termlink/artifacts/validation/release-acceptance-20261011-isolated-r59/`。
- r67–r98 期间已接入 peer-ACK 诊断、parser 和 runner 断言，保留 request/session/source sequence 关联；这些改动只提高可观测性，不能把 Android 已发送 ACK 当作 Windows 已收到 ACK。r98 进度文件仍报告完整 run failed，失败码为 `EXTENDED_C12_OVERWRITE_GLYPH_COUNT`，长路径 Agent restart 已验证但 release gates 未完成。
- 当前 DEV 基线仍是 **0.3.3/132**。本批没有正式版本号、签名 Setup、clean core 导出、对应源码交付、AppShip 上传或普通 Agent 替换；core 仍按 dirty DEV 口径处理。以上 r67–r98 只补充证据，不改变下方 22 行的最终通过状态，完整 **22 组产品门禁仍开放**。

### r49–r63 续测结果（2026-10-11）

- r56 已通过正常 Android UI 恢复登录，精确 probe 的 `account_ready=true`；随后 r59 正常注册独立验收账号，r60 在 `.recovery` 登录该账号且 probe 再次通过。独立 Agent 的 accountKey、machineId、PC ID 均与普通客户端不同，不复制普通凭据、不并发抢占普通 PC 身份。凭据仅保存在本机 ignored 私有目录并用 DPAPI 保护；普通 Agent 与用户 Host 未重启。
- r55 原生 Frozen 捕获预算通过：32×24、1400 条长逻辑行、34 页、8151 行、260832 cells、408 个逻辑行，核算 41130272 bytes，最终 `head_truncated=true`。保留 128MiB/256 页生产上限，未提高额度。r54 首次执行用了错误 TFM 路径，未成功启动；r55 才是实际运行。此结果为 native-only，不是 Relay 生命周期通过。
- r58 实际 Windows→Relay→Android Broker 图像 6804 像素、capture=1、ACK 49→50、Host 正常退出 0；首个 Unicode 语义阶段失败，实际 `LEGACY_V1`、预期 `UNICODE_16`。属于产品宽度策略接线缺口，正在新增 `terminal_state_2_22_v2;width=2;output=0;resources=2`，保留 v1 含义和旧会话恢复，不放宽验收。失败代码已修为准确的 `SEMANTIC_UNICODE_WIDTH_PROFILE`；新版双端与密封对象接线尚待统一构建和重测。
- r53/r57 真实 32×20 resize/restore 子项通过，随后主题订阅切换失败，不能将整个窗口探针记为通过。新夹具改为标准 Windows UI Automation 查询真实按钮 bounds，保留实际 SendInput 与 HWND/PID/焦点保护，删除固定坐标和反射加载；r63 WindowsDesktop 测试项目 restore 成功，尚待 build/run。
- r61 Frozen 生命周期因缺失迁移后的 AVD_HOME 未启动。r62 设置 D 盘 AVD 后冷启动成功、登录与隔离 Agent Connected 前置条件通过，但 Android Debug arm 拒绝新 1400 行/短 marker fixture，返回 `REQUEST_INVALID`，未启动测试 Host。正在给显式 lifecycle 模式补严格校验及 arm 失败快速报告；该批尚无 TTL/重连/RELEASE 的新 Relay 证据。

完整 **22 组产品门禁仍开放**。已解除登录阻塞，但新增 Unicode profile 和 Debug fixture 修复尚待真实链路重跑；native 预算、窗口子项和脚本自检不替代完整产品验收。未正式选号、签名 Setup、提交/推送、clean core 导出、对应源码交付或 AppShip 上传。

### r41–r48 续测结果（2026-10-11）

- C01/E04 的 r41 实窗回归通过，包含实际几何、主题订阅/菜单切换/同色静默/退订、窗口文本输入以及手机 fit/restore。C01 的 r42 使用真实 PS7/Claude 启动，捕获真实 `CSI 16t`，authority 保持存活，中断 Claude 后 shell 仍可输入，最终正常退出 0；未发送 AI 生成请求。
- C07/C08 的 r44 新窗口探针使用生产 Desktop、SessionHost 和实际 ConPTY 子进程。`queries` 与 `state` 分别读取 166/222 字节，经既有 `Assert-TerminalReplyProbe.ps1` 逐字节通过，覆盖固定查询、重复/分块查询、状态修改与恢复及取消负例。原始证据路径记录在应用仓 `artifacts/validation/release-acceptance-20261011-protocol-window-r44.json`。这是本机 authority→子进程 stdin 证据，不是 Android Relay 验收；未读取内层 Host 的退出码，字段保留 null，不把清理期望值 0 当作实测。
- E02 修复未完整 ACK 的 Frozen BEGIN 在断线、取消或超时后遗留捕获的问题。完整 ACK 后在 lease 内标记已交付；同 ID 的失败重试只清理未交付捕获，不误删此前 READY 捕获。显式 CANCEL 保留原有语义，PAGE 取消不清理 BEGIN，也不在 Relay 断线时统一清理所有捕获。r45 全套历史回归通过，Frozen **68 项**，含真实 ACK 原语、未交付取消/超时、同 ID 重试、session/recipient 隔离及临近期限标记不续租；Runtime finally 的真实 Relay 接线仍待验收。
- r46 AgentUpdateValidation 编译零警告/错误并运行通过，涵盖登录 generation、撤销轮换、PC 保留、非撤销 401、诊断和更新 API/build/idempotency。
- Windows 最新三组件已 publish 为 **0.3.3.132** 的 framework-dependent raw DEV，目录为应用仓 `artifacts/validation/release-acceptance-20261011-windows-dev-r47/`，DLL 与 K2 匹配，包含 E02 新修复。版本和哈希记录在 `release-acceptance-20261011-dev-windows-r47.json`；未生成本批签名 Setup，也未替换普通后台。
- r48 已将 `.recovery` 的中文登录页面显示在 `emulator-5556`，精确 probe request `9dfb401b2e604b0fbe03ce07286f5363` 仍为 `account_ready=false`，origin 正确。已请求恢复模拟器登录；没有复制 Windows token/设备身份或并发启动共享 PC 身份的旧测试 Agent。

完整 **22 组产品门禁仍开放**。本批新增本机子项通过和 E02 修复，不把组件测试、窗口探针或旧 smoke 算作全链路通过；未正式选号、提交/推送、clean core 导出、对应源码交付或 AppShip 上传。

### r33–r40 续测结果（2026-10-11）

- E04 实窗 r33 完整通过；包括主题菜单实际切换、同色静默、退订、最终查询和 journal 顺序，及原有几何/文本/手机 fit/restore。C07/C08 的 Relay child 随后新增逐字节固定查询、重复 DA3、分片查询、状态修改/恢复及取消负例，r38 已重新编译通过；这些新增阶段尚未实际经过 Android Relay。
- 旧历史迁移在移动目录前复用 `PrivateHistoryDirectory.Prepare` 校验路径、链接和 owner 并收紧 ACL。r34 的真实迁移测试确认原先继承 Users 写权限的隔离旧树迁移后为 private，历史 marker 字节不变。r38 最新 Runtime 的全套历史回归再次通过（Frozen 52 项、真实受限 token、Delta、WAL/sidecar 与 outbox）；受限 token 仍不等于另一真实 Windows 用户。
- C02 在既有 recipient-specific E2EE 实时 Ended payload 增加可选诊断正文（field 10），仅退出码 125 发送。Windows 从相应 session 的 DPAPI authority 文件读取有界转义正文，不发送本地 header/路径、input-only 证据、HTTP 元数据或历史事件。Android 严格解析后保留 Exited 和最终 ACK 语义，unsupported/malformed/limit_exceeded 分别显示 TL-H122/TL-H126/TL-H127；无字段及无效正文仍正常结束。r38 Windows AgentUpdateValidation 含真实 DPAPI 正反例通过；真实 Relay 的错误详情可见性尚未验收。
- Android r37 构建成功，API 50、session 56、network 408、app 437，共 **951 项，零失败/错误/跳过**。APK SHA-256 `4befac69b5358b62b6c929aeca88b0913e74bf5b94d6c2a005a67af2ad1532f2`；四 ABI SO 与 K2 匹配，已保留数据覆盖安装 `.recovery` **0.3.3/132**。证据为应用仓 `artifacts/validation/release-acceptance-20261011-dev-android-r37.json` 和对应构建日志。
- r38 SessionHostValidation 全套通过，包含真实 ConPTY 输入/鼠标、Host durable ACK、Kitty file/temp/shared-memory、生命周期及窗口调整。H02 真实 Host 退出回调已集成父 runner，r36 Effects/Broker/Extended/Reply 自检通过，尚未执行真实 Android 退出音频回录。
- 新 Windows 三组件已按 **0.3.3.132** publish 到 `artifacts/validation/release-acceptance-20261011-windows-dev-r40/`，native DLL 与 K2 一致。这是依赖本机 .NET 的 raw DEV 候选，未生成签名 Setup 或替换运行中的普通后台。r39 首次误用尚未 restore 的 win-x64 self-contained target，被 NETSDK1047 拒绝；r40 按既有 framework-dependent DEV 方式修正，不改生产发布参数。
- `/readyz` 和 `/v1/client-config` 实际返回 200，OIDC issuer 与 Web redirect origin 均为 `termlink2`；Android 保持注册的 custom-scheme 回调，Windows 保持 loopback 回调。r40 前置 probe 仍 `account_ready=false`；当前等待模拟器登录恢复。旧隔离测试配置与普通客户端共用 PC/machine/account 身份，未并发启动该配置以免抢占连接。普通 Agent PID 7996 和用户 Host PID 39732 的启动时间保持不变。

完整 **22 组产品门禁仍开放**，上述通过仅覆盖注明的子项；当前不能将候选构建、自检或本地回归当作全链路通过。core 仍为 dirty DEV，未正式选号、提交/推送、对应源码交付或 AppShip 上传。

### 完整范围续测 r15–r24（2026-10-11）

- Windows Agent/SessionHost/桌面程序已按 **0.3.3.132** publish 到应用仓 `artifacts/validation/release-acceptance-20261011-windows-dev-r15/`；Agent 包含 completion 持久化失败修复，桌面 r18 包含当前窗口主题菜单和窄窗换行。这是隔离 raw DEV 目录，尚未生成本批签名 Setup、替换普通后台或正式发布。Desktop 初次 restore 卡住的 r15 日志保留，之后 `--no-restore` 成功；只终止了核对过身份的本次构建进程。
- 复用 `ProductionTerminalHistoryValidation/FixtureSnapshotInspection.cs` 的独立 snapshot 检查器；r15 build 和 `--snapshot-inspector-selfcheck` 通过。它复制本 run 指定 session 的文件到临时私有目录，核对源文件前后哈希，再输出 base/target plaintext SHA-256 与 sequence，不在生产目录构造可写 Store。此自检不代表真实 Delta 恢复通过。
- E04 新增真实 Windows 原生窗口主题选择测试。r23 已实际切换并收到首次主题回复；“同色重复选择”检查原来仅凭 console input handle 被唤醒就判重复回复，没有读取字符，不能据此认定产品双发。已改为有界读取真实 INPUT_RECORD，忽略焦点/鼠标/无字符记录，真实字符仍报错。DPI、命中 PID 和焦点保护保留；r17–r23 的 UI 自动化失败日志保留，尚未取得该新版测试完整通过证据。
- H01 新增独立交互 PS7 Relay runner，使用真实 `-NoLogo -NoProfile` 和 managed-shell 自动注入。Debug inspect 只输出规范 Windows cwd 的 SHA-256、最近 8 条命令阶段/退出码/锚点，有 32KiB 上限，不输出原路径或命令正文。r24 Shell 脚本正反例自检通过，Android 新 Debug 构建与真实 Relay 尚待完成，不能当作 H01 产品验收。
- E03 新增 Delta 多片 runner，要求独立 Windows previous/current 与 Android base/target 一致，以及真实至少两片、实际接受/ACK 发送计数。r24 Broker 自检通过；复核发现主 runner 已关闭视图而 Restore helper 又 inspect/close 的编排冲突，正在修正，尚未真实运行。ACK 发送完成仍不等于 Windows 已收到 ACK。
- H02 Windows 内置 WASAPI loopback recorder 的 r19 格式自检/build 通过。r18 实际回录已知 997Hz/500ms 校准音通过，完整 6 秒 288000 帧；`audio-calibration-r18/calibration.json` 为 `calibrated`，但 **product_sound_verified=false**。DirectShow ASIOVADPRO 旧采集无法录到系统声音，不作为产品声音证据；BEL/DECPS、开关及退场静音仍需实际验收。
- `accept-20261011-semantic-r20` 真实 Broker 前置检查失败于 **ANDROID_ORIGIN_OR_ACCOUNT_MISMATCH**，session_id=null，未创建测试 Host。目标 `.recovery` 的精确 probe 仍为 `account_ready=false`、origin=`https://termlink2.kget.cc:8910`；等待恢复测试账号登录，不把该失败计为协议测试通过。

本批没有关闭完整 22 组门禁；core 仍为 dirty DEV，未提交/推送、正式选号、对应源码交付或 AppShip 上传。普通 Windows Agent 与用户 SessionHost 仍保留，测试 runner 只清理自己创建的隔离实例。

### r24–r25 复测结果（2026-10-11）

- E04 Windows 实窗 r24 **完整通过**：真实 child/ConPTY 的主题查询、订阅、实际菜单切换、同色不重复回复、退订后无回复、最终查询及 HostTheme journal 数量/顺序；同时保留物理几何、真实文本/Enter、手机 fit/restore、有序程序 resize 和拒绝结果。build 零警告/错误，日志为应用仓 `release-acceptance-20261011-native-window-r24{,-build}.log`。r23 是测试把非字符 INPUT_RECORD 当作回复的误报，未修改生产主题逻辑；此证据仍不替代 Android Relay 重连/不同主题 authority 验收。
- H01 Debug 摘要已进入最新 Android DEV；r25 App **436 项，零失败/错误/跳过**，APK 原地覆盖安装并核对 **0.3.3/132** 和四 ABI K2 哈希。APK SHA-256 `0c8a981aa15e9616dc0a97d12ebee78dbd6f1dcfe2fad81eb73e4efe7651ceeb`，完整记录 `release-acceptance-20261011-dev-android-r25.json`。r24 首次 Gradle 参数被 PowerShell 拆分造成 `.3.3` task 错误，r25 用字符串数组修正；失败日志保留，不计产品失败。
- 覆盖安装后的精确 probe request `8fd2eaa14a3f4ebaa80a9a63f4d7ec9e` 仍为 `account_ready=false`、origin=`https://termlink2.kget.cc:8910`；等待恢复模拟器测试账号。普通 Windows Agent/用户 Host 没有重启，未发布。

### r26–r32 续测结果（2026-10-11）

- E03 Delta 编排不再重复 inspect/close：Broker 保存有界、绑定 run/session/baseline 的 `delta-preclosed.json`，Restore helper 校验后直接 reopen/inspect；WAL 模式拒绝使用这个入口。r26 Broker/Restore 正反例与自检通过。仍要求真实至少两片及独立 Windows base/target SHA-256；没有新的真实 Relay 结果，ACK 发送不能当作 Windows 已收到 ACK。
- E04 Broker runner 已接入六个主题阶段，包含实际 Windows 菜单切换、同色静默、退订后恢复及最终查询。r30 脚本正反例通过。r31 测试夹具的 `Math.Max` 重载歧义已通过显式整数类型修正；r32 SessionHostValidation 编译通过，零警告/错误。新阶段还未经过 Android Relay，不能据此关闭 E04。
- H02 Windows **生产音效实际回录 r32 通过**，不是校准音替代：真实 `NativeTerminalAlerts.Offer/Play` 的 BEL 523 Hz、DECPS 523/659/784 Hz 顺序、重复回执/3 秒限流、静音、Dispose 停声及禁止排队均有频谱与时段断言。14 秒 WAV 为 48000 Hz 双声道 float32，672000/672000 帧；证据目录为应用仓 `artifacts/validation/terminal-audio-e006ec55d89540738d356a724a234751/`，主结果在 `release-acceptance-20261011-windows-audio-r32.log`。
- 该采集保留 1 条 discontinuity 原始记录：仅首包、FrameOffset=0、PacketFrames=480、Flags=1、FirstPacket=true，明细未截断。判据要求完整帧数且只允许这一启动边界；后续断续、缺明细、截断或缺帧仍拒绝。相关正反例自检通过，不将结果描述为“全程零断续”。Android Effects runner 已采用同一判据；Android 实际声音及真实 Host 退出后停声仍未验收。
- r33 前置精确 probe request `79f5022360ce40068fed218a71a7c752` 仍为 `account_ready=false`，origin 正确；真实 Relay 续测等待模拟器恢复登录。普通 Windows 后台及用户会话未替换，未正式发布。

完整 22 组门禁仍开放；自检、Windows 原生窗口与生产音效回录仅关闭各自覆盖的子项。core 仍为 dirty DEV，尚未固定正式版本、交付对应源码或上传 AppShip。

### 完整范围续测 r11–r14（2026-10-11）

- Android Host History `CHUNK_ACK` 已绑定接收响应的物理连接 generation；新测试在发送锁被占用时切换 socket generation，确认旧 ACK 不会发送到新连接、ACK 计数不增长且请求缓冲归零。network **407 项，零失败/错误/跳过**，日志为应用仓 `artifacts/validation/release-acceptance-20261011-android-ack-r11.log`。App 435 项沿用 r9 证据，不记为本批新运行。
- 最新 `.recovery` DEV **0.3.3/132** 原地覆盖安装，APK SHA-256 `87746a4f8612fe28e4c881c6315aab61061b36ad344e7ecb3a85d3ece6f76ab0`，四 ABI SO 与 K2 一致；精确版本和入包哈希见 `release-acceptance-20261011-dev-android-r11.json`。未卸载目标包，当前账号仍未恢复，不能执行依赖登录的真实 Relay 验收。
- Windows completion 登记写盘失败现在通过 `finally` 清理运行槽；保留 completed wrapper 供恢复/释放重试，诊断在释放锁后写入。r13 build 与定向 completion 测试通过，r14 **profile 全套及 122 项持久化断言通过**，含真实 IPC、Host 丢失/受控停止、持久输入、剪贴板授权与 lease；日志为 `release-acceptance-20261011-profile-r14.log`。这些不是新的 Android Relay 证据。
- r12 Graphics handshake 失败来自旧夹具目录含另一 SessionId 的 journal，已隔离夹具并补跨 SessionId 拒绝负例；r13 profile 子进程失败来自用 `dotnet DLL` 启动自启动夹具，r14 改为直接执行 Validation.exe 后通过。失败日志保留，不再视为当前未修产品错误。
- 真实 Broker WAL 故障脚本已接入主 runner：仅给本 run 新 session 的精确下一 WAL 路径放置私有空目录 blocker，要求 sidecar 已落盘且 Android ACK 不前进，再移除自身 blocker 并等待同源序号自动重试成功。r13 主/子脚本正反例自检通过；尚未真实执行，不能宣称 WAL Relay 恢复通过。
- H03 脚本已补明确只读时拒绝 OSC52、恢复控制后剪贴板保持，并尽力恢复控制，r12 自检通过；尚待真实 UI 执行。H01 自动注入、H02 实际声音、E04 主动主题切换、Frozen 并发/重连与 Delta 多片/独立哈希仍未关闭。

本批尚未替换运行中的普通 Windows Agent；旧 r6 DEV 候选不含 completion 新修复。未正式选号、提交/推送、clean core 导出或上传 AppShip；K2 仍为 dirty DEV 产物。脚本自检、本地 IPC 和组件回归均不替代 22 组完整产品门禁。

### 完整范围续测 r8–r10（2026-10-11）

- Android DEV `.recovery` `0.3.3/132` 已覆盖安装，APK SHA-256 `0e775e3db80a1c851487e4d4a03d4bc883eef7b3f5059be0c60284d9218d7eea`；四 ABI 库与 K2 一致。App 435 项、network 406 项零失败/错误/跳过。构建证据为应用仓 `artifacts/validation/release-acceptance-20261011-android-transfer-r9.log`，版本/入包库证据为 `release-acceptance-20261011-dev-android-r9.json`。r8 的 Gradle 参数解析失败保留，不计为产品失败或通过。
- 恢复诊断新增按实际 Host History requestId 关联的新片、重复片和 ACK 发送完成数；Delta 与 Broker 的预期分片数仍另列。ACK 发送完成不等于 Windows 已收到，也不替代本地持久安装。新增 SQLite 写入失败→无缓存/预算泄漏→相同引用重试成功测试已进入上述 App 回归；乱序/重复和 ACK 发送失败计数已进入 network 回归。
- Windows `release-acceptance-20261011-windows-history-r9.log` 全套通过，包含 Frozen 52 项、Delta 错基线/淘汰/损坏、Broker sidecar 已落盘但真实 WAL 写入失败后的恢复及顺序/重复/冲突、私有目录与受限令牌权限校验。r8 的 Broker 故障注入测试用了仅覆盖删除标记的委托，未触发 WAL 失败；已改为真实文件系统目标冲突，失败日志保留。受限令牌校验不代表另一真实 Windows 用户身份，隔离故障测试不代表真实 Relay 中途故障通过。
- C01/H01 Windows 实际子进程证据：`release-acceptance-20261011-powershell-shell-r8.log` 验证真实 PS7/ConPTY 中文与空格路径、prompt/input/output/end、成功/失败、切换目录；`release-acceptance-20261011-native-window-r10.log` 验证原生窗口输入、物理几何、手机 fit/restore、程序有序 resize 和拒绝。r9 窗口测试误把新增 `input_failure` 字段当异常，已修正旧断言并复跑；它不是产品退出失败。这些证据不替代 Android→Relay→Windows 相应门禁。
- 先前默认跳过的 JNI 性能探针已用手动仪表 `-e termlink_perf true` 实际执行，`release-acceptance-20261011-history-performance-r10.log` 为 `OK (1 test)`，333 次分块共 5200 行，滚动历史上限 2000 行，耗时 13.227 秒。入包 x86_64 SO 已核对 K2；仅安装 library test APK，未卸载 `.recovery`。它验证真实 JNI feed/render 与有界历史，不宣称真机速度阈值、OOM 或真实 Relay 压力通过。
- Broker runner 已接 C05 扩展、语义重建、Kitty 三来源与修改后持久恢复、真实滚轮、主题查询/订阅/退订及系统效果。普通成功必须 Host 正常退出 0，诊断 fail-closed 仅在独立 run 预期退出 125；强制终止、系统效果 `partial` 不可算完整通过。脚本自检均通过，真实运行仍待模拟器登录恢复。
- H02 权限拒绝/允许和 H03 历史遮盖脚本已补齐；H01 自动注入的 Android 可见结果、H02 实际声音及 H03 控制权相关真实场景仍须联测。E02 已补旧 Host 拒绝后仍可实时输入的门禁。真实多片 Delta、冻结并发/重连及 G01 WAL/压力门禁尚未关闭，不宣称 22 组全部验收或可以正式发布。

当前可见模拟器仍在 `termlink2` 登录页；只做覆盖安装与手动仪表，不卸载 `.recovery`。本批未正式选号、提交/推送、clean core 导出或上传 AppShip。

### 全范围继续验收批次（2026-10-11）

- 最新 Android DEV `0.3.3/132` 已重新安装到 `emulator-5556` 的 `.recovery`；APK SHA-256 `54b72eb89ee897c4bc68a4e3005bc28d9121ae0025ced5d95595f130dab6e05b`，四种 ABI 与 K2 锁文件一致。App JVM 431 项、native JVM 203 项（原有跳过 2 项）、session 55 项、network 406 项零失败；模拟器 native 仪表 61 项与 app 仪表 4 项通过。Gradle connected 仪表任务在收尾自动卸载了目标 App，原登录未保留；重装后当前等待恢复 admin 登录。后续改为手动安装仪表 APK 和 `am instrument`，不再使用会卸载目标 App 的 connected 任务。证据在应用仓 `artifacts/validation/release-acceptance-20261011-{app-full-r5,native-control-r4,instrumented-r5}.log`。
- Windows 图形启动 14 个真实子进程/故障场景通过；local-only 现在准确报告远程访问停止，不能再记成普通超时。私有历史 ACL 的生产初始化/迁移修复和账户撤销重新授权修复已实现，历史校验与账户持久化竞态回归通过；候选三组件 `0.3.3.132` 已编译，尚未替换当前后台。安装与真实撤销联测仍未关闭。证据为 `release-acceptance-20261011-{graphics-startup-probe-r5,windows-history,account-race-r2}.log`。
- `accept-20261011-warm-ime-r2` 已证明非冷启动且 IME 展开时，真实 Relay 图片由 0 到 6804 个目标像素，最终 ACK 60，Host 退出 0；这替代“该视口问题尚无成功回归”的旧状态，不替代 G01 完整恢复/WAL/压力门禁。
- 已新增 9 组 native frame 语义断言及真实 ConPTY 回复采集器，并新增冻结期间实时输出和 Relay 重连脚本；脚本自检通过，真实链路逐组执行中，不能把自检当产品验收通过。新增 Debug 重开视图、Delta 恢复统计和冻结页摘要仅用于验收；分片预期值不是线上逐片 ACK。
- `accept-20261011-semantic-r5` 在前置健康检查失败，原因是普通 Windows 后台 `ACCOUNT_REAUTH_REQUIRED`，尚未创建测试会话。已通过现有网页授权流程重新认证并继续验证；失败记录保留。
- `accept-20261011-semantic-r6` 已通过 Windows 前置检查，但目标 App 被上述仪表收尾卸载，返回 `ANDROID_ACTIVITY_UNAVAILABLE`，尚未创建会话。账户 best-effort 凭据损坏回归已通过 `release-acceptance-20261011-account-race-r3.log`；这不能替代真实撤销/重新授权验收。

本批仍未正式提交/推送、选号或上传 AppShip；K2 仍为 dirty DEV 产物。下表“尚未运行”记录属于逐项待关闭门禁，不能据新增脚本或本地测试数量宣称 22 组全部完成。

- Windows 签名 DEV Setup 0.3.3.132 已安装至 `D:\Dev\Tool\TermLink`，退出 0；APK `63E954339F2BF9F6081FE43B1658D5A9701A894AABC41B09726895C2483BF3E3` 已原地安装到可见 `.recovery` 模拟器，0.3.3/132，四种 ABI 与 K2 一致。安装后的 Windows DLL 已验证仅增加 PE 签名，不能用签名前哈希直接比较签名后文件。
- 控制权切换不再清除已认证的 terminal profile；network 400/400 通过。`r4frozen8` 三 RPC 已通过，末尾失败属于测试脚本向未读取的 Host stdin 输入；改为安卓实际键盘后，`r4frozen9` **21/21 断言通过**，BEGIN/两次 PAGE/RELEASE 中 ACK 固定 34，释放后手机输入和 Windows 输出正常，ACK=75。并发输出、过期/截断、取消及故障矩阵仍未完成。
- `r4installedcold` Broker 真实 Relay 图像像素 6804、source seq 39、最终 ACK 50；9 组 smoke 通过，C12 新增字形像素 1050。未冷启动的 `r4installed` ACK 正常但图像不可见，仍需界面状态相关专项回归。证据分别在应用仓 `artifacts/broker-product-e2e/` 对应 run 的 `evidence.json`，不能仅记录后一次成功。
- 普通 admin/termlink2 后台旧设备身份被服务器撤销；本机已保护备份并迁入同账号有效测试身份，重新登录后 Relay Connected、设备 GET 成功和 HOME-PC online。服务端撤销记录未恢复，本地会话历史未清理；新设备恢复的产品逻辑尚未修复。公网 client-config/OIDC URL 一致，Docker 管理 inspect 超时，未修改 Docker 部署。
- 普通后台 `r4localadmin` 因本机历史目录继承其他 SID 的写权限被 Broker 拒绝，Host exit 125。备份原 SDDL 后，已仅对五个 TermLink 私有目录收紧权限，保留当前用户/SYSTEM/Administrators，历史文件未删除。`r4localprivate` 真实普通后台 Relay 重跑通过：图像 6804 像素、source seq 39、ACK 50、9 组 smoke 和 C12 1050 字形像素，Host exit 0。私有目录初始化/迁移 ACL 的通用产品修复仍待实现；本机恢复不能代替它。

## 证据口径

- **K1 核心基线**：最新 core 754 项、golden 10 项、FFI 71 项（共 835 项）通过，Rustfmt 与 Clippy `-D warnings` 通过，包含独立 `TL-INPUT-DIAG/1` 同调用输入诊断、无图像 V5 快路径、任意视口 DRCS 及历史逻辑行 head 的 HashSet 查重回归。完整历史校验未裁剪。日志为应用仓 `artifacts/validation/recovery-core-20261010-history-validation-fastpath-{test,clippy}.log`。
- **K2 原生导出**：包含 K1 修复的五个平台库已重新导出，5/5 校验通过；DLL 哈希前缀为 `c2030d122ea31266`，x86_64 SO 为 `b2e49bc494de1595`，精确哈希见应用锁文件。JNI 四个输入诊断方法已补注册，导出脚本校验构建前后源码内容一致。Windows `final-r4` 与 Android 新 DEV 均重新打包、校验并安装到隔离测试实例，真实链路仍须重跑；是 DEV/dirty 产物，不能作为正式发行库。下方 W1/A1 为历史批次证据，不能视为 K2 的完整产品测试。
- **W1 Windows**：`final-r2` 的 Agent、SessionHost、Desktop 三组件 DEV `0.3.3.132` 已独立 publish，DLL 与 K2 哈希一致。NativeTerminalCore、SessionHost 全量、FileOperations、TerminalSurface、Desktop.Headless、WindowsAdapter、Heartbeat 兼容回归通过；隔离 Registry `final-r2` 为 Connected。真实 ConPTY BridgeInput 故障注入后，测试 Agent 管道收到 `Running(5) → Ended(8, 125) → Removed(9)`；此管道是测试替身，未覆盖完整 Relay。前批共享内存 real Job、非零 O/S 与真实 Claude 启动/退出通过，不能代替真实 Relay 证据。
- **A1 Android**：前批 SO 的模拟器 JNI 59/59、S7 产品 2/2、H02 真 Keystore 仪表 2/2、最新 network 399/399、API 49/49、native JVM 201 项通过/2 项跳过、app 定向 54 项通过。command exact mapping/GET 请求头测试 3/3 通过，安装后模拟器设备页恢复显示；SUBSCRIBE 与 negotiated Host History 请求已补 Broker/Host History 能力。V5 快路径 APK 已原地安装，包含 Frozen 操作且入包 SO 与 K2 一致；新 SO 尚须完整重跑 JNI/产品回归。上述模拟器/模块证据不能代替真实链路或真机验收。
- **R4 Relay**：Go 全库 32 包中 1077 个测试/子测试通过、57 项因环境条件跳过、零失败；额外 PostgreSQL/Redis 空库集成 3/3 仍通过。本地 R4 DEV 镜像 ID 为 `sha256:5b144d79599b4c55abd39d947a800f221dcd2a01f2a350f5c78763511ae46fb4`，已部署至隔离容器，沿用克隆库 `termlink_recovery_20261010`；R2 已停机可回滚。公网签名 GET 验证新请求包含 enhanced、`LegacyCommands` 不含 enhanced，双方 `online=true`；隔离 Registry `final-r2` 为 Connected。最新真实 Broker 尝试返回 `SESSION_OPEN_TL_H130`：Android `SUBSCRIBE`/`HOST_HISTORY_REQUEST` 漏传 `host_history_v1` 与 `terminal_broker_captures_v1`，transport 正在修。Broker、协议 smoke、冻结历史及完整 Android→Relay→Windows 均未通过，正式部署亦未完成。
- **G1 Graphics**：真实 Relay run `r4stage` 已验证 Windows Kitty `t=f` 捕获→Android 获取/native apply→durable ACK，magenta 像素 6804（基线 0），source seq 33、最终 ACK 43、Host 正常退出 0。C03/C04/C06/C09/C10/C11/C12/C13/C14 共 9 组仅完成 ACK/hash/截图 smoke，不等于完整语义验收；其中 C12 暴露 106×32 视口 DRCS 空白，修复已通过 K1，待新双端库/真正字形像素断言。正式客户端尚未升级。

### 最新真实链路结果与新增修复

- 上述 R4 段落末尾的 TL-H130 是历史失败；A1 能力修复后真实 Broker 已通过 G1，不再作为当前阻塞。
- `r4frozen5` 成功打开 106×32 会话，bootstrap=`REMOTE_SNAPSHOT`、live ACK=37；BEGIN 的 `FROZEN_HISTORY_FAILED` 根因是对已有 live client 的 session lane 重复注册。Android 已借用 live client、明确资源所有权，并在 live 退休路径先退休 frozen owner；FrozenHistoryBrowseTest 5/5 通过，修复已原地安装。PAGE/RELEASE 尚未验收，不能记为 E02 通过。
- 最新 `r4frozen6` 在打开阶段返回 **TL-H131**，尚未执行 frozen BEGIN。Android 等待附着回执 15 秒超时，Windows 此前约 0.4 秒完成同会话快照选择；SUBSCRIBE/attach 的关联日志不足，尚无法确定延迟位置。证据为 `artifacts/tail-first-e2e/r4frozen6/evidence.json`。TL-H130 是已解决的历史失败。
- 新 K2 双端库 `r4frozen7` 打开成功（约 55.8 秒，106×32、REMOTE_SNAPSHOT、live ACK 35、Connected/subscriber ready/surface 正常），但 BEGIN 再次失败，安全 DEV 日志定位为 `phase=profile`、`awaitTerminalProfile` 的 15 秒超时。尚未发出 frozen RPC，PAGE/RELEASE 未覆盖；fixture 已正常清理，模拟器保留。证据为 `artifacts/tail-first-e2e/r4frozen7/evidence.json`，正在核对 live client 的就绪和协议状态等待，不直接归因为 native 性能或把 H131 标为已消失。
- 增强 PowerShell 新建失败根因是 Windows OPEN_TERMINAL 回执漏 `terminal_command_id`。Worker 已修，含增强命令及四种原 Shell 的真实 Worker 回执测试已通过，日志为 `recovery-windows-20261010-enhanced-worker.log`。Windows `final-r4` 三组件 DEV 0.3.3/132 publish 成功、隔离 Registry 已升级并 Connected，DLL 与 K2 一致；安卓真实新建须再验。
- 当前已安装 APK 哈希为 `1EEE1CC8836E2D95955F7EB1227719FAEA4DD9BA2EC0A3692180915B8A680979`，四种 ABI SO 均与 K2 一致，包含 DRCS 和 frozen 所有权修复，版本 0.3.3/132、独立 `.recovery` 包、登录保留。
- 性能探针旧 SO 的 5200 行基线为 feed 约 9.2 秒、render 约 5.5 秒；DRCS/V5 快路径 SO 为 feed **8704.33 ms**、render **5179.15 ms**；K2 HashSet 库为 feed **8311.95 ms**、render **4649.10 ms**，1/1 通过，分别减少约 4.5%/10.2%。仅单次模拟器对照，改善有限；Broker 初次打开仍约 37.7 秒，性能门禁未完成。

当前仅验收 **Relay Only**；LAN/NAT/direct 矩阵不适用。只有上述本机隔离 DEV 服务端已部署；没有 Docker Hub 推送、正式发布或客户端替换结论。

## 22 组矩阵

| ID | 实现指针（相对本仓库；应用指针用 `../termlink/`） | 当前已证实 | 尚未运行的真实门禁 |
|---|---|---|---|
| C01 | `crates/termlink-terminal-core/src/host_geometry.rs`, `query_reply.rs`, `lib.rs`, `host_geometry_tests.rs`; `../termlink/win-agent/src/TerminalCore/NativeTerminalHostGeometryV1.cs` | 几何查询/尺寸 profile 的 core 与 FFI 路径包含在 K1，五库为 K2。 | 真实 PS7/Claude 的 14/16/18/19t、DPI/IME/dock/resize 以及 Android renderer 经 Relay 的 authority 与顺序。 |
| C02 | `diagnostic.rs`, `query_reply.rs`, `extended_queries.rs`, `input.rs`; `crates/termlink-terminal-ffi/src/input_diagnostic.rs`, `include/termlink_terminal.h`; `../termlink/android/terminal-engine-api/src/main/kotlin/dev/termlink/terminal/api/TerminalProtocolDiagnostic.kt`, `TerminalInputDiagnostic.kt`; `../termlink/win-agent/src/AgentControl/TerminalFailureStore.cs` | 输出诊断分类、原始序列/字节位置和 fail-closed core 测试属于 K1；同调用 `TL-INPUT-DIAG/1` 精确输入诊断与五库导出属于 K1/K2；Windows 与真实 ConPTY→测试 Agent 管道回归为 W1，Android 新 SO 的 JNI 59/59 为 A1。 | 真实 Claude/PowerShell 不支持输出及输入字符/修饰键经 Relay 到 Android 的明确提示；跨帧根因保留、其他会话隔离、权限与通知相关产品回归。 |
| C03 | `unicode16.rs`, `unicode_width.rs`, `width_profile.rs`, `screen.rs`; `../termlink/android/terminal-engine-native/src/main/kotlin/dev/termlink/terminal/nativecore/NativeTranscriptMapper.kt` | Unicode/width profile 与边界测试纳入 K1。 | 真实 Unicode 16/UAX#29（旗帜、ZWJ、VS、Indic、孤立组合符）在 ConPTY、重排、复制、历史和双端渲染中的一致性。 |
| C04 | `line_rendition.rs`, `render_frame_v5.rs`, `screen.rs`; `../termlink/win-agent/src/TerminalCore/NativeTerminalRenderFrameV4.cs`, `NativeTerminalRenderFrameV5.cs`; `../termlink/android/terminal-engine-native/src/main/kotlin/dev/termlink/terminal/nativecore/RenderFrameV5Decoder.kt` | 行 rendition、V5 frame 和 core 单元/FFI 路径在 K1；图形产品增强归 G1，尚未 live。 | `ESC #3/#4/#5/#6/#8` 在真实 PTY 的双高/双宽、裁剪、滚动、resize、Snapshot 恢复及 Android/Windows 像素证据。 |
| C05 | `lib.rs`, `input.rs`, `modes.rs`, `screen.rs`; `../termlink/win-agent/src/TerminalCore/NativeTerminalTraditionalProtocol.cs` | DECSTR、ESC 6/9、CSI `?5W` 的 core 代码和本地测试纳入 K1。 | 真实主/备用屏、margin/tab/wrap、光标及输入模式 reset，确认不误删历史或重复发送输入。 |
| C06 | `sgr_stack.rs`, `lib.rs`; `../termlink/win-agent/src/TerminalCore/NativeTerminalCoreV2.cs` | SGR push/pop、别名、栈边界的 core 测试纳入 K1。 | 真实 truecolor/扩展下划线、多层/满栈、Snapshot/renderer/reset 的双端回放。 |
| C07 | `extended_queries.rs`, `query_reply.rs`, `state_reports.rs`; `../termlink/win-agent/tests/ProtocolValidation/Program.cs`, `../termlink/win-agent/tests/NativeTerminalCoreValidation/Program.cs` | 已登记查询族和真实能力声明的 core/FFI 证据纳入 K1。 | 经 Relay 的 DA/DSR/显示范围/checksum 等逐字节回复、分块归属、外层去重和权威 PTY 回复。 |
| C08 | `state_reports.rs`, `snapshot.rs`, `lib.rs`; `../termlink/win-agent/src/TerminalCore/NativeTerminalStateReports.cs` | 状态报告/恢复的 core 实现及边界测试纳入 K1。 | 真实报告→修改→恢复闭环、坏参数/截断原子拒绝，并与私有 Snapshot 分离。 |
| C09 | `pages.rs`, `page_tests.rs`, `screen.rs`; `../termlink/win-agent/tests/NativeTerminalCoreValidation/PageProtocolValidation.cs`; `../termlink/android/terminal-engine-native/src/main/kotlin/dev/termlink/terminal/nativecore/NativeFrozenHistoryViewport.kt` | 页状态、页协议及有界 page store 已进入 K1。 | 多页 PTY 的 NP/PP/PPA/PPR/PPB、跨页复制、页上限、resize、Snapshot/资源归属的真实 Relay/设备回放。 |
| C10 | `traditional.rs`, `legacy_charsets.rs`, `legacy_projection.rs`, `charset.rs`, `traditional_tests.rs`; `../termlink/win-agent/src/TerminalCore/NativeTerminalTraditionalProtocol.cs` | VT52/C1/GR/UPSS 与 legacy 映射的 core 测试纳入 K1。 | UTF-8 默认不误认高位字节、模式切换、分块输入和历史恢复在真实 Windows/Android 的兼容性。 |
| C11 | `macros.rs`, `macro_tests.rs`; `../termlink/win-agent/tests/NativeTerminalCoreValidation/TraditionalProtocolValidation.cs` | 宏编码、预算和递归保护有 core 本地证据（K1）。 | 真实宏定义/替换/删除/调用、非法编码、取消、reset、跨 Relay 重试以及宿主效果的无半提交验证。 |
| C12 | `drcs.rs`, `drcs_bank.rs`, `terminal_drcs.rs`; `../termlink/win-agent/src/TerminalCore/NativeTerminalGraphicsResources.cs`; `../termlink/android/terminal-engine-native/src/main/kotlin/dev/termlink/terminal/nativecore/GraphicsResourcesDecoder.kt` | DRCS parser/bank/资源边界及任意现代视口最近 face 选择、resize/snapshot/frozen 回归通过 K1；标准 80/132×24/36/48 保持精确匹配。已导出安装；`r4installedcold` 实际新增字形像素 1050。 | designation/覆盖/擦除、DPI/resize、Snapshot/历史资源回收及 Android/Windows 绘制的完整语义。 |
| C13 | `sixel.rs`, `terminal_sixel.rs`, `graphics_png.rs`, `graphics_zlib.rs`; `../termlink/win-agent/src/TerminalCore/NativeTerminalGraphicsResources.cs` | SIXEL 解码、repeat/压缩/上限的 core 测试纳入 K1。 | 真实 SIXEL PTY 的流式/取消/截断、alpha/颜色、margin/alternate/resize、超大 repeat 防护和跨端恢复。 |
| C14 | `osc.rs`, `host_theme.rs`, `color_extensions_tests.rs`, `host_theme_tests.rs`; `../termlink/win-agent/src/TerminalCore/NativeTerminalHostThemeV1.cs`; `../termlink/android/terminal-engine-api/src/main/kotlin/dev/termlink/terminal/api/TerminalHostThemeV1.kt` | OSC/主题/颜色归属的 core 与本地测试纳入 K1。 | 真实标题/选择区颜色/17/117/DECAC 的可见效果、主题切换/保存恢复及有序 Host 回复经 Relay 的双端验证。 |
| H01 | `shell_integration.rs`; `../termlink/win-agent/src/SessionHost/PowerShellShellIntegration.cs`, `../termlink/win-agent/src/TerminalCore/NativeTerminalShellState.cs`; `../termlink/android/app/src/main/kotlin/dev/termlink/product/ShellIntegrationMenu.kt` | OSC 7/133、shell state 和目录/命令定位实现已接线；Windows SessionHost 主路径属于 W1。 | PS7 真实 cwd/prompt/command/output/end、中文/空格/URI/UNC/越权路径、退出状态跨重连、Android 打开目录及防伪标记。 |
| H02 | `host_signals.rs`, `host_effect.rs`; `../termlink/win-agent/src/TerminalCore/NativeTerminalHostSignals.cs`, `../termlink/pc-client/src/Desktop/NativeTerminalAlerts.cs`; `../termlink/android/app/src/main/kotlin/dev/termlink/product/TerminalSignalMenu.kt` | BEL/OSC 9/DECPS 事件和平台 effect 的 core/本地实现纳入 K1；Android 真 Keystore 仪表 2/2 通过（A1）。 | Relay 实时通知、前后台/静音/权限拒绝/限流、完成/取消、旧输出重放不重复提醒，以及 Android/Windows OS 实际行为。 |
| H03 | `clipboard.rs`, `host_effect.rs`; `../termlink/win-agent/src/Common/NativeDisplaySignals.cs`, `../termlink/pc-client/src/Desktop/NativeTerminalClipboard.cs`; `../termlink/android/app/src/main/kotlin/dev/termlink/product/TerminalClipboardPrompt.kt` | OSC 52 有界写入、授权 effect 和本地校验纳入 K1。 | 真实 Relay/OS 剪贴板、Base64/空/超限/拒绝、后台限制、控制权切换、回放/重试幂等及 Android 真机。 |
| H04 | `modes.rs`, `input.rs`; `../termlink/win-agent/src/SessionHost/WindowsConsoleMouseInput.cs`, `../termlink/android/app/src/main/kotlin/dev/termlink/product/TerminalMouseWheel.kt`, `NativeTerminalView.kt` | 1007/alternate-scroll 解析与鼠标/历史路由有 core/绑定本地证据，属于 K1。 | 备用屏/鼠标报告/主屏历史的组合经真实 Relay 验证；Android 非鼠标模式单指原操作不变、鼠标模式不双发。 |
| E02 | `history_frame.rs`, `history_page.rs`, `snapshot.rs`; `../termlink/win-agent/src/Agent/ProductionFrozenHistoryService.cs`, `../termlink/win-agent/src/Protocol/FrozenHistoryCrypto.cs`; `../termlink/android/app/src/main/kotlin/dev/termlink/product/FrozenHistoryBrowse.kt`, `../termlink/android/core-crypto/src/main/kotlin/dev/termlink/crypto/FrozenHistoryCrypto.kt` | immutable capture/page/token/anchor 的 core/绑定证据纳入 K1；`r4frozen9` 真实 Relay BEGIN/两次 PAGE/RELEASE、浏览中 live ACK 不变、释放后安卓输入及实时输出通过 21/21。 | 并发输出冻结、过期/截断、取消、重连、旧 Host 能力拒绝及完整产品场景。 |
| E03 | `delta.rs`, `delta_journal.rs`; `../termlink/win-agent/src/Agent/ProductionStateDeltaService.cs`, `../termlink/win-agent/src/Protocol/DeltaCrypto.cs`; `../termlink/android/app/src/main/kotlin/dev/termlink/product/TerminalDeltaRestore.kt`, `../termlink/android/core-crypto/src/main/kotlin/dev/termlink/crypto/DeltaCrypto.kt` | Delta/journal/密封对象 core/FFI 证据纳入 K1；新 SO 的 S7 产品向量模拟器 2/2 通过（A1）。 | 真实 Relay 分片、丢包/乱序/损坏/未知 feature、journal 淘汰、断线重同步和内存压力。 |
| E04 | `host_theme.rs`, `host_effect.rs`, `osc.rs`; `../termlink/win-agent/src/TerminalCore/NativeTerminalHostThemeV1.cs`; `../termlink/android/terminal-engine-api/src/main/kotlin/dev/termlink/terminal/api/TerminalHostThemeV1.kt` | 主题查询/准确能力声明/有序 Host effect 纳入 K1。 | 真实主题订阅/查询/切换/禁用/恢复、两个客户端主题不同、仅当前 authority 产生 PTY 回复，并经 Relay 重连验证。 |
| G01 | `kitty_graphics.rs`, `graphics_resources.rs`, `graphics_state.rs`, `graphics_layout.rs`; `../termlink/win-agent/src/SessionHost/KittyFileBroker.cs`, `KittySharedMemoryBroker.cs`, `BrokerCaptureCoordinator.cs`; `../termlink/android/app/src/main/kotlin/dev/termlink/product/BrokerCaptureRestore.kt` | Kitty parser/resource/codec/FFI 属于 K1；Windows shared-memory real Job 与非零 O/S 隔离路径及故障清理已有证据，导出库为 K2。Android 能力字段已修，`r4installedcold`/`r4localprivate` 的真实 file→Relay→Android 图像 smoke、durable ACK 和 Host 正常退出通过。 | 新增 file/temp/shared-memory 三来源及源文件修改后的恢复、两阶段恢复/WAL 重试、完整语义和压力回归仍需真实执行。r20 与 r33 当前前置失败为模拟器未登录，不再以历史 TL-H130 作为当前阻塞。合作式 `t=s` 不能打开任意 Windows/WSL 映射；未经登记的 Win32/WSL/POSIX 映射必须拒绝或明确 unsupported。 |

## 发布阻塞条件

1. 以上“未运行”项不得用 K1/K2 或 mock Relay 记录替代；至少要补齐真实 Relay、Windows SessionHost、Android Debug/设备的对应证据。
2. A1 中新 SO 的通用 JNI、S7 产品向量和 H02 真 Keystore 仪表已通过；通知路由、输入诊断的真实产品交互与其余 G01/E02 场景必须单独回归。
3. 当前不安排 LAN/NAT/direct 验收；不要把 direct 矩阵中的 `lan_direct`/`nat_direct` 当作通过或发布阻塞以外的完成项。
4. 本机隔离 DEV 服务端已部署，仍无正式版本、Docker Hub 推送或客户端发布结论；core dirty 或未完成 Corresponding Source/哈希核验时不得正式发布五库。
