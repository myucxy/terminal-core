# core 原生依赖包发布

[中文 README](../README.md) · [English README](../README.en.md)

core 使用独立版本，不跟随 TermLink 应用版本。首次候选为 `0.1.0-rc.1`；候选发布不表示应用
全部协议、平台和设备已验收。版本、Git tag、干净源码 revision 和所有附件一经发布不覆盖。
本流程本机构建，发布到现有 GitHub 仓库，不更改仓库可见性、不使用应用发布渠道。

## 产物

- `terminal-core-v<version>-native.zip`：Windows x64 DLL、Android 四种 ABI SO、C 头文件、
  `terminal-core.lock.json`、包内 `core-release.json`、LICENSE、NOTICE 和第三方来源/许可。
- `terminal-core-v<version>-source.zip`：同一 commit 的全部 tracked 源码、JNI/CMake/脚本、
  Cargo.lock、vendored 依赖、静态 CRT 与 vendor 合并配置、逐文件源码清单和重建说明。
- `core-release.json`：版本、tag、revision、profile/ABI 上限、目标、工具版本及两份 ZIP 的大小/哈希。
- `SHA256SUMS`：两份 ZIP 与外部 manifest 的哈希。
- `SHA256SUMS.sigstore.json`、`core-signing.pub`：Cosign 签名 bundle 与公钥。

公钥文件 SHA-256 必须从维护者的独立可信渠道固定，不能只信任同次下载的公钥。
私钥和密码不进入仓库、Release、命令参数或日志。签名默认保留透明日志验证，不跳过校验。

当前独立 core 公钥位于 [`release/core-signing.pub`](../release/core-signing.pub)，文件 SHA-256：
`3b4b7b833f108cdcf47ffb35cc44c5c8d11b54a5fd1925ed4ea4a16594d7f3e2`。
这是 core 依赖包签名身份，与应用 APK/Setup 及 Docker 签名密钥分开。

## 本机准备

使用 `D:\Dev` 清单内工具，构建前执行 `Ensure-DevEnv.ps1` 检查 Rust、NDK 等所需工具。
Rust 为 1.97.1；NDK 为 27.0.12077973；Android API 为 26；Windows 使用 MSVC 静态 CRT。
`-CosignPath` 接受 Cosign 可执行文件或本机受控包装脚本；容器包装必须正确映射文件路径。

```powershell
cargo +1.97.1 fmt --all -- --check
cargo +1.97.1 clippy --workspace --all-targets --all-features --locked --offline -- -D warnings
cargo +1.97.1 test --workspace --all-targets --all-features --locked --offline
.\scripts\Test-CoreRelease.ps1
.\scripts\Test-JniInputRegistration.ps1
```

先提交并核对源码，确保工作区干净。新目录不覆盖旧候选；源码在构建中发生变化会拒绝生成清单。

```powershell
$version = '0.1.0-rc.1'
.\scripts\Build-NativeArtifacts.ps1 -OutputDirectory "dist/build-$version"
.\scripts\New-CoreReleasePackage.ps1 -Version $version `
  -NativeDirectory "dist/build-$version" -OutputDirectory "dist/release-$version"
.\scripts\Sign-CoreRelease.ps1 -ReleaseDirectory "dist/release-$version" `
  -CosignPath $cosign -PrivateKeyPath $privateKey -PublicKeyPath $publicKey `
  -ExpectedPublicKeySha256 $trustedKeyHash
```

解压对应源码到新目录，以 vendor 配置执行离线测试与重建；执行真实 Windows 绑定及 Android JNI
校验。`cargo check`、单元测试、模拟器与真实设备是不同证据；Release 说明准确记录实测范围。
本流程不自动签名或安装 Windows DLL，不更新 APK/Setup、不修改运行中的应用或服务器。

## 上传与发布

Git SSH 凭据不能代替 Release API token。脚本支持已授权 Git Credential Manager 中的 GitHub OAuth
凭据，也支持 `GH_TOKEN` / `GITHUB_TOKEN` 或受保护文件 `-TokenFile`；需要该仓库写入权限。
不要把令牌发到聊天或写进脚本。可通过 `git credential-manager github login --browser` 完成本机授权。
发布说明用 UTF-8 文件 `-NotesPath`，提供中英文摘要、兼容范围、源码和签名验证方式。

```powershell
.\scripts\Publish-CoreRelease.ps1 -Action Inspect -Version $version
.\scripts\Publish-CoreRelease.ps1 -Action Upload -Version $version `
  -ReleaseDirectory "dist/release-$version" -NotesPath $notes `
  -CosignPath $cosign -ExpectedPublicKeySha256 $trustedKeyHash
# Upload 创建/续传 draft，并下载全部附件重新验证签名、ZIP、源码和五库清单。
.\scripts\Publish-CoreRelease.ps1 -Action Publish -Version $version `
  -ReleaseDirectory "dist/release-$version" `
  -CosignPath $cosign -ExpectedPublicKeySha256 $trustedKeyHash
```

Publish 会再次下载回验后才将 draft 置为 published；`rc` 自动为 prerelease，不改 latest。
如果同名已上传附件哈希不同，停止并保留 draft，不删除/替换附件；已发布 Release 不允许修改。
脚本不替换 tag，不上传 staging、构建日志、私钥或应用源码。

仓库仍为私有时，Release 仅对有读取权限的接收者可用。必须核实目标接收者能下载完整对应源码；
本次发布不意味着获准公开仓库。GitHub 自动源码 ZIP 不含 vendored 依赖，不能代替本流程源码包。

## 应用固定版本导入

```powershell
.\scripts\Publish-CoreRelease.ps1 -Action Download -Version $version `
  -DownloadDirectory $download -CosignPath $cosign -ExpectedPublicKeySha256 $trustedKeyHash
.\scripts\Import-CoreRelease.ps1 -ReleaseDirectory $download -Version $version `
  -ExpectedRevision $revision -TermLinkDirectory ..\termlink `
  -CosignPath $cosign -ExpectedPublicKeySha256 $trustedKeyHash
```

导入要求显式版本、源码 revision 和独立可信的公钥指纹。先验证签名与全部附件，再复制五库，
锁文件最后写；复制失败使用备份恢复，回滚目录在系统临时目录，输出路径可追溯。
导入只写五库和锁文件；C#/Kotlin 适配器仍由应用维护，不能认为任意新 core 自动兼容旧适配器。
验证应用绑定/JNI及相关回归后一起提交依赖，应用正式发布仍运行
`scripts/Verify-NativeArtifacts.ps1 -RequireClean`。不使用 `latest` 浮动版本、不在普通应用构建时联网下载。

## English summary

Build all five native libraries from one clean, committed revision. Package matching source with locked,
vendored dependencies and build materials. Sign `SHA256SUMS` with Cosign and verify against an independently
pinned public-key SHA-256. Upload six allowlisted assets to a GitHub draft, download and verify them,
then publish. Release versions/tags/assets are never overwritten; RC releases are prereleases.

Import requires the exact version, commit, and trusted signing key. Verify before writing; copy the lock
last and retain rollback data. Application adapters, client releases, repository visibility and server
deployment are outside this core dependency release. A private release does not make corresponding source
available to recipients without repository access. Record test scope honestly.
