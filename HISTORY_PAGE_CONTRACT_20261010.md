# E02 immutable History Page V1：实施契约

这是开发契约，文字阶段完成情况见 [S5 实现记录](S5_IMMUTABLE_HISTORY_20261010.md)。

- 捕获 primary/page 1 的已封存历史逻辑行；当前可编辑 viewport、未结束的软换行逻辑行不作为 sealed history 返回。
  alternate screen 不替代 primary history。capture 冻结 revision、Unicode width profile、OSC 色表/链接和完整行片段。
- exclusive `before_line_id` 使用 capture 内的准确行 ID；`u64::MAX` 表示从最新端开始。
  ID 不是排序键，不能用数值比较推测位置。请求必须匹配 capture token 与 revision；错终端/失效 token 不回退到实时内容。
- 页面包含完整 logical lines，按展示顺序返回；通过 `next_before_line_id` 向前读取。
  head-truncated 单独标记，绝不将缺失的历史头补成空格，也不把 live tail 当作“完整逻辑行”。
- 复用 Render V4 编码格子/字形、颜色、超链接、双宽/双高行。页面可包含多个最多 512 物理行的 V4 子帧，
  子帧只负责可视数据，page 的逻辑行目录负责连续性；子帧边界不是分页边界，不改变 logical line ID/offset。
- 每页最多 128 个逻辑行、65,536 cells、4 MiB UTF-8 和 16 MiB wire；默认预算可更小。
  若最新待取整行超出请求预算，返回明确的 budget/required-size 错误，不截断字素或悄悄跳过该行。
- 同一 capture、参数和 anchor 返回 byte-identical 页面，实时输出/resize/ED3/关闭不改变已有 capture。
  新 capture 反映新状态，过期/释放后的 token 返回 stale。
- core 不读取时钟。C ABI 宿主 registry 负责 token 生命周期、数量/内存预算；JNI/.NET 自动释放，
  产品层同会话仅保留一个浏览 capture。固定五分钟，单项 64 MiB、每库 64 项/256 MiB；过期清理在 registry 操作时执行。
  过期时明确提示刷新，不能隐式换内容导致读到不同记录。
- 新接口 additive，不修改既有 checkpoint/Render V4 bytes。页内所有 V4 子帧的 revision 必须相等，
  decoder 先验证长度/数量/CRC/目录连续性，再分配/呈现；损坏内容不更新历史游标与实时 ACK。
- H01 命令标记与图形资源加入后需同步进入 history capture 和页目录，不能将当前文字阶段冒称最终 E02 完成。

验收：分页重试、整行/宽字符/组合字符、极长逻辑行、淘汰头、非单调 ID、alternate/ED3、resize 后捕获隔离、
错 token/revision/anchor、资源超限、capture 释放、两会话隔离、DLL/JNI 精确 bytes、Windows/Android 历史滚动。
