# H02 开发中的契约与后续边界

2026-10-10。**核心、双端平台消费者和旧投射保护已实现，H02 产品与真机验收仍未全部完成。**

## 已接入

- BEL 生成 Host Effect V2 kind 3（PlatformAction），required feature bit 4。payload `[1,0,0,0]`，同一个源事件的 BEL 合并为第一个，避免恶意连续铃声导致分配放大。
- DECPS `CSI volume;duration;notes...,~`：缺省 0，volume 饱和至 7，duration 至 255（1/32 秒），最多 30 音符，notes 饱和至 25；0 为静音，1..25 对应 C5..C7。payload `[2,volume,duration,count,notes...]`。每次 capture 最多 64 个声音请求，超过则原子拒绝。
- OSC 9 仅实现 `4;state;progress` 进度。state 0..4：隐藏/正常/错误/不确定/暂停；省略值 0，百分比饱和至 100，隐藏/不确定的 canonical percent 为 0。其他 action 或损坏参数明确拒绝，没有把它们当任意宿主命令执行。
- 进度是 `HostSignalsV1` 状态，12-byte `TLHS1\0\0\0` + state/percent/2 reserved；Snapshot **2.21**、feature `0x4_0000_0000`、section `0x403`。reset 清空；无此状态沿用旧 canonical version。canonical 上界 66,070,235，离 64 MiB 仍有 1,038,629 字节。
- 新 C `tl_terminal_get_host_signals_v1`，JNI `hostSignalsV1`，Kotlin/C# 严格解码和类型化 PlatformAction。PTY reply 路径严格分流，不能将 BEL/音符 payload 写回 PTY。
- Android engine 提供进度 flow 和可注入的 `TerminalPlatformActionSink`。`notifyLiveOutputApplied` 仅在 RemoteTerminalSession 新接受实时 Output 后调用；duplicate、raw replay、checkpoint 不触发。pending effects 在新事件/reset/install/dispose 时清理。
- 产品 profile 现为 **`terminal_state_2_21_v1;width=1;output=0;resources=1`**。五个 DEV 库已按此核心阶段重新导出；不是正式构建/发布。

## 证据

- `s3-signals-core-20261010.log`：587 core + 10 golden + 57 FFI；`s3-signals-ffi-20261010.log` 已通过随后新增的第 58 个 FFI host-state 读出用例。
- `s3-signals-clippy-20261010.log`：all-targets -D warnings。
- `s3-signals-export-20261010.log`：五库，dirty DEV。
- `s3-signals-native-20261010.log`：真实 DLL platform actions、进度 ABI；同时通过既有 shell/历史/页/传统协议验证。
- `s3-signals-managed-20261010.log`、`s3-signals-android-build-20261010.log`：新增 payload/feature/非法音符/边界的严格解码测试。
- `s3-signals-instrumented-20261010.log`：45 项模拟器 JNI/View 测试通过；`s3-signals-session-20261010.log` 随后重新验证重复实时输出及 resetAndReplay 不触发提醒。

行为参考：2026-10-10 只读查看 Microsoft Terminal `src/terminal/adapter/adaptDispatch.cpp` 的 `PlaySounds`、`DoConEmuAction`；自行实现协议，没有复制源码。参考 URL `https://raw.githubusercontent.com/microsoft/terminal/main/src/terminal/adapter/adaptDispatch.cpp`。尝试查询 main revision 时 GitHub 连接重置，因此不把此次 main 读取当固定版本来源或发行材料；正式记录需补固定 revision/文档。

## 必须继续完成，不能越过

1. 双端已接入设备开关（默认关闭）和持久去重；Android 前台提示/后台私密无声通知、AudioTrack；Windows 原生窗口状态文字/后台任务栏闪烁及系统声音通道。声音最长 2 秒、3 秒冷却、无无限队列；Android 遵循通知音量/铃声模式/DND，Windows 查询系统通知状态，停用/关闭取消。真实扬声器、厂商后台和勿扰策略仍须产品/真机验证。
2. Android 以 account+session 的摘要隔离，Windows 以当前 OS 用户目录+session 隔离，持久化 sourceSequence/offset/ordinal 高水位后才执行；拒绝、限流和静音也登记。损坏或无法写入时拒绝动作；最多 4096 个会话记录，满额不驱逐旧记录。这是至多一次尝试，崩溃可丢失一次提醒，并非 exactly-once。
3. 旧 Windows 外层路径现使用 **核心同一 parser 生成的规范化显示流**，不再透传 managed 会话原始输出。新的原子 `tl_terminal_apply_event_with_legacy_projection_v1` 同时返回 V2 effects 与 TLPJ1 展示流；BEL/DECPS/OSC9/52、已由宿主执行的行列调整不投射。不是第二套字符串过滤 parser。未附着 Agent 的独立诊断 harness 仍原样透传，不能当作受管产品权限保证。
4. 宏定义留在核心、调用展开后再投射；C1 转成对应 7-bit 控制序列；跨帧 OSC/CSI 只在完整解析后投射；取消的字符串不投射。补充 UTF-8 中断屏障，避免被移除动作把两个残缺字符拼接；REP 按权威的 repeat source 决定是否投射。投射超过 8 MiB 原子拒绝，没有退回原始字节的路径。
5. H03 OSC52 仍未开发；需写入-only、严格 Base64/大小、控制租约/后台限制、用户同意、去重。未来复用 platform action 路径，但当前 type 并未虚构支持。
6. 产品 Relay、后台/权限拒绝、重连/崩溃/升级以及真实双端显示验证。LAN/NAT 不适用。

## 平台消费者及投射保护证据

- `s3-signal-consumers-android-20261010.log`：5 项产品测试，含持久隔离/替换/竞争、缺权限、账户变更、关闭与默认拒绝；app 编译通过。
- `s3-signals-surface-20261010.log`：Windows 默认拒绝/开关/消费者重建/文件损坏/IO 失败、帧编解码、PCM 静音和 2 秒上限，以及现有真实 Skia 绘制回归。
- `s3-signals-pipe-20261010.log`：实际当前用户 PID 验证管道，进度和一次性动作轮询。
- `s3-signals-native-window-20261010.log`：真实子进程→ConPTY→authority→Avalonia，输入/尺寸/主题/退出回归。
- `s3-safe-projection-full-20261010.log` / `s3-safe-projection-clippy-20261010.log`：594 core + 10 golden + 59 FFI 及无警告检查；TLPJ1 输出所有权和缓冲区发布失败/异常不提交。
- `s3-safe-projection-native-20261010.log`：真实 DLL/.NET，动作隔离、跨帧标题、准确错误诊断。
- `s3-safe-projection-sessionhost-20261010.log`：managed nested ConPTY 实际鼠标/键盘/回复/生命周期/窗口尺寸回归。
- `s3-safe-projection-claude-20261010.log`：真实 PowerShell 7 启动本机 Claude，观察 16t，核心未失败，Ctrl+C 后继续输入并 exit 0。
- `s3-safe-projection-instrumented-20261010.log`：45 项模拟器 JNI/View；仅替换测试 APK，产品主 APK 未安装新代码。

**仍未完成**：Windows 原生展示仍是显式入口，完整 profile 的默认迁移未做；旧外层路径现在安全抑制 OS 动作，但尚无启用本机提醒的消费者/UI。Android 后台提醒目前打开应用入口，尚非精准 CLI 会话跳转。无 GMS/小米真机、实际声音/DND、持久记录压力和产品 Relay 故障验收未完成。通知不带终端正文，也不能保证进程被杀后仍能收到 BEL（终端 BEL 不是离线推送协议）。整个 22 组能力没有完成，不能发正式版。
