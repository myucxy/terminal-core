# C10 / C11 开发与验证记录

2026-10-10；两仓库为未提交 DEV 状态。本文是阶段记录，22 组能力尚未完成；不表示发布、部署或主客户端升级。

## C10 已实现

- VT52 模式进出、A/B/C/D/F/G/H/I/J/K/Y/Z/=/>，光标地址 `0x20..0x7f`，方向键、PF1–4、数字小键盘编码。
- DOCS ISO-2022/UTF-8、DECAC1、S7C1T/S8C1T、GR 与 SS2/SS3；默认 UTF-8 不误认连续字节。
- UPSS：CSI `&u` 查询，DCS `Ps!u` 指定，`<` G-set 动态引用；不接受递归引用。默认为 96 字符 Latin-1。
- 16 组补充字符表见 `third_party/legacy-charsets/README.md`。数据固定 Windows Terminal revision
  `6676c6f938f7e98ad680f0cecedd3bafb6529734`；保留 MIT 许可和来源。解析器为 TermLink 实现。
- ISO-2022 标题/链接采用 Latin-1 解码，屏幕 G-set 不改写控制字符串。
- Snapshot 2.15，required feature `0x10000000`；charset codec 3，mode codec 6。旧状态仍导出原有 canonical bytes。
- C1 回复经核心/.NET/Kotlin 校验；外层回复过滤识别完整 C1 和混合 ST，UTF-8 文本保持原样。
- 字符表生成器 `--check` 核验生成可复现；许可证通过 Android generated assets 和 Windows 发布载荷携带。

## C11 当前实现

协议：DCS `Pid;Pdt;Penc!z` 定义，CSI `Pid*z` 调用，DSR `?62n` 存储空间（16-byte blocks）、`?63;Pidn` 校验和。
Pid 0–63，Pdt 0 替换指定项/1 删除全部后定义，Penc 0 文本/1 十六进制。
十六进制支持 `!count;hex;`，末尾重复区可由 ST 结束；0 重复计数按 1 处理。空定义删除对应项。

| 资源 | 上限 |
|---|---:|
| 宏数量 | 64 |
| 每会话所有已解码定义 | 262,144 bytes |
| 单个传入编码定义 | 524,288 bytes |
| 嵌套调用深度 | 16 |
| 单个物理 Output 事件的总展开量 | 1,048,576 bytes |
| 单次原子事务的总展开量 | 4,194,304 bytes |

宏存储共享不可变字节，事务复制不重复复制全部定义。调用预算跨同一 Output 内的多次调用累计。
取消/畸形/超限不提交部分定义、网格、ACK 或宿主回复；不影响其他会话。
调用期间拒绝重定义；宏内 RIS 清空定义并取消当前嵌套展开的剩余字节，后续物理输出继续处理。
宏可以留下未完成 parser 状态；此时 checkpoint 仍不合格。

Snapshot 2.16 required feature `0x20000000`；新增 `0x18` 宏 section，严格 ID 顺序、长度/总量/重复项校验。
最大 canonical snapshot 61,932,836 bytes，64 MiB 硬上限不变。无宏状态不升级旧快照。

新增诊断 V3（8 KiB 上限）保留物理调用的 V2 字段，附加 `expanded_source=macro` 及 `expanded_*` 字段。
展开坐标的 sequence 是宏 ID，offset 是已解码宏字节偏移，不能冒充源事件坐标。首次内部失败保留，不被外层展开覆盖。
V1/V2 导出保持原样；普通错误经 V3 导出仍采用 V2 文本。无关控制字符串载荷继续脱敏。
坏十六进制仅公开具体拒绝字节、码点和 retained payload 索引，不公开完整宏载荷。

宏留下半条控制序列后由物理输出/外层宏补完的诊断使用 `expanded_source=mixed`，展开坐标为逻辑字节位置，
`expanded_origins` 分别记录 `macro/output,ID,offset,length`，最多 16 个连续来源段；未保留部分用
`expanded_origin_omitted_bytes` 明示，不猜补来源。载荷脱敏贯穿混合来源。Android 严格核对来源段、总长度、
溢出及上下层版本/影响一致性；Windows DPAPI 存储支持 8 KiB V3 并保留首个根因。

真实 ConPTY 输入测试发现单字节 C1 被 UTF-8 管道替换。SessionHost 四条输入出口统一通过跨写入 UTF-8
转换器，将独立 C1 编码为 UTF-8；合法中文/emoji 连续字节保持原样。核心协议字节与输入排序不变。

## 已有证据与待验证边界

应用日志在 `artifacts/validation/`：

- `s4-traditional-core-20261010.log`：539 core、10 golden、50 FFI 通过；后续标题/VT52 DEL 修复纳入宏全量回归。
- `s4-traditional-dotnet-20261010.log`、`s4-traditional-android-20261010.log`：新五库绑定通过，API36 模拟器 34 项 JNI/View 通过。
- `s4-traditional-conpty-20261010.log`：真实进程输出的 UPSS/字符集/C1 切换/VT52/DOCS 在 bundled ConPTY 保留。
- `s4-traditional-sessionhost-20261010.log`：SessionHost、三条鼠标路径、原生显示管道及窗口尺寸回归通过。
- `s4-macros-dotnet-20261010.log`、`s4-macros-android-20261010.log`：宏恢复、回复顺序、显示及递归回滚通过，模拟器 35 项 JNI/View 通过。
- 宏 V3 诊断的最新重建/验证单独保存在 `s4-macro-diagnostics-*`；未完成的命令不能视为通过。
- `s4-mixed-diagnostics-core-20261010.log`：552 core、10 golden、51 FFI；对应 Clippy 和五库导出通过。
- `s4-mixed-diagnostics-android-20261010.log`、`s4-mixed-diagnostics-jni-20261010.log`：API/native 单测及模拟器 35 项 JNI/View 通过。
- `s4-macro-conpty-20261010.log`：包括宏在内的 12 组实际输出序列保留；`s4-c1-input-conpty-20261010.log` 验证实际 SessionHost 输入出口的 C1 及 Win32 按键语义。
- `s4-mixed-diagnostics-surface-20261010.log` 和 `s4-mixed-diagnostics-native-window-20261010.log`：队列、Skia 渲染及真实窗口输入、手机尺寸调整/还原、epoch/过期/屏幕限制通过。

尚须完成：生产 capability/profile 协商、真实 Relay 恢复与混合版本验证、ARM64 运行和长测。
C10/C11 仍未标记“产品完成”。
本阶段未覆盖 C09 页、宿主动作、图形资源、History/Delta 等后续工作。

Windows 原生输入队列另已修复鼠标 release/focus 预留、连续 move 合并及渲染饥饿；
原生展示不注册外层回复期待，避免无人消费的队列增长。证据为 `s0-native-backpressure-*`。
