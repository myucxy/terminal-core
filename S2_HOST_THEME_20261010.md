# E04 宿主主题状态（2026-10-10，DEV）

## 范围与契约

独立实现 `CSI ?996n` 查询、`CSI ?997;1n` 深色/`CSI ?997;2n` 浅色回复、2031 订阅。
行为参考 Ghostty 公开 color-scheme 协议说明及 MIT 源码，未复制实现。已知可用主题时启用 2031 立即回复一次，
实际 scheme 改变才主动报告；重复主题、单纯颜色变化及关闭订阅不会主动报告。
无可信宿主主题时不猜 OS 状态，2031 查询返回 unrecognized，996 无回复。

- C event 6：`HOST_THEME_V1`，1044 字节。u32 version=1、u8 scheme=1/2、三个保留零字节，
  随后 259 个 RGBx 色值（x=0）：256 色调色板、前景、背景、光标。坏长度、保留字段、scheme 在应用前拒绝。
- 与几何一样来自当前选定的 Windows 原生显示宿主，通过当前用户认证 pipe 命令 10、LocalFrame 20 进入 WAL。
  原生 Avalonia 窗口发布实际使用的主题基色；程序颜色覆盖仍由终端状态控制。
- Host Effect V2 沿用 PTY_REPLY，在主题事件结束偏移 1044 上产生单次回复。
  Windows authority 执行；历史 producer、shadow 和 Android 镜像只计算并丢弃回复，避免重放副作用。
- OSC 状态新增显式颜色归属位。程序设置为与当前基色相同的值仍视为覆盖；重置相应位后才重新继承。
  DECAC 别名按真正的目标色槽处理；RIS 清除覆盖、保留宿主基础主题；旧快照首次采用主题时保守保留已有颜色。
- Snapshot 2.19 / required feature `0x1_0000_0000` / critical section `0x401` 保存 scheme 与 260 个归属位。
  没有主题事件的旧状态保持原 canonical 格式。最大快照 65,796,449 字节，64 MiB 内剩余 1,312,415 字节。
- Relay 消息 72 `HOST_THEME` 仅允许 PC 发出，加密并纳入终端事件顺序与 ACK。
  必需解码能力更新为 `terminal_state_2_19_v1`；Relay 协商另需 `terminal_host_theme_v1`。
  本轮附着记录的 2.18 标识是主题加入前的阶段证据，不可当成当前全部状态的解码能力。

## 本地证据

应用仓库 `artifacts/validation/`：

| 文件 | 已通过内容 |
|---|---|
| `s2-theme-core-20261010.log` | 569 core、10 golden、53 FFI；分块查询、显式相等色、别名/重置、旧快照迁移、失败回滚和主题事件边界 |
| `s2-theme-clippy-20261010.log` | 全 workspace/all-targets Clippy |
| `s2-theme-export-20261010.log` | 一个 DLL + 四 ABI SO 从 core 脚本导出，dirty DEV lock |
| `s2-theme-dotnet-20261010.log` | DLL 绑定、有序回复、V4 配色与快照恢复 |
| `s2-theme-desktop-20261010.log` | Windows 桌面构建 |
| `s2-theme-native-window-20261010.log` | 真实子进程通过 bundled ConPTY 向实际原生窗口查询主题；主题/几何进入 journal，输入、fit/restore 通过 |
| `s2-theme-history-20261010.log` | Windows 持久 WAL、producer、shadow、加密 outbox/uploader 与恢复后主题查询 |
| `s2-theme-final-android-20261010.log` | production network 回归与 40 项真实 JNI/View；主题真实背景像素及镜像不产生输入回复 |
| `s2-theme-relay-20261010.log` | Go realtimewire/realtime，包括 PC-only 路由 |
| `s2-theme-live-agent-20261010.log` | 当前解码/Relay 能力下真实双 SessionHost 的 isolated failure 与连接换代 |

仍是 opt-in 原生窗口；外层终端不能提供可靠主题时保持“未知”。完整产品 Relay/mixed-version、ARM64 真机、
长时间运行和升级回滚未完成。不把这些本地证据计为 22 组全完成，也未正式发布/部署/替换主客户端。
