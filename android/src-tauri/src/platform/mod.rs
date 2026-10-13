#[cfg(target_os = "android")]
mod android;
#[cfg(target_os = "android")]
pub use android::call;

#[cfg(not(target_os = "android"))]
pub fn call(_method: &str, _argument: &str) -> Result<String, String> {
    Err("安卓系统能力仅在安卓运行时可用".to_owned())
}
