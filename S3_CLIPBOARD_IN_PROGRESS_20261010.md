# H03 开发中：剪贴板写入请求

2026-10-10。核心、双端消费者与逐次确认已接入，本地测试通过；**真实产品 Relay、系统剪贴板及真机验收尚未完成，不能声明 H03 产品完成**。

- OSC52 `c;Base64` 和 `;Base64` 仅写 system clipboard。`?` 读取明确拒绝，其他 selection 明确拒绝。
- 严格标准 Base64：完整 quartet、末尾规范 padding/零残余位，无空白/URL-safe 变体；UTF-8 文本最多 16 KiB，允许空串清空请求，拒绝 U+0000。格式错误携带 payload 位置与具体 token；解码后错误标明位置是 Base64 quartet。
- Host Effect V2 PlatformAction payload `[3,0,0,0] + u32le(length) + UTF8`；每次 capture 最多 4 个请求，原子超限。无 capture 只验证；V1 capture 不能表示时原子拒绝。
- 请求不入 Snapshot，也不生成读取回复。旧外层投射始终移除 OSC52，含宏展开。
- Kotlin/.NET 类型化严格解码已接入；Windows 原生 display action 长度升级为 u32，允许有界文本。
- 协议身份为 `terminal_state_2_21_v2;width=1;output=0;resources=1`；Snapshot 格式仍 2.21。五个 dirty DEV 库已同步导出，真实 DLL/JNI 测试通过，安装中的主客户端未改。

已过：`s3-clipboard-core-20261010.log` 的 3 个专项用例（全部拆帧点、UTF-8/Base64/最大长度、V1/突发上限）。

## Android

- 云朵菜单开关默认关闭；开启仅允许提出逐次确认，不会自动复制。实际显示中的 live 引擎供应当前未过期控制租约（ID/version），只读/历史引擎不供应许可。
- receipt 持久提交后捕获许可，再切主线程；展示前和用户确认时均复核账号、会话可见、开关和同一租约。确认还要求对话框窗口有焦点，30 秒到期；100ms watcher 关闭失效提示，全局最多一个。
- 预览最多 240 字符，转义控制/Unicode Format 字符；复制精确原文。不读取剪贴板、不把正文写入普通日志。
- Robolectric 的 dialog 焦点通知需要 drain main looper；修复的是测试驱动，没有削弱产品焦点条件。

## Windows

- 原生窗口“提醒”菜单增加默认关闭的逐次复制开关；沿用持久 receipts（拒绝和后台丢弃同样登记）。跨窗口命名 semaphore 最多一个提示，30 秒到期，失去焦点/关闭/取消/开关关闭拒绝。
- 经过当前用户/PID 验证的 display pipe → SessionHost → 已认证的 Agent pipe → 当前 ProductionRuntime。两次都使用新 GET lease；只接受明确的 `404 LEASE_NOT_FOUND`。活动手机租约、其他 404、403、登录失效、网络异常或会话非活动均拒绝，不调用 force-release，不抢手机控制。
- Host gate 使用 request UUID、两阶段消费、3 秒查询期限以及 local/remote input epoch。查询与日志 ACK 独立；新 IPC 22/23 使用 seq=0，不落 journal、Snapshot 或 Relay。
- 确认时重新查询当前租约，并核对同一 Host 输入 epoch；批准响应只消费一次。服务端租约检查与本机 OS 调用不构成跨机器原子事务，最终决定点是 Host 回执；未收到的纯远端租约变更仍需真实控制切换/弱网验收。
- 使用 Win32 Unicode clipboard 写入；仅确认后分配/移交文本，不读已有剪贴板。UI 测试替换 OS writer，未覆盖真实系统剪贴板/锁屏。
- 当前仅显式 `--native-display` 原生窗口接入。旧外层窗口抑制 OSC52，不开放 raw 旁路；默认展示迁移仍未完成。

## 验证证据（应用 artifacts/validation）

- `s3-clipboard-full-20261010.log`：597 core + 10 golden + 59 FFI；`s3-clipboard-clippy-20261010.log`：all-targets -Dwarnings。
- `s3-clipboard-export-20261010.log`：五库；`s3-clipboard-native-20261010.log`：真实 DLL 文本/非法 UTF8/NUL/安全投射。
- `s3-clipboard-android-validation-20261010.log`：API36 emulator-5556 的 46 JNI 项、core-session/core-network 及 7 产品单元项。
- `s3-clipboard-product-20261010.log`：预览 Format 转义改动后的 7 产品单元项。
- `s3-clipboard-win-{ui,agent,host}-build-20261010.log`：Desktop 双目标、Agent、SessionHost 编译无错误/警告。
- `s3-clipboard-surface-20261010.log`：receipt 跨实例、防后台重放、两个授权阶段/输入 fence/超时/取消、Avalonia headless 实际按钮流程（OS writer 替换）、真实 Skia 绘制。
- `s3-clipboard-profile-20261010.log`：真实认证 Agent IPC，授权等待期间仍持久 ACK，许可/拒绝，授权不入 WAL；原 profile 122 assertions 通过。
- `s3-clipboard-sessionhost-20261010.log` 和 `s3-clipboard-recovery-20261010.log`：ConPTY 输入/尺寸/生命周期/display pipe、生产附着恢复回归通过。

复核为同一执行者 self-review，无独立审查者。未提交/发布/部署/替换主客户端。剩余：真实 Relay 手机与 Windows 控制切换、切账号/锁屏、真实 clipboard、厂商后台与升级混版本；整批 22 组仍开发中。
