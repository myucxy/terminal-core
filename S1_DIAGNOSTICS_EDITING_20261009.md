# S1 首批：协议诊断与剩余编辑指令

日期：2026-10-09。状态：已编码、本地核心与绑定验证通过；不是 S1 或全量 22 组完成声明。

## 本批范围

保留先前 Claude 几何查询/DECSTR 修复，新增 C02 的输出失败诊断和 C05 剩余编辑。
Windows 仍为会话权威，Android 使用相同核心作镜像；Server 无代码或配置改动。

| 指令/接口 | 实现与边界 |
|---|---|
| `ESC 6` / DECBI | 向左一列；在左边距且处于上下边距内时，将整个边距矩形右移一列。边距外只移动光标，屏幕边界夹紧 |
| `ESC 9` / DECFI | 向右一列；在右边距且处于上下边距内时，将整个边距矩形左移一列。修复宽字符边界、取消待换行，不追加历史 |
| `CSI ?5W` / DECST8C | 清除自定义 tab 并恢复每八列的默认位置，包括当前宽度之外的保存位置；省略参数采用 5，其他 selector/多参数/子参数明确拒绝 |
| `failure_diagnostic_v1` | 非状态 API；返回最近输出失败的有界 ASCII 证据。已 fail-closed 时保留首次根因，未提交候选仍回滚 |
| `tl_terminal_export_failure_diagnostic_v1` | additive C ABI，沿用 versioned buffer/同库释放；无证据返回 OK 空 buffer，最大 4096 字节 |
| JNI/.NET | 同库导出和严格长度/字符校验；旧库缺少导出时保留原状态错误，新 SO 可加载没有此可选方法的旧 Kotlin 绑定 |

DECBI/DECFI/DECST8C 只参考 Windows Terminal 固定版本
`6676c6f938f7e98ad680f0cecedd3bafb6529734` 的公开行为（MIT），未复制源码或增加依赖：
[BackIndex / ForwardIndex / TabSet](https://github.com/microsoft/terminal/blob/6676c6f938f7e98ad680f0cecedd3bafb6529734/src/terminal/adapter/adaptDispatch.cpp)。
TermLink 自有实现复用 Screen、TabStops、宽字符修复及既有快照。

## 诊断契约

`TL-DIAG/1` 是本地技术详情，不进入 Snapshot、Render Frame、PTY 回复或 Server 遥测。
字段为 category、reason、family、sequence、hex、start、end、observed_bytes、hidden_bytes、truncated_bytes。
事件内偏移从 0 开始，end 为结束后的位置；跨事件保留物理输入字节，不将 C1 改写后的虚拟 ESC 冒充原文。

示例为测试用未知模式，与真实用户会话无关：

```text
TL-DIAG/1
category=unsupported
reason=private mode
family=CSI
sequence=<ESC>[?9999h
hex=1B 5B 3F 39 39 39 39 68
start=1:2
end=2:5
observed_bytes=8
hidden_bytes=0
truncated_bytes=0
```

- 分类区分 unsupported、malformed、limit_exceeded；原有 C ABI 状态值不变。
- 原始预览最多 192 字节，跟踪新的 ESC 候选也独立有界；输出详情最多 4096 字节。
- 字符串载荷及其终止字节在 sequence/hex 同时隐藏。只在 parser 确认开始新指令后，才将新的 ESC/C1 后缀作为诊断证据。
- CR/LF、ESC、C1、非 ASCII、反斜线均可见转义，不原样执行。无法展示的部分明确计数，不猜补字节。
- 诊断独立于确定性终端状态，Terminal 相等性及 Snapshot 不包含它。操作失败不会推进事件序号、改变网格或发布半个 effect frame。
- 可重试失败之后的新操作清除旧证据；FailedClosed 则保留根因。两端结构化证据不加入通用异常 Message，避免被普通异常日志自动上报。

## 已执行验证

工具已按 `D:\Dev` 清单核验：Rust 1.97.1、.NET 8.0.303、JDK 21、Android SDK/NDK。
DEV Android/Windows 数字基线经现场核验仍为 0.3.3 / 132，没有递增。

| 检查 | 结果 |
|---|---|
| Rust fmt / Clippy（all targets，warnings as errors） | 通过 |
| Rust core / golden / FFI | 485 + 10 + 49 = 544 项通过，0 失败、0 忽略 |
| Windows NativeTerminalCoreValidation | 通过；覆盖异常根因、隐私、C05 网格与制表位 |
| Windows SessionHostValidation | 通过；实际嵌套 ConPTY 输入、鼠标、查询回复归属、生命周期和 resize |
| 隔离真实 PowerShell → Claude → 中断 → shell | 再次通过，观察到真实 CSI 16t；权威存活，shell 接受后续输入，退出码 0 |
| Android API / native / network / app 单元测试 | 23 + 126 + 338 + 387 = 874 项通过，0 失败、0 跳过 |
| Android 模拟器真实 JNI | API 36 / emulator-5556：24 项通过，0 失败、0 跳过 |
| DEV DLL / 四 ABI SO | 重新构建导出，5 个 hash 校验通过；四 ABI 编译不等于四 ABI 实机验证 |
| 双仓库 diff whitespace | 通过 |

新增测试覆盖每个事件断点、C1/无效字节、字符串打断后的错误归属、载荷隐藏、截断、事务回滚、FailedClosed 根因、可重试错误更新；
编辑覆盖上下左右边距、边距外光标、宽字符、主副屏、待换行、历史不增长、resize/tab 隐藏位和快照后缀。
本批复核为同一执行者 self-review，没有声称独立人员审核。

源码基线：core HEAD `e6e849172c6fca0de01ae0799eae97ad9483e687`，app HEAD
`d6fe9c6f4b21d9babc3c3b8f52ed6364455f5ab4`，均含未提交改动，HEAD 本身不是完整候选源码。
DEV 锁文件 `source_dirty=true`；最终库 hash 见应用 `terminal-core.lock.json`。
DLL SHA-256：`77c7931323adf0870184540c6d0fe9dc6b9d8e6a7f8e5277eda48359df690475`。

日志在应用 `artifacts/validation/diagnostic-editing-*-20261009.log`、
`diagnostic-editing-android-summary-20261009.json`；发布基线在 `diagnostic-dev-release-baseline-20261009.json`。

## 尚未关闭的工作

- C02 仍是部分实现：精确失败参数索引/U+ 码点、profile/revision/来源与实际影响的统一展示、非输出方向诊断、其余可选查询分类尚待补齐。
- Android 详情/复制 UI 已接入并编译，尚未在主 App 通过真实会话点按查看/复制；Windows 外层控制台退出后的持久错误入口也未补齐。
- C05 核心和双端绑定已通过；主 App 经公网 Relay 到更新 Windows 的产品验收未做。
- C01 可信 geometry、C07 扩展查询及 S2–S8 仍按全量计划继续，未删除任何低优先级能力。
- 本批未完成性能基线对比、长时间 soak、ARM64 真机、混合旧版、升级/回滚演练，不能据此宣布阶段或发布门禁全部通过。

未替换运行中的主客户端、未部署 Server、未提交/推送/上传正式版本。模拟器只更新独立 instrumentation 测试包。
所有产品验收继续 Relay Only；测试日志中的 `direct` 指本地直接 ConPTY 测试，不是 LAN/NAT。
