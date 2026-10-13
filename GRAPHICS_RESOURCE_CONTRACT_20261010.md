# S6 共用图形资源契约（开发中）

用于 C12 DRCS、C13 SIXEL、G01 Kitty 的同一资源仓库。协议行为参考：
[VT330/340 SIXEL](https://vt100.net/docs/vt3xx-gp/chapter14.html)、
[Kitty graphics](https://sw.kovidgoyal.net/kitty/graphics-protocol/)。仅参考公开行为，不复制实现或图片。
本机参考正文留在应用 ignored `artifacts/validation/*-spec-20261010.*`。

## 资源与预算

- 每个不可变资源由永不复用的非零 u64 ID 标识。替换创建新资源；删除不回退 next ID，reset 也不复用 ID。
- 格式为直通 RGBA8888（未预乘）或 A8（DRCS mask）；尺寸各 1..4096，长度必须严格等于宽×高×像素字节数。
- 单资源 16 MiB，每 session 32 MiB，最多 256 资源；进程共享 payload 合计 256 MiB。尺寸乘法/总量均 checked。
- transaction、checkpoint、冻结历史使用 Arc 共享不可变像素；删除仅解除本仓引用，最后一个引用释放时归还进程预算。不得修改旧 capture 可见的内容。
- 解析临时画布也计入预算，分配前登记。SIXEL payload 上限 4 MiB、像素工作量 16,777,216；placement 最多 4096，每轴最多 4096 cells。PNG/Kitty 输入上限见下文，解压输出受调用方预算限制。
- 全局资源限制只决定该进程能否接受新资源，不改变资源的 canonical ID/bytes。预算拒绝不能提交半资源或推进 next ID。

## 独立 TLGRS1 编码

48 字节头：magic `TLGRS1\0\0`、u16 version=1、u16 header=48、u32 total length、
u64 next ID、u32 count、u32 reserved=0、u64 pixel bytes、u32 CRC32C（跳过该字段）、u32 reserved=0。

资源按 ID 递增：32 字节头（u64 ID、u32 width、u32 height、u8 format〔1 RGBA/2 A8〕、
3 bytes reserved=0、u32 data length、u32 payload CRC32C、u32 reserved=0），接原始像素及 4 字节对齐零 padding。
严格拒绝零/重复/倒序/越界 ID、未知格式、尺寸/长度/预算溢出、非零 padding/reserved、CRC 错误和尾随数据。

该编码用于实验 Terminal/Snapshot、Render V5 和冻结历史；C ABI/JNI 已有显式图形入口，.NET/Kotlin 已有独立解码器。
产品 profile 仍为 `2.21_v2`，图形能力未开启；S7 资源 Delta、RPC/传输预算与真实双端恢复验收仍未完成。

## 解码、布局与状态包（同日开发补充）

- 图像解码临时buffer与最终像素共享进程256MiB预算；PNG输入32MiB，单张最终RGBA16MiB。PNG含Adam7及标准位深，静态图形profile拒绝APNG。Kitty控制头1024字节、每段Base64≤4096、每张≤16384段/32MiB解码输入。
- 位置采用canonical有理数cell单位（不含本地浮点结果），只消费有序HostGeometry。每轴最多4096 cells，source crop限在资源实际尺寸。
- DRCS不占Unicode PUA，用A8 atlas资源及显式glyph矩形引用。TLDRCS1固定192字节，两组font×两宽度rendition，保存load generation和defined bitmask。
- TLGS1头64字节，之后依次TLGRS1、TLDRCS1、768-byte SIXEL palette、24-byte Kitty identity条目、168-byte placement条目。头保存next placement/Kitty ID、各计数、resource bytes与CRC32C；offset 44为u32 DECSDM scrolling布尔值，其余padding/reserved为0。placement最多4096，资源身份最多256。
- placement在原144字节布局后新增offset 144/148的i32 origin_x/origin_y及152/156/160/164的u32 clip left/top/right/bottom。origin相对行锚点，clip是相对完整allocation的半开cell矩形；裁剪不重采样source像素。TLGRS1最大33,563,440 bytes，TLGS1最大34,258,736 bytes。
- placement锚点page1..6主屏/DEC页，page7备用屏；logical line ID允许0（core初始行），cell offset由Screen解释。TLGS1验证资源引用；Terminal Snapshot额外验证对应page及row/cell锚点存在。
- Snapshot 2.22使用必需feature `0x8_0000_0000` 和required section `0x500`承载TLGS1，强制保存row anchors；grapheme table v2保存Cell/REP的显式glyph引用，charset section v4保留原始designation。glyph及placement引用在像素物化前预检，退役atlas允许由旧Cell/历史继续引用。
- 仅显式图形Snapshot API接受128MiB，旧Snapshot 2.0–2.21仍64MiB且canonical bytes不变；旧C/JNI导入拒绝2.22，V1–V4 render拒绝图形profile。源码独立保守界为111,470,186 bytes：旧66,070,235 + 39目录/对齐 + 34,258,736 TLGS1 + 20×557,057 glyph扩展 + 36 charset扩展，低于128MiB。产品RPC/crypto/持久历史预算仍待同步，不以放大core常量视为完成。

## Render V5 开发帧

Rust、显式C ABI/JNI入口和独立.NET/Kotlin解码器已接通，仍为开发契约。上限64MiB、4096 image draws；glyph draws不超过当前网格cells（最多262,144），每cell最多一条。

64-byte 头使用小端：

| 偏移 | 字段 |
|---|---|
| 0 | 8-byte magic `TLRFRM5\0` |
| 8 / 10 | u16 version=5 / header=64 |
| 12 | u32 total bytes |
| 16 | u64 revision |
| 24 / 28 | u32 columns / rows |
| 32 / 36 / 40 | u32 text V4 bytes / TLGRS1 bytes / image draw count |
| 44 | u32 glyph count |
| 48 / 52 | u32 required features=7 / CRC32C（跳过自身） |
| 56 | u64 reserved=0 |

随后依次是完整 V4 text、TLGRS1、144-byte image draw数组、32-byte glyph draw数组：

| 偏移 | 字段 |
|---|---|
| 0 / 8 | u64 internal placement / resource ID |
| 16 / 20 / 24 / 28 | i32 column / i32 row / i32 z / u32 Kitty image ID |
| 32 | source x/y/width/height，4×u32 |
| 48 | destination x/y/width/height，4×（u64 numerator / denominator） |
| 112 / 116 / 120 / 124 | u32 allocated columns / rows / protocol（1 SIXEL、2 Kitty）/ reserved=0 |
| 128 / 132 / 136 / 140 | u32 clip left / top / right / bottom |

glyph draw的offset 0/4为u32 column/row，8为u64 resource，16/20/24为u32 atlas x/width/height，28为u32 reserved=0；atlas y固定为0。记录按row/column严格递增，目标必须为V4中单列、非continuation的单字符cell，资源必须为A8。

负column/row表示完整图片部分在窗口外；消费者同时裁剪到placement clip和viewport。图片按z、image ID、internal placement ID排序；
低于-1073741824的z绘制于非默认背景下，其他负z在文字下，非负z在文字上。
文本revision/行列必须与外头完全一致；仅携带实际被image/glyph引用的RGBA/A8资源。解码先验证全部引用、比例、坐标、CRC和V4字形目标，
再复制文本/像素，避免坏 placement 触发大块分配。旧 V4 解码门禁不放宽。

Windows受控Skia验证已有2092项断言、8个Rust真实fixture通过；Android绘制尚待测试。不能用独立解码或软件像素测试代替真实PTY、Relay和ARM64真机验收。

## History V2 开发封装

`TLHPV2\0\0`使用96-byte小端头，u16 version=2/header=96；offset 16/24/32/40为capture ID/revision/requested anchor/next anchor，48/52/56/60/64为line count/frame count/columns/cells/UTF-8 bytes，68为分页标志，72为required features=1，76..92保留为0，92为跳过自身的CRC32C。之后为40-byte logical-line目录及u32长度前缀的Render V5 fragments；旧TLHPV1继续只接V4。

V2页上限64MiB，单次请求最多128 logical lines、65,536 cells、4MiB UTF-8；图形capture上限128MiB。V1页/capture仍为16/64MiB；FFI registry仍最多64 captures、合计256MiB。capture配额计入其持有的共享像素，进程像素预算对同一Arc分配仅登记一次。冻结capture保留主屏图形资源与锚点，不受后续下载、删除、ED3或resize修改；页和viewport走显式V2入口。

## 实验输入与生命周期边界

Kitty APC 只在 `new_with_graphics_profile` 开启。上传的不可变分块链不进入快照；有未完成上传时 checkpoint/Render 不可用。
成功或可选 query 的失败回复作为同一事务的 Host Effect，publish 前不得发到 PTY。动态文件/共享内存请求仍明确拒绝，不在 core 打开路径。
有效 query 的失败返回 EINVAL/ENOSPC/ENOTSUP 并继续会话；有状态的不支持指令仍按诊断门禁处理。
DRCS已接下载、原始designation、GL/GR/SS、Cell/REP、copy/reflow、Snapshot及冻结历史；旧Cell持有不可变A8引用，重新下载不会改写旧字形。
SIXEL已接真实Output分派、SGR/OSC/DECAC当前色、独立VT340 palette、透明背景及有序HostGeometry。仅图形profile接纳DECSDM：默认及`?80h`启用从文本cursor起点滚动，`?80l`从页左上绘制并保留文本cursor；失败或取消不提交半资源、palette或滚动。

Kitty删除、备用屏清除及placement滚动日志已接入；局部边距裁片、历史淘汰、ED3、resize/reflow和行属性改写采用重锚与clip保留仍可见部分。SIXEL与Kitty的擦除策略分开处理，ECH等新增hooks和性能cache还需最终回归。
最新已核实统一基线`s6-integrated-motion-20261010.log`为713 core+10 golden+63 FFI通过，发生于上述cache/ECH追加修改之前；不能当作最终源码全量通过。Windows broker、S7资源Delta/tombstone、RPC/crypto/持久历史预算、immutable产品profile与真实Relay/ARM64门禁仍未完成。产品保持`2.21_v2`，本轮未导出五库、安装客户端或发布。
