import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import test from "node:test";

const root = new URL("../", import.meta.url);
const read = (path) => readFileSync(new URL(path, root), "utf8");

test("open-source release is isolated, not debuggable, and optimized", () => {
  const gradle = read("android/src-tauri/gen/android/app/build.gradle.kts");
  const release = gradle.split('getByName("release") {')[1];
  assert.match(release, /applicationIdSuffix = "\.opensource"/);
  assert.match(release, /isDebuggable = false/);
  assert.match(release, /isJniDebuggable = false/);
  assert.match(release, /isMinifyEnabled = true/);
  assert.match(release, /isShrinkResources = true/);
  assert.match(release, /signingConfig = signingConfigs\.getByName\("openSourceRelease"\)/);
  for (const variable of ["DEBATE_ANDROID_KEYSTORE", "DEBATE_ANDROID_KEY_ALIAS", "DEBATE_ANDROID_STORE_PASSWORD", "DEBATE_ANDROID_KEY_PASSWORD"]) {
    assert.ok(gradle.includes(variable));
  }
  assert.doesNotMatch(release, /signingConfigs\.getByName\("debug"\)/);
});

test("release builder cannot silently publish a debug APK or an old library", () => {
  const path = new URL("scripts/build-android-release.ps1", root);
  assert.ok(existsSync(path), fileURLToPath(path));
  const script = readFileSync(path, "utf8");
  assert.match(script, /android build --apk --target/);
  assert.doesNotMatch(script, /android build --debug/);
  assert.match(script, /Finished.+release/);
  assert.match(script, /Get-FileHash/);
  assert.match(script, /apksigner/);
  assert.match(script, /zipalign/);
  assert.match(script, /application-debuggable/);
  assert.match(script, /Import-Clixml/);
  assert.match(script, /finally/);
});

test("private signing material is excluded even if a developer puts it in the project", () => {
  const ignored = read(".gitignore");
  for (const pattern of ["*.p12", "*.jks", "*.keystore", "**/keystore.properties", "**/signing*.xml"]) {
    assert.ok(ignored.includes(pattern), pattern);
  }
});
