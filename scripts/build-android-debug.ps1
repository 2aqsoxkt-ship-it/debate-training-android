[CmdletBinding()]
param([ValidateSet('aarch64','x86_64')][string]$Target = 'aarch64', [switch]$ListInputs)
$ErrorActionPreference = 'Stop'
$taskProject = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../android'))
$taskGradle = Join-Path $taskProject 'src-tauri/gen/android'
function Get-GradleInputs {
    $taskFiles = & rg --files --hidden --no-ignore --glob '!**/.gradle/**' --glob '!**/build/**' $taskGradle
    if ($LASTEXITCODE -ne 0) { throw 'Cannot enumerate Android Gradle inputs.' }
    foreach ($taskFile in $taskFiles) {
        if ($taskFile.Replace('\','/') -match '(\.(gradle|gradle\.kts|kt|xml|png|pro|properties)$|gradlew\.bat$|gradle-wrapper\.jar$)') { $taskFile }
    }
}
if ($ListInputs) { Get-GradleInputs; exit 0 }
. (Join-Path $PSScriptRoot 'android-env.ps1')
$taskRustTarget = if ($Target -eq 'aarch64') { 'aarch64-linux-android' } else { 'x86_64-linux-android' }
$taskAbi = if ($Target -eq 'aarch64') { 'arm64-v8a' } else { 'x86_64' }
$taskFlavor = if ($Target -eq 'aarch64') { 'arm64' } else { 'x86_64' }
$taskCapitalized = if ($Target -eq 'aarch64') { 'Arm64' } else { 'X86_64' }
Push-Location -LiteralPath $taskProject
try {
    & npm.cmd run tauri -- android build --debug --apk --target $Target --ci 2>&1 | Tee-Object -Variable taskBuildOutput
    if ($LASTEXITCODE -eq 0) { exit 0 }
    $taskBuildLog = $taskBuildOutput -join "`n"
    if ($taskBuildLog -notmatch 'Creation symbolic link is not allowed' -or
        $taskBuildLog -notmatch 'Finished.+dev.+profile' -or
        $taskBuildLog -notmatch 'symlinking lib') {
        throw 'Android build failed before the verified Windows symlink fallback. Do not package an old library.'
    }
    # Tauri compiled the exact source successfully but this Windows account
    # cannot create file symlinks. Copy only that generated library; neither
    # enable developer mode globally nor bypass any Rust compilation failure.
    $taskLibrary = Join-Path $env:CARGO_TARGET_DIR "$taskRustTarget/debug/libdebate_training_agent_lib.so"
    if (-not (Test-Path -LiteralPath $taskLibrary)) { throw 'Compiled Android library is missing.' }
    $taskDestination = [IO.Path]::GetFullPath((Join-Path $taskGradle "app/src/main/jniLibs/$taskAbi/libdebate_training_agent_lib.so"))
    if (-not $taskDestination.StartsWith($taskProject + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) { throw 'Library destination escaped the Android project.' }
    if ((Test-Path -LiteralPath $taskDestination) -and ((Get-Item -LiteralPath $taskDestination).Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'Refuse to copy through a symlink.' }
    Copy-Item -LiteralPath $taskLibrary -Destination $taskDestination -Force
    if ((Get-FileHash -LiteralPath $taskLibrary).Hash -ne (Get-FileHash -LiteralPath $taskDestination).Hash) { throw 'Android library copy verification failed.' }
    Write-Output 'Compiled Android library copied without changing Windows developer settings.'
    # AGP refuses a non-ASCII Windows project path. Stage only generated Gradle
    # inputs and platform Kotlin, never move the user's source or disable AGP's
    # check. A fresh owned directory also prevents removed source lingering.
    $taskWork = Join-Path $env:CARGO_TARGET_DIR ('android-work-' + $Target + '-' + [Guid]::NewGuid().ToString('N').Substring(0,8))
    New-Item -ItemType Directory -Path $taskWork | Out-Null
    $taskInputs = Get-GradleInputs
    foreach ($taskInput in $taskInputs) {
        $taskRelative = [IO.Path]::GetRelativePath($taskGradle, $taskInput)
        $taskNormalized = $taskRelative.Replace('\','/')
        if ($taskNormalized -notmatch '(\.(gradle|gradle\.kts|kt|xml|png|pro|properties)$|gradlew\.bat$|gradle-wrapper\.jar$)') { continue }
        $taskCopyTarget = [IO.Path]::GetFullPath((Join-Path $taskWork $taskRelative))
        if (-not $taskCopyTarget.StartsWith($taskWork + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) { throw 'Gradle input escaped the dedicated staging directory.' }
        New-Item -ItemType Directory -Path ([IO.Path]::GetDirectoryName($taskCopyTarget)) -Force | Out-Null
        Copy-Item -LiteralPath $taskInput -Destination $taskCopyTarget
    }
    $taskNative = Join-Path $taskWork 'platform'
    New-Item -ItemType Directory -Path $taskNative | Out-Null
    Copy-Item -LiteralPath (Join-Path $taskProject 'plugins/platform/android/src/main/java') -Destination $taskNative -Recurse
    $taskStagedLibrary = Join-Path $taskWork "app/src/main/jniLibs/$taskAbi/libdebate_training_agent_lib.so"
    New-Item -ItemType Directory -Path ([IO.Path]::GetDirectoryName($taskStagedLibrary)) -Force | Out-Null
    Copy-Item -LiteralPath $taskLibrary -Destination $taskStagedLibrary
    if ((Get-FileHash -LiteralPath $taskLibrary).Hash -ne (Get-FileHash -LiteralPath $taskStagedLibrary).Hash) { throw 'Staged library hash differs from the compiled source.' }
    # Keep the original full debug library for diagnostics; remove only DWARF
    # from this packaging copy so a debug APK is not hundreds of megabytes.
    & (Join-Path $env:NDK_HOME 'toolchains/llvm/prebuilt/windows-x86_64/bin/llvm-strip.exe') --strip-debug $taskStagedLibrary
    if ($LASTEXITCODE -ne 0) { throw 'Debug symbol stripping failed.' }
    Write-Output ('Generated Gradle inputs staged at: ' + $taskWork)
    Push-Location -LiteralPath $taskWork
    try {
        & .\gradlew.bat --console plain --no-daemon "-Pdebate.platformSourceDir=$taskNative/java" "-PtargetList=$Target" "-ParchList=$taskFlavor" "-PabiList=$taskAbi" "assemble${taskCapitalized}Debug" -x "rustBuild${taskCapitalized}Debug"
        if ($LASTEXITCODE -ne 0) { throw 'Android Gradle packaging failed.' }
        $taskApk = Join-Path $taskWork "app/build/outputs/apk/$taskFlavor/debug/app-$taskFlavor-debug.apk"
        if (-not (Test-Path -LiteralPath $taskApk)) { throw 'Gradle reported success without the expected APK.' }
        $taskRelease = Join-Path $taskProject "../releases/android/$taskFlavor"
        New-Item -ItemType Directory -Path $taskRelease -Force | Out-Null
        Copy-Item -LiteralPath $taskApk -Destination (Join-Path $taskRelease "debate-training-$taskFlavor-debug.apk") -Force
        Write-Output ('Debug APK: ' + [IO.Path]::GetFullPath((Join-Path $taskRelease "debate-training-$taskFlavor-debug.apk")))
    } finally { Pop-Location }
} finally { Pop-Location }
