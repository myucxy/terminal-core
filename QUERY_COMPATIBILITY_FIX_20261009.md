# Claude 启动查询及首批优先兼容修复

日期：2026-10-09。状态：代码与本地验证完成；尚未正式发布或替换运行中的客户端。

## 本轮实现

| 对照 ID | 本轮结果 | 仍保留的边界 |
|---|---|---|
| C01 / Q0 | `14/15/16/18/19t` 不再导致核心 FailedClosed；18t 回复权威行列数 | 像素与完整屏幕几何无可信共享来源，消费并允许应用超时回退，不伪造 10×20 字体像素 |
| C02 / Q0 | 对已知只读尺寸查询族的数值参数变体采用无回复降级 | 未全面放开未知 CSI/DCS、模式或 SGR；坏快照、序号、资源上限及有状态未知指令仍严格拒绝 |
| C05 | 实现精确 `CSI !p` 软复位，保留文本/历史/光标位置/标签/标题/备用屏幕 | C05 的 ESC 6/9、`CSI ?5W` 尚未补齐；没有把软复位变成硬复位 |
| C03 / E04 | 2027/2031 的 DECRQM 返回 0，停止声明未实现的字素/主题报告能力 | 保留旧快照位及 setter 的字节兼容；完整 UAX #29、国旗宽度和主题改变通知仍是后续工作 |
| E01 | 应用 Windows 回复过滤扩展到 4/5/6/8/9 几何族；18t 按 source sequence/offset 与 authority 回复合并 | 不吞掉普通方向键、错误类型/尺寸或没有查询期待的字节；不把外层像素作为 authority 状态 |

没有在 Android 输入按钮、某个 agent 或网络层按字符串删除输出。本次只读查询分类位于共享 core。
Host Effect V2 新增严格的 `CSI 8;rows;columns t` payload 子集（两个值均为规范十进制、范围 1..512）；
Rust、.NET 和 Android Kotlin 校验同步更新。C ABI/JNI 名称、结构体和 Snapshot 版本均不变。
旧 Host Effect V1 不扩展此语法；请求回复时返回可重试 `HostEffectVersionRequired`，不将状态置为 FailedClosed。

DECSTR 只重置 active screen 的 saved cursor/charset，不污染备用屏幕后方主屏幕保存的光标。
9001 Win32 input、鼠标、焦点、粘贴及 Kitty 协商不因软复位被擅自重置。

## 验证

- Rust 1.97.1：468 core 单元测试、10 golden、48 FFI 测试；fmt、Clippy `-D warnings`。
- Windows .NET：NativeTerminalCoreValidation 和 SessionHostValidation，包括回复分块、不同几何族乱序、
  18t source coordinate 合并、失败投射取消、不误抢输入控制；既有 nested ConPTY 鼠标和窗口调整回归。
- Android：terminal-engine-api 21 项、terminal-engine-native 126 项单元测试；API 36 模拟器 22 项 instrumented tests。
  新 JNI 用例从 checkpoint 逐字节重放 Claude 查询，验证回复、继续写字、DECSTR 及再次 checkpoint 恢复。
- 故障会话真实 WAL：修复 DLL 回放 36/36 成功，未删除 `16t`、未改变原历史；原基线在第 34 条失败。
- 实际安装的 Claude CLI：新建隔离 PowerShell/managed SessionHost，测试桥接确认 ACK；观察到真实 `16t` 后仍存活，
  Ctrl+C 退出 CLI 后 shell 执行标记命令，SessionHost `authority_failed=false`，最终退出码 0。
  仅启动/退出，没有向模型发送任务；不操作用户已有会话。该测试是隔离本地链路，不等于公网 Relay 联测。
- 统一构建 1 个 Windows DLL 和 4 个 Android ABI SO，导出至同级应用，五库哈希校验通过。

对应命令：

```powershell
# core 仓库；工具依照 D:\Dev 清单核验后运行
cargo +1.97.1 fmt --all -- --check
cargo +1.97.1 clippy --workspace --all-targets --locked --offline -- -D warnings
cargo +1.97.1 test --workspace --locked --offline
./scripts/Build-ToTermLink.ps1 -Development

# 同级应用仓库；DEV 基线经 AppShip 现场核验为 0.3.3/132
dotnet run --project win-agent/tests/NativeTerminalCoreValidation/TermLink.NativeTerminalCore.Validation.csproj --no-restore -p:Version=0.3.3 -p:FileVersion=0.3.3.132
dotnet run --project win-agent/tests/SessionHostValidation/TermLink.SessionHost.Validation.csproj --no-restore -p:Version=0.3.3 -p:FileVersion=0.3.3.132
# 可选真实 CLI；传入已安装工具的绝对路径，只启动隔离的新会话
dotnet run --project win-agent/tests/SessionHostValidation/TermLink.SessionHost.Validation.csproj --no-build -- --claude-startup-probe <pwsh.exe> <claude.cmd>

# 应用 android/，ANDROID_SERIAL=emulator-5556；使用仓库 Gradle 8.13
./gradlew.bat :terminal-engine-api:testDebugUnitTest :terminal-engine-native:testDebugUnitTest :terminal-engine-native:connectedDebugAndroidTest '-PtermlinkVersionCode=132' '-PtermlinkVersionName=0.3.3' --offline --console=plain
```

本机日志在同级应用 `artifacts/validation/`：`core-query-fix-*.log`、`geometry-*.log`、
`geometry-android-unit-summary-20261009.json`、`claude-cli-diagnosis-20261009-fixed.json`、
`claude-real-startup-fixed-20261009.log`。应用归档同名说明记录平台文件及后续门禁。

## 接续与交付限制

本轮导出为本地 DEV，锁文件明确 `source_dirty=true`，不是已提交 revision 的正式发行库。
运行中的 Windows 客户端和 Android 主应用仍是原版本；模拟器只安装/运行了独立 instrumentation 测试包。
服务端无改动。尚未执行真实手机 App → 公网 Relay → 更新后 Windows 的整条产品链路。
正式发布前需固定 core/app revision，重新导出 clean 库、同步重建双端，按发布手册完成联测及审核；旧候选包不能复用。

后续优先：完整 Unicode width profile/快照兼容设计、剩余可选查询策略、C05 补充编辑、H01 shell 目录/命令标记。
不会将本轮报告为 18 组差距全部完成。历史对照报告保留修复前基线，以上表格说明本轮覆盖范围。
