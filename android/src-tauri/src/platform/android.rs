use jni::objects::{GlobalRef, JClass, JObject, JString, JValue};
use jni::JavaVM;
use std::sync::OnceLock;

struct AndroidRuntime {
    vm: JavaVM,
    application: GlobalRef,
    class: GlobalRef,
}

fn runtime() -> Result<&'static AndroidRuntime, String> {
    static RUNTIME: OnceLock<Result<AndroidRuntime, String>> = OnceLock::new();
    RUNTIME
        .get_or_init(|| {
            // Tauri 2.11/Tao 0.35 owns this context; it does not initialize the
            // old ndk-context singleton. Absence is an error, never a panic.
            let context = tao::platform::android::prelude::main_android_context()
                .ok_or("安卓运行环境尚未就绪")?;
            let vm = unsafe { JavaVM::from_raw(context.java_vm.cast()) }
                .map_err(|_| "安卓运行环境不可用")?;
            let mut env = vm.attach_current_thread().map_err(|_| "安卓线程接入失败")?;
            let result = (|| -> jni::errors::Result<(GlobalRef, GlobalRef)> {
                let activity = unsafe { JObject::from_raw(context.context_jobject.cast()) };
                let application = env
                    .call_method(
                        &activity,
                        "getApplicationContext",
                        "()Landroid/content/Context;",
                        &[],
                    )?
                    .l()?;
                let loader = env
                    .call_method(
                        &application,
                        "getClassLoader",
                        "()Ljava/lang/ClassLoader;",
                        &[],
                    )?
                    .l()?;
                let name = env.new_string("com.debate.training.android.platform.PlatformBridge")?;
                let class = env
                    .call_method(
                        loader,
                        "loadClass",
                        "(Ljava/lang/String;)Ljava/lang/Class;",
                        &[JValue::Object(&name)],
                    )?
                    .l()?;
                Ok((env.new_global_ref(application)?, env.new_global_ref(class)?))
            })();
            if result.is_err() {
                let _ = env.exception_clear();
            }
            drop(env);
            let (application, class) = result.map_err(|_| "安卓系统桥接初始化失败")?;
            Ok(AndroidRuntime {
                vm,
                application,
                class,
            })
        })
        .as_ref()
        .map_err(Clone::clone)
}

pub fn call(method: &str, argument: &str) -> Result<String, String> {
    // Android/Tauri owns this VM and the global Activity reference. We only
    // borrow them; never delete either reference or create another runtime.
    let runtime = runtime()?;
    let mut env = runtime
        .vm
        .attach_current_thread()
        .map_err(|_| "安卓线程接入失败")?;
    let result = env.with_local_frame(16, |env| -> jni::errors::Result<String> {
        let class: &JClass = runtime.class.as_obj().into();
        let arg = env.new_string(argument)?;
        let value = env
            .call_static_method(
                class,
                method,
                "(Landroid/content/Context;Ljava/lang/String;)Ljava/lang/String;",
                &[
                    JValue::Object(runtime.application.as_obj()),
                    JValue::Object(&arg),
                ],
            )?
            .l()?;
        let text = env.get_string(&JString::from(value))?.into();
        Ok(text)
    });
    if result.is_err() {
        // Never describe the Java exception: credential arguments may appear
        // in diagnostics. Return only a fixed, redacted platform error.
        let _ = env.exception_clear();
    }
    result.map_err(|_| "安卓系统操作失败，请检查权限或重新配置密钥".to_owned())
}
