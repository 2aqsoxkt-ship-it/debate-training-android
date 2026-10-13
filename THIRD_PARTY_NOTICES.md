# 第三方组件

MIT 许可证仅适用于本项目原创代码；第三方组件保留各自著作权及许可证。源码不重新分发 Android SDK、NDK、JDK 或模型权重。DeepSeek 是外部服务，不是本项目内置模型文件。

主要运行时组件包括 React / React DOM / React Router / Zod（MIT）、Tauri（MIT OR Apache-2.0）、Rust 生态的 Tokio / Serde / reqwest / rusqlite 等，以及 AndroidX、Material Components 和 Kotlin。确切依赖版本由 `android/package-lock.json`、`android/src-tauri/Cargo.lock` 和 Gradle 配置记录。

发布时生成的 `third-party/` 目录保留依赖的许可与来源信息。该记录涵盖构建解析到的依赖，不表示每个开发依赖或其他平台组件都进入了 Android 安装包。依赖自己的 NOTICE、LICENSE 及双许可选择不被本项目 MIT 许可证替换。

`third-party/sources/` 还提供未经修改的 MPL 依赖及部分未随 crate 附带独立许可文件的上游源代码压缩包。依赖清单提供精确版本和上游来源，许可文件保留原文，不为通过格式检查而改写。

界面插图来自项目开发阶段生成的素材；无需联网下载第三方图片。不要把本项目许可理解为对图片独占权、商标或第三方服务条款的保证。
