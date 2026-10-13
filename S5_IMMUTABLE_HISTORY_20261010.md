# S5/E02 冻结历史文字阶段（2026-10-10）

范围：primary/page 1 的文字历史、冻结 viewport、生命周期及两端本地浏览。
本阶段不宣称 H01 命令标记、图形资源、远端分页 RPC、持久 transcript 或整个 E02 已完成。

## 实现及边界

- `HistoryCapture` 冻结 source revision、primary storage、OSC 色表/链接与 Unicode width profile。
  alternate/live tail 不作为 sealed page 内容。平滑 viewport 另保留捕获时的 live tail，二者 API 明确分开。
- `HistoryPageRequest` 按 exclusive 精确行 ID 查询，ID 可为零，不是排序键；latest 使用 `u64::MAX`。
  整个逻辑行跨多个 V4 片段时仍属于同一页；V4 子帧末行仍是既有的局部 hard boundary，逻辑连续性由页目录表达。
- `TLHPV1\0\0`：96 字节头、40 字节行目录、带长度 V4 子帧。CRC32C 覆盖整个消息但排除自身字段 `92..96`。
  页最多 128 行、65,536 cells、4 MiB meaningful lead-cell UTF-8、16 MiB wire；空白 padding 受 cells/wire 上限约束。
  超限的下一个整行返回 required cells/UTF-8，不截断或跳过。解码逐项验证长度、保留位、目录、CRC、revision、字形/颜色/链接引用、宽字符和逻辑行边界。
- additive C ABI `history_begin/page/viewport/check/release_v1`：不复用 token，owner identity 不随内存地址复用。
  同终端一个 capture；每库实例最多 64 个、256 MiB 总计、单 capture 64 MiB 保守分配计费。
  拒绝发生在 clone 前；包含 cells/String、行元数据、两个临时 viewport 与有界 OSC 余量。
  固定五分钟单调时钟 lease，不因读取续期；释放/过期/错误 owner、revision、anchor 明确区分（状态 16–19）。
  原终端销毁后 capture 在 lease 内仍独立可读。过期内存在下一次 registry 操作清理，空闲时仍受全局限额约束。
- C#/Kotlin decoder 与 JNI 绑定已接入。Windows `IDisposable`/兜底 finalizer，Android `AutoCloseable` 及引擎生命周期释放。
  owner/token 只解决同进程句柄误用，不是远端认证凭据；未直接暴露为跨会话 RPC。
- Android 自有 View 第一次向上滚动捕获；后续输出、ED3 和远端 resize 继续进实时 mirror/ACK，但保持浏览画面。
  回到底部/本地输入释放捕获，恢复实时；真实 authoritative size 继续更新，不能用冻结画面的旧尺寸反馈给 PC。
  过期或内存限额只提示，不退休终端。Windows opt-in 原生窗口按同样规则浏览；静止时只检查 lease，不重复导出大帧。

## 修复及兼容性

自动软换行经过全 viewport 滚动时，旧路径清除了上一行 soft-wrap，导致长行被假封存成多个逻辑行。
已在 output-wrap capture 路径恢复该位，explicit scroll 保留原硬边界行为。600 行中文宽字符单逻辑行及快照恢复已覆盖。
由于回放语义改变，mandatory admission 从 `terminal_state_2_19_v1` 更新为 `terminal_state_2_19_v2`。
Snapshot 仍为 2.19，V4 wire 不变；不能让旧语义的 mirror 参与新会话。后续固定 profile 与 Host/Agent IPC admission 见 [S1 持久协议记录](S1_PERSISTENT_SESSION_PROFILE_20261010.md)，动态协商与产品升级矩阵仍有后续工作。

## 本地验证

DEV 数字基线仍为 0.3.3/132。两仓均 dirty，五库由 core 脚本重新导出，锁文件记录哈希。
测试日志在应用 `artifacts/validation/`（不包含正式发布结论）：

| 日志 | 结果 |
|---|---|
| `s5-history-codec-20261010.log` | 576 core + 10 golden + 56 FFI 通过（含备用屏 resize 后捕获 primary geometry） |
| `s5-history-clippy-20261010.log` | workspace/all-targets `-D warnings` 通过 |
| `s5-history-export-20261010.log` | Windows x64 与 Android 四 ABI dirty DEV 库导出成功 |
| `s5-history-dotnet-20261010.log` | 真实 DLL：分页、精确 retry、样式/链接/Unicode、600 行整行、损坏页、销毁源后读取通过 |
| `s5-history-android-final-20261010.log` | Android native 单元及模拟器 42 项 JNI/View 测试通过；含浏览冻结、ED3、resize、输入返回实时 |
| `s5-history-sessionhost-20261010.log` | 真实 ConPTY 生命周期/鼠标/回复/尺寸，以及 Windows pipe 冻结/恢复/空闲抑制通过 |
| `s5-history-admission-20261010.log` | 旧/缺失/未来/损坏 capability 及发布前隔离通过 |
| `s5-history-android-20261010.log` | Android production 网络测试与此前 41 项 JNI/View 通过 |

本轮为 self-review：检查了所有 C/JNI/托管 ownership、错误后 buffer 清理、无符号值和长度转换、冻结 revision 与实时 ACK 隔离。
ARM 真机、真实部署 Relay 故障/混用、长时间 soak、升级回滚及全量 22 组验收未完成；没有上传、部署或替换主客户端。
