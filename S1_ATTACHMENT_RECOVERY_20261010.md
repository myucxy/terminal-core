# 终端附着协商与重连回归（2026-10-10）

开发状态：本地实现与下列检查通过；不是正式发布，也不是完整 immutable session profile 协商。
应用代码、协议、Windows 和 Android 改动位于同级 `termlink` 仓库。core ABI 此阶段未改变。

- `terminal_attach_v1` / `terminal_host_geometry_v1` 是实际物理连接的 Relay routing capabilities。
- Android 的 SUBSCRIBE 携带请求 UUID 和 `terminal_state_2_18_v1` 解码能力。
  Windows 在发布终端流和服务历史前核验。加密消息 94 返回对应的接受/拒绝结果，
  不占用终端事件序列，也不推进应用 ACK 或 Relay replay high-water。
- 缺少 Relay、客户端能力、格式无效与确认超时分别提示；超时不推断为已确定的版本不兼容。
  新版 Android 产品的实时、history-only 两个入口都启用协商。旧版 Android UI 不能显示新增错误码。
- 重连前遗留的密文可能先于确认到达。Android 在确认前不应用、不 ACK，确认后从未变的持久游标重放。
  新输入、显式尺寸请求及原始密文重试受当前确认 generation 约束；旧确认/重复确认不能放行新连接。
- Windows exact retry outbox 核验当前订阅、flow hold 与能力；暂缓会话保留密文，不阻塞同手机的其他会话。
- history-gap 重建、换密钥的等待协商移出 renderer ingress mutex，避免等自己持有的锁。
  会话失败执行 scoped UNSUBSCRIBE，不断开其他会话共用的物理 WebSocket。
- 修复 SessionHost managed 模式 stdin EOF 提前关闭 ConPTY 输入的问题：Agent 控制的会话继续保留输入，
  避免 ConPTY 启动探测回复写入失败导致 exit 125。standalone EOF 与显式结束语义保留。

## 验证证据

应用仓库 `artifacts/validation/`：

| 证据 | 结果与边界 |
|---|---|
| `s1-attach-recovery-android-20261010.log` | 141 项 production network 测试、选定 app 契约/错误分类测试通过；包含带协商的重连输入与历史 Gap 恢复 |
| `s1-attach-subscriber-20261010.log` | Windows 能力列表/拒绝隔离、订阅与认证恢复通过 |
| `s1-attach-relay-outbox-20261010.log` | exact bytes、ACK 回收、重连、容量、暂停与同端其他会话继续传输通过；只执行 Relay outbox 子集 |
| `s1-attach-sessionhost-20261010.log` | 实际 ConPTY 鼠标、回复、生命周期、原生窗口 pipe、尺寸调整通过 |
| `s1-attach-live-agent-20261010.log` | 实际 Agent + 两个 SessionHost + loopback Relay，managed stdin EOF、单会话失败与物理连接换代通过 |
| `s1-attach-relay-20261010.log` | Go 消息方向、能力交集、94 不污染终端高水位通过 |
| `s1-attach-native-emulator-20261010.log` | emulator-5556 上 38 项 JNI/View 回归通过；不代替产品附着联测 |

上述不代替已部署 Server + 模拟器产品联测、ARM64 真机、长时间运行及升级回滚。
尚须实现固定 width/资源/profile 与 SessionHost IPC 混版协商；新增必需状态必须更新解码能力标识。
两仓库源码仍有未提交改动，五库仍为 dirty DEV，未上传/发布/部署，也未替换主 Windows 客户端。
