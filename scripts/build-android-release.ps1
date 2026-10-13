[CmdletBinding()]
param(
    [ValidateSet('aarch64','x86_64')][string]$Target = 'aarch64',
    [switch]$InitializeSigning,
    [string]$SigningDirectory = (Join-Path $env:USERPROFILE '.debate-android-signing')
)
$ErrorActionPreference = 'Stop'
$taskProject = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../android'))
$taskGradle = Join-Path $taskProject 'src-tauri/gen/android'
. (Join-Path $PSScriptRoot 'android-env.ps1')
$taskSigning = [IO.Path]::GetFullPath($SigningDirectory)
if ($taskSigning.StartsWith($taskProject + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
    throw 'Keep signing keys outside the source project.'
}
$taskStore = Join-Path $taskSigning 'open-source-release.p12'
$taskPasswordFile = Join-Path $taskSigning 'signing-password.xml'
$taskAlias = 'debate-open-source-release'
$taskVariableNames = @('DEBATE_ANDROID_KEYSTORE','DEBATE_ANDROID_KEY_ALIAS','DEBATE_ANDROID_STORE_PASSWORD','DEBATE_ANDROID_KEY_PASSWORD')
$taskPrevious = @{}
foreach ($taskName in $taskVariableNames) { $taskPrevious[$taskName] = [Environment]::GetEnvironmentVariable($taskName, 'Process') }
try {
    if ($InitializeSigning -and -not (Test-Path -LiteralPath $taskStore)) {
        if (Test-Path -LiteralPath $taskPasswordFile) { throw 'Signing password exists without a keystore; inspect manually instead of overwriting it.' }
        New-Item -ItemType Directory -Path $taskSigning -Force | Out-Null
        $taskPassword = [Convert]::ToBase64String([Security.Cryptography.RandomNumberGenerator]::GetBytes(32))
        $env:DEBATE_ANDROID_STORE_PASSWORD = $taskPassword
        $env:DEBATE_ANDROID_KEY_PASSWORD = $taskPassword
        & (Join-Path $env:JAVA_HOME 'bin/keytool.exe') -genkeypair -noprompt -storetype PKCS12 -keystore $taskStore -alias $taskAlias -keyalg RSA -keysize 4096 -validity 10000 -dname 'CN=Debate Training Android Open Source, O=2aqsoxkt-ship-it' -storepass:env DEBATE_ANDROID_STORE_PASSWORD -keypass:env DEBATE_ANDROID_KEY_PASSWORD
        if ($LASTEXITCODE -ne 0) { throw 'Release key generation failed.' }
        # Windows DPAPI binds this encrypted password to the current user/machine.
        ConvertTo-SecureString $taskPassword -AsPlainText -Force | Export-Clixml -LiteralPath $taskPasswordFile
        $taskPassword = $null
        Write-Output 'Created a private release key outside the repository. Back up this directory securely.'
    }
    if (-not (Test-Path -LiteralPath $taskStore) -or -not (Test-Path -LiteralPath $taskPasswordFile)) {
        throw 'Missing private release signing material. Run once with -InitializeSigning; never use the debug key.'
    }
    $taskSecure = Import-Clixml -LiteralPath $taskPasswordFile
    if ($taskSecure -isnot [Security.SecureString]) { throw 'Invalid encrypted signing password.' }
    $taskCredential = [Management.Automation.PSCredential]::new('release', $taskSecure)
    $env:DEBATE_ANDROID_KEYSTORE = $taskStore
    $env:DEBATE_ANDROID_KEY_ALIAS = $taskAlias
    $env:DEBATE_ANDROID_STORE_PASSWORD = $taskCredential.GetNetworkCredential().Password
    $env:DEBATE_ANDROID_KEY_PASSWORD = $env:DEBATE_ANDROID_STORE_PASSWORD
    $taskRustTarget = if ($Target -eq 'aarch64') { 'aarch64-linux-android' } else { 'x86_64-linux-android' }
    $taskAbi = if ($Target -eq 'aarch64') { 'arm64-v8a' } else { 'x86_64' }
    $taskFlavor = if ($Target -eq 'aarch64') { 'arm64' } else { 'x86_64' }
    $taskCapitalized = if ($Target -eq 'aarch64') { 'Arm64' } else { 'X86_64' }
    Push-Location -LiteralPath $taskProject
    try {
        & npm.cmd run tauri -- android build --apk --target $Target --ci --config src-tauri/tauri.release.conf.json 2>&1 | Tee-Object -Variable taskBuildOutput
        $taskBuildExit = $LASTEXITCODE
    } finally { Pop-Location }
    if ($taskBuildExit -ne 0) {
        $taskBuildLog = $taskBuildOutput -join "`n"
        if ($taskBuildLog -notmatch 'Creation symbolic link is not allowed' -or $taskBuildLog -notmatch 'Finished.+release.+profile' -or $taskBuildLog -notmatch 'symlinking lib') {
            throw 'Release compilation failed before the verified Windows symlink fallback. Refuse to publish an old library.'
        }
    }
    $taskLibrary = Join-Path $env:CARGO_TARGET_DIR "$taskRustTarget/release/libdebate_training_agent_lib.so"
    if (-not (Test-Path -LiteralPath $taskLibrary)) { throw 'Current release library is missing.' }
    $taskWork = Join-Path $env:CARGO_TARGET_DIR ('android-release-' + $Target + '-' + [Guid]::NewGuid().ToString('N').Substring(0,8))
    New-Item -ItemType Directory -Path $taskWork | Out-Null
    $taskInputs = & rg --files --hidden --no-ignore --glob '!**/.gradle/**' --glob '!**/build/**' $taskGradle
    if ($LASTEXITCODE -ne 0) { throw 'Cannot enumerate generated Gradle inputs.' }
    foreach ($taskInput in $taskInputs) {
        $taskRelative = [IO.Path]::GetRelativePath($taskGradle, $taskInput)
        $taskNormalized = $taskRelative.Replace('\','/')
        if ($taskNormalized -notmatch '(\.(gradle|gradle\.kts|kt|xml|png|pro|properties)$|gradlew\.bat$|gradle-wrapper\.jar$)') { continue }
        if ($taskNormalized -match '(keystore\.properties|signing[^/]*\.xml)$') { continue }
        if ((Get-Item -LiteralPath $taskInput).Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Refuse linked build inputs.' }
        $taskCopy = [IO.Path]::GetFullPath((Join-Path $taskWork $taskRelative))
        if (-not $taskCopy.StartsWith($taskWork + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) { throw 'Build input escaped staging.' }
        New-Item -ItemType Directory -Path ([IO.Path]::GetDirectoryName($taskCopy)) -Force | Out-Null
        Copy-Item -LiteralPath $taskInput -Destination $taskCopy
    }
    $taskNative = Join-Path $taskWork 'platform'
    New-Item -ItemType Directory -Path $taskNative | Out-Null
    Copy-Item -LiteralPath (Join-Path $taskProject 'plugins/platform/android/src/main/java') -Destination $taskNative -Recurse
    $taskStagedLibrary = Join-Path $taskWork "app/src/main/jniLibs/$taskAbi/libdebate_training_agent_lib.so"
    New-Item -ItemType Directory -Path ([IO.Path]::GetDirectoryName($taskStagedLibrary)) -Force | Out-Null
    Copy-Item -LiteralPath $taskLibrary -Destination $taskStagedLibrary
    if ((Get-FileHash -LiteralPath $taskLibrary).Hash -ne (Get-FileHash -LiteralPath $taskStagedLibrary).Hash) { throw 'Staged native library hash mismatch.' }
    Push-Location -LiteralPath $taskWork
    try {
        & .\gradlew.bat --console plain --no-daemon "-Pdebate.platformSourceDir=$taskNative/java" "-PtargetList=$Target" "-ParchList=$taskFlavor" "-PabiList=$taskAbi" "assemble${taskCapitalized}Release" -x "rustBuild${taskCapitalized}Release"
        if ($LASTEXITCODE -ne 0) { throw 'Release Gradle packaging failed.' }
        $taskApk = Join-Path $taskWork "app/build/outputs/apk/$taskFlavor/release/app-$taskFlavor-release.apk"
        if (-not (Test-Path -LiteralPath $taskApk)) { throw 'Expected signed release APK is missing.' }
        $taskBuildTools = Join-Path $env:ANDROID_HOME 'build-tools/36.0.0'
        & (Join-Path $taskBuildTools 'apksigner.bat') verify --verbose --print-certs $taskApk
        if ($LASTEXITCODE -ne 0) { throw 'APK signature verification failed.' }
        & (Join-Path $taskBuildTools 'zipalign.exe') -c -P 16 4 $taskApk
        if ($LASTEXITCODE -ne 0) { throw 'APK alignment check failed.' }
        $taskBadging = & (Join-Path $taskBuildTools 'aapt.exe') dump badging $taskApk
        if ($LASTEXITCODE -ne 0 -or ($taskBadging -join "`n") -match 'application-debuggable' -or ($taskBadging -join "`n") -notmatch "package: name='com.debate.training.android.opensource'") {
            throw 'APK is debuggable or has the wrong release identity.'
        }
        $taskOutput = Join-Path $taskProject "../releases/android/$taskFlavor"
        New-Item -ItemType Directory -Path $taskOutput -Force | Out-Null
        $taskPublished = Join-Path $taskOutput "debate-training-$taskFlavor-release.apk"
        Copy-Item -LiteralPath $taskApk -Destination $taskPublished -Force
        Write-Output ('Verified release APK: ' + $taskPublished)
        Get-FileHash -LiteralPath $taskPublished -Algorithm SHA256
    } finally { Pop-Location }
} finally {
    foreach ($taskName in $taskVariableNames) { [Environment]::SetEnvironmentVariable($taskName, $taskPrevious[$taskName], 'Process') }
    $taskCredential = $null
    $taskSecure = $null
}
