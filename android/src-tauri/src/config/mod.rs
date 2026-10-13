pub fn app_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests {
    use super::app_version;

    #[test]
    fn version_matches_package_manifest() {
        let config: serde_json::Value =
            serde_json::from_str(include_str!("../../tauri.conf.json")).unwrap();
        assert_eq!(app_version(), config["version"].as_str().unwrap());
    }
}
