[CmdletBinding()]
param()
$taskTools = Join-Path $env:LOCALAPPDATA 'DebateAndroidTools'
if (-not $env:JAVA_HOME) {
    $taskJdks = @(Get-ChildItem -LiteralPath (Join-Path $taskTools 'jdk') -Directory -ErrorAction SilentlyContinue | Sort-Object Name)
    if ($taskJdks.Count -ne 1) { throw 'Set JAVA_HOME to JDK 21, or install one JDK under LocalAppData/DebateAndroidTools/jdk.' }
    $env:JAVA_HOME = $taskJdks[0].FullName
}
if (-not $env:ANDROID_HOME) {
    $env:ANDROID_HOME = if ($env:ANDROID_SDK_ROOT) { $env:ANDROID_SDK_ROOT } else { Join-Path $taskTools 'sdk' }
}
if (-not (Test-Path -LiteralPath (Join-Path $env:JAVA_HOME 'bin/java.exe'))) { throw 'JAVA_HOME does not contain a Windows JDK.' }
if (-not (Test-Path -LiteralPath (Join-Path $env:ANDROID_HOME 'platform-tools/adb.exe'))) { throw 'ANDROID_HOME does not contain Android platform-tools.' }
$env:ANDROID_SDK_ROOT = $env:ANDROID_HOME
$env:NDK_HOME = Join-Path $env:ANDROID_HOME 'ndk/28.2.13676358'
# The NDK command launcher loses non-ASCII object paths on Windows. rustc also
# cannot atomically persist .rmeta from TEMP into an EFS-encrypted directory.
# Keep this generated cache separate from the source and from encrypted AppData.
$taskBuildCache = if ($env:DEBATE_ANDROID_BUILD_CACHE) { $env:DEBATE_ANDROID_BUILD_CACHE } else {
    Join-Path ([Environment]::GetFolderPath('Desktop')) 'DebateAndroidBuildCache'
}
$taskBuildCache = [IO.Path]::GetFullPath($taskBuildCache)
if ($taskBuildCache -match '[^\x20-\x7e]') { throw 'Android build cache must have an ASCII-only path. Set DEBATE_ANDROID_BUILD_CACHE to a dedicated unencrypted directory.' }
New-Item -ItemType Directory -Path $taskBuildCache -Force | Out-Null
if ((Get-Item -LiteralPath $taskBuildCache).Attributes -band [IO.FileAttributes]::Encrypted) {
    throw 'Android build cache must not be EFS-encrypted. Choose a different dedicated cache; do not disable system encryption.'
}
$env:CARGO_TARGET_DIR = $taskBuildCache
$env:GRADLE_USER_HOME = Join-Path $taskBuildCache 'gradle'
$env:PATH = "$env:JAVA_HOME/bin;$env:ANDROID_HOME/platform-tools;$env:ANDROID_HOME/cmdline-tools/latest/bin;$env:PATH"
Write-Output 'Android environment configured for this process only.'
