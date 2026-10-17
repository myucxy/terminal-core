# terminal-core 0.1.0-rc.1 发布记录

2026-10-18：[GitHub Release](https://github.com/myucxy/terminal-core/releases/tag/v0.1.0-rc.1)
已发布为 **prerelease**，非 Draft，不设为 latest。仓库仍为私有，可见性未更改。

## 固定来源与附件

- 源码 / tag 对应 commit：`3964186547b14f91a9180b7fc0763b0987d8e3f4`，`source_dirty=false`。
- Rust 1.97.1；Android NDK 27.0.12077973 / API 26；Windows x64 MSVC 静态 CRT。
- C ABI v1；Snapshot 上限 2.22、Render Frame 上限 V5，实际语义由 profile 与消费端绑定决定。

| 附件 | 大小（bytes） | SHA-256 |
| --- | --- | --- |
| [原生依赖包](https://github.com/myucxy/terminal-core/releases/download/v0.1.0-rc.1/terminal-core-v0.1.0-rc.1-native.zip) | 4,822,047 | `ef5c4e20e72cab1bbbdaaa76588e1658c2234cc5440b0069d53a1b0ac6ca5788` |
| [对应源码包](https://github.com/myucxy/terminal-core/releases/download/v0.1.0-rc.1/terminal-core-v0.1.0-rc.1-source.zip) | 2,643,789 | `4d3f513897f282f96fda66b7bdcd9f6d8ebd7fffc367f8d2308439dae2d5ef74` |

另有 `core-release.json`、`SHA256SUMS`、`SHA256SUMS.sigstore.json`、`core-signing.pub`，共 6 个附件。
公钥文件 SHA-256 为 `3b4b7b833f108cdcf47ffb35cc44c5c8d11b54a5fd1925ed4ea4a16594d7f3e2`。
独立 core 签名身份不替换 Android、Windows 或 Docker 的签名密钥；私钥和授权令牌未进入 Git 或附件。

## 已验证

- Rustfmt、Clippy `-D warnings`；754 core + 10 golden + 73 FFI，共 837 项通过。
- 发布工具 19 项检查通过：不安全路径、ZIP traversal、重复路径、特殊文件、解压预算、
  文件损坏、dirty 库与覆盖既有包等拒绝边界；真实签名校验拒绝篡改后的 SHA256SUMS。
- Windows 新 DLL 通过真实应用绑定、Delta V1 与冻结历史预算验证；绑定验证构建 0 警告 / 0 错误。
- Android x86_64 模拟器、MI 6 ARM64 与 ARM32 均直接加载候选 SO，通过 60 个 JNI 方法注册、
  ABI、UTF-8、render、snapshot round-trip 与 resize 冒烟；没有安装或替换用户 App。
- Android 四 ABI 的 ELF 架构和 LOAD segment 16 KiB 对齐核验通过。
- 源码包含 796 个清单文件与 vendored 依赖。离线测试及 Windows Release 重建通过；
  签名验证后的源码 ZIP 解压到新目录，以空 Cargo 缓存再次离线运行 837 项测试通过。
- 五库在隔离应用目录导入成功，应用现有 Verify-NativeArtifacts 校验 5/5 通过。
- Draft 上传后全部 6 个附件下载回验；Publish 再次下载验证签名、归档、源码逐文件清单和五库哈希后发布。
- 新发布提交与原生包的 Gitleaks 扫描未发现密钥泄露。

## 边界

x86 库完成编译与 ELF 检查，现有模拟器没有 app_process32，未做 x86 runtime 冒烟。
本批不替代完整图形、OEM、混版本、性能或长稳产品验收。应用实际五库与锁文件保持
`1c907c51b3c6652b23a7d7216ff1bb13d3410a69`，未升级应用、发布客户端或部署服务器。

复用步骤见[发布与固定版本导入](RELEASE.md)。接收者需要仓库读取权限，且须能取得对应源码附件。

## English summary

Published as a private-repository prerelease, with six signed/checksummed assets bound to clean commit
`3964186547b14f91a9180b7fc0763b0987d8e3f4`. Both upload and publication downloaded and reverified all assets.
837 Rust tests, Windows binding/Delta/frozen-history validation, and JNI smoke on x86_64, ARM64 and ARM32
passed. Vendored corresponding source passed offline tests with an empty dependency cache and a Windows
release rebuild. x86 runtime and full product/graphics/OEM/mixed-version/performance/soak acceptance remain
outside this release evidence. The application dependency and installed clients were not changed.
