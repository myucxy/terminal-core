# 查询、SGR 栈与备用屏幕滚轮开发记录

2026-10-09。开发中，未完成全部 22 组能力，未正式发布或升级主客户端。

## 本批行为

- C07：DA3、DECREQTPARM、private DSR 15/25/26/53/55/56/75/85、DECRQDE、DECRQCRA。
  DA3 使用固定虚拟标识 00000000；串行参数为兼容常量，不代表网络速率。
  矩形 checksum 为 DEC 16 位减法，包含 UTF-16 字符单元、保护/显示属性与低 16 色索引；
  truecolor 采用默认颜色编号。现阶段仅活动页 0/1，页和宏相关报告仍随 C09/C11 完成。
- C02：已登记 DA/DSR/终端参数/XTVERSION 的未知数字只读 selector 无回复且不改变状态；
  非法语法、未知有状态指令仍保持事务失败。不是忽略所有未知 CSI。
- C06：`CSI #{/#p`、`CSI #}/#q`，全属性或 1/2/3/4/5/7/8/9/21/30/31 选择性保存恢复；
  无参数保存完整 SGR（含 truecolor、扩展下划线、下划线颜色、overline）。显式未知 selector 是空保存。
  全终端共享 10 层固定栈，溢出淘汰最旧层，空 pop 无作用；不保存 protected/hyperlink。
  4/21 分别维护普通/双下划线语义；主副屏共用，DECSTR 保留栈，RIS 清空。
- H04：1007 默认关闭；备用屏幕且无鼠标跟踪时，竖向滚轮按当前光标/Kitty 协议编码方向键。
  鼠标跟踪优先；主屏、关闭模式、横向滚轮不生成替代方向键。编码不把 1007 声明成触摸鼠标报告。

## 状态格式

Snapshot V2.11 新增 required feature：SGR stack `0x800000`、alternate scroll `0x1000000`。
SGR 栈是 critical section `0x10`，每项保存 mask/rendition ID；最多 10 项，校验计数、掩码、引用、长度和 CRC。
1007 存储于 MODE_BANK section version 5。仅使用新状态时输出 V2.11；旧状态保持旧格式字节。
快照最大保守预算增加至 44,892,825 字节，仍小于既有 64 MiB 上限。
旧核心不理解新 required feature 会拒绝；产品附着前的 profile 协商仍待接入，不把拒绝快照算作完整混用支持。

## 验证

- Rust 1.97.1 fmt、Clippy `--workspace --all-targets -D warnings` 通过。
- 最新全量：498 core + 10 golden + 49 FFI = 557，0 失败。
- 新库经 .NET NativeTerminalCoreValidation 与 SessionHostValidation 通过，包括实际 nested ConPTY 既有鼠标/resize 回归。
- Android API/native 单元 23+126=149，模拟器 API 36 的真实 JNI 27 项通过，0 失败/跳过。
- 已重新导出 DLL 和四 ABI SO，应用锁文件校验通过，`source_dirty=true`。未分发。
- 当前 DEV 版本现场核验仍为 0.3.3 / 132，不递增。

日志在应用 `artifacts/validation/s1-extended-query-*`、`s2-sgr-stack-*`、`s2-alternate-scroll-*`。
先前失败的开发测试保留在执行历史，最终上述日志为修复后重跑结果。

## ConPTY 实测前置

隔离真实子进程经项目自带 ConPTY：SIXEL、Kitty、DRCS、OSC7/133、SGR 栈样本原始序列完整到达。
同机 Windows 10 build 19045 系统 ConPTY：前三种图形控制序列丢失，OSC7/133/SGR 栈保留。
这是传输证据，不是图形解析/展示完成。图形 profile 必须绑定可保留序列的后端，不能静默回退系统后端。
日志 `s0-conpty-protocol-preservation-20261009.log` 与 `s0-system-conpty-protocol-preservation-20261009.log`。

## 参考与边界

行为参考 xterm ctlseqs（XTPUSHSGR/XTPOPSGR）和 Microsoft Terminal
`6676c6f938f7e98ad680f0cecedd3bafb6529734` 的 sgrStack/adaptDispatch（MIT）；实现为本仓库独立 Rust 代码。
没有复制上游实现，也没有引入 Termux 构建依赖。协议固定栈语义不代表平台 UI 已验收。
产品 Relay、真实手机、性能/soak、旧版本混用、完整诊断元数据等仍按全量计划执行。
