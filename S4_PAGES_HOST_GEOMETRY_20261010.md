# C09 页与 C01 有序宿主尺寸开发记录

2026-10-10。两仓库仍为未提交 DEV；不代表 22 组能力或正式发布完成。

## DEC 页

- 六个逻辑页，页 2–6 惰性分配，共享 32,768 个额外 cell 的硬预算；只有第一页保存滚动历史。
- NP/PP、PPA/PPR/PPB、DECPCCM 64、active/visible/saved page、主副屏交互、跨页 DECCRA、页 CPR/显示范围/checksum。
- 写入非可见页时保持显示页不变并隐藏光标。跨页复制保留宽字符/链接并裁剪双宽行隐藏列。
- Snapshot 2.17、required feature `0x40000000`、section `0x300`。严格检查页 ID、顺序、几何、资源总量和引用。
- 包含逐分块、恶意快照、预算回滚及 1,000 次种子状态转换/恢复测试。
- 参考 Microsoft Terminal `6676c6f938f7e98ad680f0cecedd3bafb6529734` 的 `PageManager.cpp` 行为；MIT，未复制实现。

## 有序宿主尺寸

新增 additive `Event::HostGeometry` / C event kind 5，旧结构尺寸不变。
32 字节小端 payload：8 个 u32，依次为 version=1、cell 宽高、物理屏幕宽高、外窗口宽高、reserved=0。
cell 每维最多 1,024px，屏幕/窗口最多 32,768px；每对全零表示不可用，拒绝半零、越界及非零 reserved。
只有选定显示宿主通过 SessionHost 日志写入。它不来自 PTY 输出，也不由 Android 本地测量擅自覆盖。

| 查询 | 行为 |
|---|---|
| 14t | 已知 cell 时返回 `rows*cellHeight`、`columns*cellWidth` |
| 14;2t | 已知外窗口真实测量时返回；不拿字符网格猜边框 |
| 15t | 已知物理屏幕时返回 |
| 16t | 已知整数 cell profile 时返回 |
| 18t | 总是返回权威行列 |
| 19t | 屏幕/cell 均已知且结果非零时返回屏幕可容纳行列 |
| 未登记参数变体/不可用测量 | 可选探测无回复，继续处理输出 |

尺寸状态随 Snapshot 2.18、required feature `0x80000000`、section `0x400` 恢复。
默认不可用状态保持旧 canonical snapshot 字节。RIS/DECSTR 不清除真实宿主测量；宿主可以有序撤回。
当前最大 canonical snapshot 保守界为 **65,796,370 字节**，低于 64 MiB，余 **1,312,494 字节**。
后续图形资源须重新规划共享预算，不能直接增加无界图片数据。

C#/Kotlin/JNI/FFI 已接入，回复的类型、数字格式和维度上限在三种语言中一致。
Avalonia 原生窗口测量 DPI 后的整数 cell、所在屏幕和 Win32 外窗口，变化时发布到 durable journal；
Agent/历史快照/Android 实时与历史回放消费同一事件。Relay 新增仅 PC 可发的加密消息 71，服务端不解码内容。

查询语义参考 XTerm Patch #411 ctlseqs，2026-10-10 下载文件 SHA-256
`364c1c1987c85b1c1135e57a93e9008054f46d09a338de8dcc66ea9a7c613709`，只参考协议行为。

## 本地证据

日志在应用 `artifacts/validation/`，DEV 基线 0.3.3/132。

| 证据 | 结果 |
|---|---|
| `s4-pages-android-retry-20261010.log` | 原模拟器 Activity 超时后重启指定测试 AVD，37/37 通过 |
| `s1-geometry-core-20261010.log` | 565 core + 10 golden + 52 FFI，通过 |
| `s1-geometry-clippy-20261010.log` | workspace/all-targets，零警告 |
| `s1-geometry-five-libraries-20261010.log` | DLL + 四 ABI SO 导出并校验；source_dirty=true |
| `s1-geometry-dotnet-20261010.log` | 实际 DLL 有序查询、重置、尺寸改变、快照/撤回通过 |
| `s1-geometry-android-jni-20261010.log` | emulator-5556，38/38 实际 JNI/View 通过 |
| `s1-geometry-native-window-20261010.log` | 实际子进程→ConPTY→日志 ACK→核心→PTY 应答，14/16/18/19 一致；实际窗口测量、手机 fit/restore 通过 |
| `s1-geometry-history-20261010.log` | Windows WAL/producer/shadow/checkpoint 保留尺寸状态，通过 |
| `s1-geometry-relay-android-20261010.log` | 生产网络测试含加密尺寸、重复、坏 payload 会话隔离及不推进 ACK，通过 |
| `s1-geometry-relay-go-20261010.log` | Relay/wire 路由与 PC/Android 发送权限，通过 |
| `s1-geometry-sessionhost-20261010.log` | 宿主应答、生命周期、鼠标、原生展示及尺寸回归，通过 |

自我复核，没有独立审核者。没有替换主客户端、部署服务端或上传制品。

## 尚未关闭

- C09 页尺寸 DECSCPP/DECSNLS 的有序宿主请求、结果路径后续已实现，见 [有序请求记录](S4_ORDERED_PROGRAM_RESIZE_20261010.md)；产品部署验收仍待完成。
- C01 整数 cell profile 在非整数 DPI 下是明确的虚拟图形像素单位；图形绘制须采用相同单位。
- 当前真实测量来源只接入 opt-in Avalonia 原生窗口；外层终端路径仍按未知像素安全降级。
- 新宿主事件及 Snapshot 2.18 尚须生产 profile/旧客户端协商、服务端版本能力门禁、公网 Relay 与故障恢复实机验收。
- 生产默认展示迁移、ARM64 真机、IME/辅助功能/性能/长稳/升级回滚及其他能力组仍未完成。
- Server 代码涉及消息白名单；上线必须先通过混合版本门禁，不能将本地路由测试称为在线服务已支持。
