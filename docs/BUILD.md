# 构建与签名

已验证的发行构建环境为 Windows + PowerShell 7。源码不依赖维护者的用户名或 SDK 路径。其他系统可使用 Tauri 官方 Android 构建流程，但本仓库的签名初始化脚本使用 Windows DPAPI，不能在其他系统直接运行。

## 工具链

安装 Node.js 22+、Rust stable、PowerShell 7、JDK 21、Android SDK 命令行工具与 `rg`。在你自己的 SDK 路径执行 `sdkmanager`，阅读并接受官方许可证后安装：

```powershell
sdkmanager "platform-tools" "platforms;android-36" "build-tools;36.0.0" "ndk;28.2.13676358"
rustup target add aarch64-linux-android
```

当前进程设置 `JAVA_HOME` 和 `ANDROID_HOME`。不要将全局环境改成维护者的路径。`scripts/android-env.ps1` 会使用这些变量；缺省时才查找本机 `LocalAppData/DebateAndroidTools`。脚本不会下载 SDK 或接受新的许可证。

Windows NDK 与某些 EFS 加密目录不能可靠处理编译中间文件。通过 `DEBATE_ANDROID_BUILD_CACHE` 指定一个英文、非 EFS 的独立缓存目录。默认是桌面 `DebateAndroidBuildCache`；只包含生成文件，不移动源码，不关闭系统加密。构建需联网下载依赖，首次可能较慢并占用数 GB 空间。

## 检查

```powershell
npm --prefix android ci
npm --prefix android run typecheck
npm --prefix android run lint
npm --prefix android run format:check
npm --prefix android test
npm --prefix android run build
node --test tests/release-config.test.mjs
Set-Location android/src-tauri
cargo fmt --all --check
cargo test --locked
```

普通测试使用合成输入与模拟模型。不要使用 `cargo test -- --ignored`：被忽略的验收测试可能产生真实模型费用。不要在公共 CI 配置个人 API 密钥。

浏览器 UI 测试默认使用已安装的 Microsoft Edge：`npm --prefix android run test:ui`。其他环境需自行配置 Playwright 浏览器。

CI 模板是 `docs/ci-checks.example.yml`。首次发布使用的 GitHub 授权不含 `workflow` 权限，故未启用 Actions，也不声称线上 CI 已通过。维护者可在获得相应授权后，将模板复制为 `.github/workflows/checks.yml`。

## 编译 ARM64 release APK

在仓库根目录执行：

```powershell
pwsh -NoProfile -File scripts/build-android-release.ps1 -Target aarch64 -InitializeSigning
```

首次创建独立 RSA 4096 发行密钥，保存于当前用户的 `.debate-android-signing` 目录，密码使用 Windows DPAPI 加密。不写入源码、不输出密码、不上传 GitHub。再次构建去掉 `-InitializeSigning`，沿用同一签名。

需要其他私有目录时传 `-SigningDirectory`。必须安全备份密钥，并单独建立可恢复的密码备份；DPAPI 文件不能保证在换机器或重装系统后解密。丢失签名密钥将无法向已安装用户提供同签名更新。

产物：`releases/android/arm64/debate-training-arm64-release.apk`。脚本校验签名、16 KB zipalign、应用标识和不可调试属性后才复制至发行目录。模拟器构建可改 `-Target x86_64`，并提前安装 `x86_64-linux-android` Rust target。

Tauri 会在构建时生成 `tauri.settings.gradle`、`tauri.build.gradle.kts` 等与本机路径相关的输入，这些文件故意不纳入公共 Git。不要手工把维护者的生成路径复制到自己的系统。

如 Windows 无符号链接权限，脚本仅在确认本轮 Rust release 编译成功、且错误明确是 Tauri 建立符号链接失败时，启用复制后哈希核对的替代打包。其他编译失败直接停止，绝不使用旧 `.so`。

发布使用独立应用标识 `com.debate.training.android.opensource`。自行签名不能覆盖维护者签名包，也不能覆盖原 debug 测试版；保留旧版即可保留旧数据。当前没有自动跨版本数据迁移工具。

官方参考：[Android 签名](https://developer.android.com/studio/publish/app-signing)、[Tauri Android 签名](https://v2.tauri.app/distribute/sign/android/)、[R8 优化](https://developer.android.com/topic/performance/app-optimization/enable-app-optimization)。
