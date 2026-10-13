# S1 会话协议固定与历史恢复门禁

2026-10-10；DEV 0.3.3/132，应用未提交工作树。本节是局部实现与验证记录，不表示 22 组能力完成或发布。

## 当前实现

- Windows HostHello/AgentHello 在既有 PID、随机 token 后携带精确协议描述：
  `terminal_state_2_19_v2;width=1;output=0;resources=1`。继续使用既有 width 1 / UTF-8；不默默启用 Unicode width 2。
  先核验 token，再核验描述；Agent 还核验管道的实际进程 ID 和注册身份。旧格式、未知/不一致描述不接收输出。
- SessionAgent 在成功验证 Host 后、返回 AgentHello 前，用 DPAPI＋原子写入保存 `terminal-profile.dpapi`。
  标记包含格式版本、会话 ID 和完整描述；首次数据写入失败不能跳过标记。重连不改写标记。
- 独立历史仓库的 WAL、checkpoint、outbox、transcript 和加密分发缓存共用同一会话描述门禁。
  标记再绑定 PC device ID、session ID 和 DPAPI domain。新空仓库只在写入当前已接受的事件/投影时建标记，空读取不建标记。
- 有输出、检查点或输入去重记录的旧目录缺少标记时，不根据新程序版本推断旧语义，不回填标记，也不改写或重放原数据。
  已标记但语义不兼容、损坏、过大或会话/DPAPI 身份不符分别拒绝。标记受保护大小上限 4096 字节。
  每个打开的仓库/缓存实例首次验证后缓存结果，避免每个输出块增加一次 DPAPI 解密；重新打开必须重新核验。
- 已退出 Host 的恢复分支也执行检查；不向不兼容日志补写新版 Interrupted/SessionRemoved。
  Registry 保存兼容性原因，附着和 HostHistory RPC 返回准确错误，后台历史生成不热循环重试此类不可恢复错误。
  此门禁不自动删除原文件；用户主动删除和既有七天保留策略仍按原规则执行。
- Android 显示非自动重试提示：TL-H133 为本地 Host/Agent 不匹配；TL-H134/135/136 分别为历史标记缺失、无效、不兼容。
  提示新建会话，并说明原文件保留；不建议删除历史，不报成服务器不可用。
- 测试发现并修复管道等待期间主动停止的竞态：只有 stopRequested 已设置时将 Begin/EndWait 的管道关闭视为正常停止；其他 I/O 故障仍抛出。

## 验证

应用仓库 `artifacts/validation/`：

| 日志 | 证据 |
|---|---|
| `s1-persisted-profile-20261010.log` | 实际命名管道旧握手拒绝、匹配重连 ACK；122 个持久化断言；匹配握手不能重新标记旧 WAL；六类拒绝双仓库数据哈希不变；三个历史 RPC 保留错误分类 |
| 同上 | Registry 已结束恢复、输入去重、Host 丢失、受控停止均通过；正常日志仍生成连续 Interrupted/Removed |
| `s1-persisted-profile-history-20261010.log` | 既有生产 WAL/producer/outbox/uploader 回归通过 |
| `s1-persisted-profile-android-20261010.log` | production 网络和 SessionOpenFailure 单元测试通过 |
| `s1-persisted-profile-live-agent-20261010.log` | 隔离真实 Agent＋两个 SessionHost＋loopback Relay，从连接 generation 1 恢复到 2 |

本阶段不改 native ABI、Snapshot 2.19 或五个库，沿用 S5 导出的库。测试只在隔离实例运行，未替换主客户端。

## 仍未完成

这是当前固定 profile 的持久化与严格拒绝，不是任意 profile 的动态协商，也不是旧历史自动迁移工具。
新增图形资源、命令标记、事件版本时仍须同步扩展身份与升级矩阵。旧历史只保留，未承诺可用新内核恢复。
产品部署 Relay 混合版本、升级/回滚、ARM 真机和长时间运行验证仍待完成；本地通过不能代替这些证据。
