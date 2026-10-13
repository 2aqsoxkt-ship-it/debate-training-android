# 辩论训练智能体 · Android 开源版

中文辩论训练应用，支持手机与平板。正反双方各四个辩位，可让 AI 模拟，也可由人接管一个或多个辩位。正式开始前须确认赛制、赛程和席位安排。

**当前为 0.1.2 早期预发布，不是已完成全部验收的正式产品。** 此仓库仅包含安卓工程，不包含桌面版、个人比赛数据库、开发历史、API 密钥或发行签名私钥。

## 下载与使用

在 [GitHub Releases](https://github.com/2aqsoxkt-ship-it/debate-training-android/releases) 下载 `debate-training-arm64-release.apk`，适用于 Android 8.0 及以上的 ARM64 手机和平板。需允许从该下载来源安装应用。尚未验收安卓兼容层上的鸿蒙设备，不支持 HarmonyOS NEXT 原生安装。

开源版应用标识是 `com.debate.training.android.opensource`，可与旧 debug 测试版并存。**不要卸载旧版来安装本版本**；两者数据独立，不自动迁移原比赛记录或密钥。以后同一发行签名的开源版可覆盖更新；自行编译并使用其他密钥签名的包不能覆盖官方包。

首次打开后需手动点击开始。在模型设置页填入你自己的 DeepSeek API 密钥并测试连接，再创建本机账号、填写辩题、选赛制、核对赛程并开始训练。使用 AI 功能需联网，费用由你自己的 API 账户承担；纯人工输入不需要购买内置服务。没有内置可供所有人使用的 API 密钥。

## 已有功能

- 陈词、团队质询、自由辩、质询小结和总结；可手工调整识别后的赛程。
- 字数额度与双方共享字池；自由辩团队轮换、多辩位人工/AI 控制。
- 质询回答逐字展示，可调展示速度；打断仅保留正式公开前缀。
- 中文编辑事件回放、输入统计、训练报告和基于模型的评价。
- 手机单栏、平板双栏；用户界面显示“正方一辩”等中文名称。
- SQLite 本地存储；Android Keystore + AES-GCM 保存用户 API 密钥。

## 尚未完成的验收

完整后台任务、系统杀进程后恢复、安卓原生分享和系统返回键流程仍需完善；尚未覆盖所有国产输入法、所有 Android 8.0 设备、真实平板与 16 KB 内存页设备。本次发布未做真实付费 AI 全场比赛，不承诺模型可用性、费用上限或评分的客观性。模型名称与思考设置以应用实际配置及供应商支持情况为准。

评分是训练反馈，不是正式赛事裁判结果。输入来源不能确认时会降低输入控制评价置信度；AI 内容仍可能存在事实错误。

## 源码与开发

```text
android/       React / TypeScript 前端、Tauri / Rust 后端、Android Kotlin 桥接
scripts/       Windows 构建与独立签名脚本
tests/         发布配置回归测试
docs/          构建、安全、隐私与发布说明
third-party/   第三方依赖许可与来源记录
```

需 Node.js 22+、Rust stable、JDK 21、Android SDK 36 / Build Tools 36.0.0 / NDK 28.2.13676358。锁文件固定依赖；详细步骤见[构建说明](docs/BUILD.md)。

```powershell
npm --prefix android ci
npm --prefix android run typecheck
npm --prefix android run lint
npm --prefix android test
node --test tests/release-config.test.mjs
```

浏览器开发模式没有设备原生后端，不等于可用的手机版。安卓发行包按 release 模式编译、关闭调试、启用 R8 和资源裁剪；源码公开，不添加防反编译壳，不承诺无法逆向。

项目原创代码使用 [MIT 许可证](LICENSE)，第三方依赖保留各自许可。请阅读[隐私说明](docs/PRIVACY.md)、[安全说明](SECURITY.md)和[第三方说明](THIRD_PARTY_NOTICES.md)。
