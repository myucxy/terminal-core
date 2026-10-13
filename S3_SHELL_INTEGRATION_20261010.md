# H01：Shell 标记、目录与命令定位

2026-10-10。本批完成核心、C/JNI/.NET/Kotlin、Android 菜单及 Windows 原生窗口路径的实现与本地验证。不是 22 组全部完成或正式发布。

## 协议与边界

- OSC 7 接收有界 `file://host/absolute/path`，严格 UTF-8/百分号编码；OSC 133 A/B/C/D 记录 prompt/input/output/end、可选有符号退出状态，最多保留 64 条命令标记。标记不捕获或执行命令。
- 重复 B/C 保留首个位置；缺 A 的 B/C 建立部分记录；新 A 将未完成记录标记为中断；D 没有活动命令时忽略。reset 清理记录但不复用 command ID。
- 锚点包含源序号、逻辑行 ID、cell offset、页和主副屏。按冻结历史中的实际位置解析，不把行 ID 当排序键。reflow 后仍能定位；ED3/驱逐后新捕获明确拒绝；已经冻结的历史继续可读。
- Snapshot **2.20**，required feature `0x2_0000_0000`，section `0x402`。没有 shell 状态时保留旧版本 canonical bytes；影响解码的产品协议身份升级为 **`terminal_state_2_20_v1;width=1;output=0;resources=1`**，旧持久标记不能被新二进制重写冒充兼容。
- Shell State V1 `TLSHV1\0\0` 最大 273,696 字节：32 字节头、当前目录、最多 64 个 116 字节固定记录加目录。C checkpoint 导出、JNI 与两端严格解码；快照 preflight 不分配动态 shell 集合或 URI 解码缓冲。
- 新增 C `tl_terminal_checkpoint_export_shell_state_v1`、`tl_terminal_history_resolve_anchor_v1`。旧接口/结构大小不变，缓冲沿用原所有权规则。
- URI 始终是不可信元数据。目录操作只接受空 host/localhost 的 drive-local Windows 路径，拒绝网络 host、UNC、设备/反斜杠逃逸、ADS、控制字符、点段和无效编码。Android 使用现有认证和路径授权的远程目录 RPC；Windows 显式点击后通过独立参数打开 Explorer，不执行 shell 字符串。

## 产品接入

- Android 云朵菜单增加“打开当前目录”“命令定位”，保持 CLI 页面/快捷栏几何不变。菜单按需读取，无每输出帧的额外 shell 状态拷贝。命令列表显示序号、状态和目录，不伪造未捕获的命令文本。
- Windows 原生窗口增加紧凑目录/命令按钮及 Ctrl+Shift+M；沿用已验证 PID 的当前用户 pipe。读取/定位不改变终端权威和输出 ACK。超限/过期/失效锚点不应终止 shell。
- 自动接入新建交互 PowerShell 7/Windows PowerShell，仅追加 session-local `-NoExit -EncodedCommand`。保留 profile、原 prompt 和 PSConsoleHostReadLine；不改用户磁盘 profile、执行策略或 PSReadLine 快捷键。指定 `-Command`/`-File` 等自定义参数时保留原启动语义，不自动注入。
- PowerShell D 标记报告其管线成功状态 **0/1**，保留原 `$LASTEXITCODE` 给用户；不把上一次 native 程序遗留的非零码误报为后续 cmdlet 的退出码。直接由程序发送的 OSC 133 D 数值仍原样保留。
- 主屏之外的标记可保存/恢复，当前命令定位仅浏览保留的主屏文字历史，其他页/副屏明确提示不可定位。后续 E02 继续补齐持久分页/图形/标记的历史 API。

## 本地证据

日志均在应用仓库 `artifacts/validation/`，DEV 基线仍为 0.3.3 / 132。

| 验证 | 证据 |
|---|---|
| Core 全量 583 + golden 10 + FFI 57；Clippy all-targets -D warnings | `s3-shell-core-full-20261010.log`、`s3-shell-clippy-20261010.log` |
| 五个 DEV 库导出、实际 DLL 读取/冻结锚点/边界/路径拒绝 | `s3-shell-export-20261010.log`、`s3-shell-dotnet-20261010.log` |
| 真实 PowerShell 7.6.6 / Windows PowerShell 5.1 → ConPTY：中文/空格 cwd、所有阶段、成功/失败、native 失败后 cmdlet 失败、cd | `s3-shell-live-20261010.log`、`s3-shell-live-ps51-20261010.log` |
| 原生展示 pipe：目录状态、命令锚点、失效拒绝、历史、输入及 PID | `s3-shell-native-pipe-20261010.log` |
| 真实子进程 → ConPTY → authority → Avalonia：窗口、输入、几何、手机适配/恢复、程序 resize | `s3-shell-native-window-20261010.log` |
| Windows 两个 Desktop target 编译 | `s3-shell-desktop-20261010.log` |
| Android 编译、API/native 单元；emulator-5556 共 44 项 JNI/View 实测 | `s3-shell-android-build-20261010.log`、`s3-shell-instrumented-20261010.log` |
| 精确 profile/DPAPI/恢复/旧数据保留 122 断言 | `s3-shell-profile-20261010.log` |

尚未完成：正式服务器上的产品目录菜单点击和混合版本/升级矩阵、自定义 prompt 工具的兼容矩阵。当前仅使用 Relay；未测试 LAN/NAT。未替换主 Windows/Android 应用、未部署服务器、未提交/推送/发布。本次模拟器仅更新 instrumentation 测试应用。
