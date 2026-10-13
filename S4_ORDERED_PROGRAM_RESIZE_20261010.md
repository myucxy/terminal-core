# C09 应用请求的有序尺寸调整

2026-10-10。DEV 0.3.3/132，未提交源码、未发布。此记录补充页存储及宿主尺寸记录，不代表整个 C09 产品验收结束。

## 协议与实现

| 输入 | 行为 |
|---|---|
| `CSI Ps $ \|`（DECSCPP） | 请求列数；省略或 0 为 80；保留执行时实际行数 |
| `CSI Ps * \|`（DECSNLS） | 请求行数；省略或 0 为 24；保留执行时实际列数 |
| 参数超 512、多参数、冒号形式 | 原子失败，沿用 C02 准确源诊断 |
| 1 行/列或显示器无法容纳 | core 能表达请求；当前 Windows 窗口最小 2 格，返回真实宿主拒绝，不改网格 |

请求本身不修改网格。Host Effect V2 新增 kind 2、required feature bit 2，载荷为两个 LE u32：
columns、rows，其中恰好一项为 0（保留该轴）。有此记录的帧 required_features=3，无记录继续为 1。
每批最多 64 个 resize 请求；V1 捕获在提交前返回 HostEffectVersionRequired；无效果捕获的历史重放仅解析。

SessionHost 在输出获得 durable ACK 后将请求交给单个有界工作队列，避免阻塞负责处理后续 ACK 的管道线程。
复用当前外层终端/自有窗口的实际 resize 通道：先改变真实展示与 ConPTY，再发布已有 Resize 事件。
之后发布 local kind 21 / wire 73 `HostResizeResult`，保存请求的 source sequence、结束 offset、ordinal、
所请求的轴、实际尺寸和精确结果码。输出重放和 Android 镜像只检查/丢弃 resize effect；绝不再次执行宿主操作。
尺寸结果也仅推进事件游标，不能替代先前的 Resize 事件。

原生窗口和安卓前台显示拒绝结果，包含规范化的请求及源坐标。明确标注“规范化”，不冒充原始转义字节；
不支持指令的原始序列仍由 C02 诊断提供。安卓重复事件不再次提示，历史重放不弹提示，后台无提示，前台三秒限频。
程序请求不占用手机“恢复桌面尺寸”的还原点；程序成功改变尺寸后清除已失效还原点。

会话精确协议身份更新为 `terminal_state_2_19_v3;width=1;output=0;resources=1`；
Snapshot 保持 2.19。新增 Relay 能力 `terminal_host_resize_v1`，缺少它明确拒绝终端附着；
因此最终发布时本次服务端路由变更需要同步部署，现在线上服务未更改。

## 本地证据

应用 `artifacts/validation/`：

- `s4-resize-core-all-20261010.log`：578 core + 10 golden + 56 FFI；分块、缺省、旧捕获回滚、资源上限与有序尺寸测试。
- `s4-resize-clippy-20261010.log`：workspace all-targets `-D warnings`。
- `s4-resize-export-20261010.log`：五个 dirty DEV 库及锁文件由 core 仓库构建/导出。
- `s4-resize-dotnet-20261010.log`：真实 DLL 请求/后续 Resize、严格 feature/kind/载荷校验。
- `s4-resize-conpty-20261010.log`：真实进程输出经过 bundled ConPTY 后 DECSCPP/DECSNLS 原文保留。
- `s4-resize-native-window-20261010.log`：真实 child→ConPTY→authority→Avalonia；先 96 列再 25 行、查询确认、拒绝 1 列、结果有序；手机 fit/restore、输入优先级与过期仍通过。
- `s4-resize-android-20261010.log`：native/core-session/network 单元及 app 编译；包含加密结果、重复去重、非法源事件不 ACK、历史不重放动作/提示。
- `s4-resize-instrumented-20261010.log`：模拟器 43 项 JNI/View、api 及 App 错误分类；真实 JNI mirror 等待有序尺寸，零输入回送。
- `s4-resize-history-20261010.log`：WAL/producer/checkpoint/outbox/uploader 与 shadow 重放新结果通过。
- `s4-resize-relay-20261010.log`：Go Relay 路由方向/鉴权及 wire 回归。
- `s4-resize-live-agent-20261010.log`：实际 Agent＋两个 SessionHost＋隔离 Relay 连接恢复。

仍待：真实部署的手机联合操作、ARM64 真机、混合版本及升级回滚、长时间运行验收。Windows 自有窗口仍是 opt-in；
外层终端是否接受窗口 resize 由实际宿主决定，拒绝必须准确返回。此阶段没有安装主客户端、上传、推送或部署。
