# ============================================================
# ScanKing - one-shot Android APK build script
# Usage:  powershell -NoProfile -ExecutionPolicy Bypass -File scripts\build_apk.ps1
# Flags:  -Release      build release (needs signing config; default: debug, installable)
#         -Reinit       delete src-tauri\gen\android and re-init
#         -NoMirror     do not use China mirrors (gradle/maven/rustup)
#         -Target xxx   aarch64 (default) | armv7 | i686 | x86_64 | all
# All command output -> scripts\build.log ; console shows steps + heartbeat.
# ============================================================
param(
    [switch]$Release,
    [switch]$Reinit,
    [switch]$NoMirror,
    [string]$Target = "aarch64"
)

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
$Log  = Join-Path $PSScriptRoot "build.log"
Set-Content -Path $Log -Value "=== ScanKing build started $(Get-Date -Format 'yyyy-MM-dd HH:mm:ss') ===" -Encoding utf8

function Step($msg) {
    $line = "=== STEP: $msg ==="
    Write-Host $line -ForegroundColor Cyan
    Add-Content $Log $line
}
function Fail($msg) {
    $line = "!!! FAILED: $msg"
    Write-Host $line -ForegroundColor Red
    Add-Content $Log $line
    Add-Content $Log "=== BUILD FAILED ==="
    exit 1
}

# Run a command with stdout/stderr redirected to temp files, mirror them into
# build.log every 2s (no pipe deadlock), print a console heartbeat every 30s.
function Run($exe, $arguments, $cwd) {
    if (-not $cwd) { $cwd = $Root }
    Add-Content $Log ">>> $exe $($arguments -join ' ')   [cwd=$cwd]"
    $outF = Join-Path $env:TEMP "sk_run_out.tmp"
    $errF = Join-Path $env:TEMP "sk_run_err.tmp"
    Remove-Item $outF, $errF -ErrorAction SilentlyContinue
    $p = Start-Process -FilePath $exe -ArgumentList ($arguments -join " ") `
        -WorkingDirectory $cwd -NoNewWindow -PassThru `
        -RedirectStandardOutput $outF -RedirectStandardError $errF
    $null = $p.Handle  # cache handle so ExitCode is readable after exit
    $oPos = 0; $ePos = 0; $tick = 0
    $sync = {
        param($file, [ref]$pos)
        try {
            if (Test-Path $file) {
                $fs = New-Object IO.FileStream($file, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::ReadWrite)
                try {
                    if ($fs.Length -gt $pos.Value) {
                        $fs.Position = $pos.Value
                        $sr = New-Object IO.StreamReader($fs)
                        $chunk = $sr.ReadToEnd()
                        $pos.Value = $fs.Position
                        if ($chunk) { Add-Content -Path $Log -Value $chunk -NoNewline }
                    }
                } finally { $fs.Close() }
            }
        } catch {}
    }
    while (-not $p.HasExited) {
        Start-Sleep -Seconds 2
        & $sync $outF ([ref]$oPos)
        & $sync $errF ([ref]$ePos)
        $tick++
        if ($tick % 15 -eq 0) {
            $tail = ""
            try { $tail = (Get-Content $Log -Tail 1 -ErrorAction SilentlyContinue) } catch {}
            Write-Host ("    ... running {0} min | {1}" -f [math]::Round($tick * 2 / 60.0, 1), $tail) -ForegroundColor DarkGray
        }
    }
    $p.WaitForExit()
    Start-Sleep -Milliseconds 500
    & $sync $outF ([ref]$oPos)
    & $sync $errF ([ref]$ePos)
    Add-Content $Log ""
    if ($null -eq $p.ExitCode) { return 0 }
    return $p.ExitCode
}
function Refresh-Path {
    $env:Path = [Environment]::GetEnvironmentVariable("Path", "Machine") + ";" +
                [Environment]::GetEnvironmentVariable("Path", "User") + ";" +
                "$env:USERPROFILE\.cargo\bin"
}

$UseMirror = -not $NoMirror

# ---------- 1. Rust ----------
Step "check Rust toolchain"
Refresh-Path
$cargo = Get-Command cargo -ErrorAction SilentlyContinue
if (-not $cargo) {
    Step "Rust not found - installing rustup (minimal, stable)"
    if ($UseMirror) {
        $env:RUSTUP_DIST_SERVER = "https://rsproxy.cn"
        $env:RUSTUP_UPDATE_ROOT = "https://rsproxy.cn/rustup"
    }
    $ri = Join-Path $env:TEMP "rustup-init.exe"
    try {
        Invoke-WebRequest "https://static.rust-lang.org/rustup/dist/x86_64-pc-windows-msvc/rustup-init.exe" -OutFile $ri
    } catch {
        if ($UseMirror) { Invoke-WebRequest "https://rsproxy.cn/rustup/dist/x86_64-pc-windows-msvc/rustup-init.exe" -OutFile $ri }
        else { Fail "cannot download rustup-init.exe" }
    }
    $rc = Run $ri @("-y", "--profile", "minimal", "--default-toolchain", "stable")
    if ($rc -ne 0) { Fail "rustup install failed (see build.log)" }
    Refresh-Path
    $cargo = Get-Command cargo -ErrorAction SilentlyContinue
    if (-not $cargo) { Fail "cargo still not on PATH after install" }
}
Add-Content $Log (& cargo --version)

Step "add Android target ($Target)"
$targets = if ($Target -eq "all") { @("aarch64-linux-android","armv7-linux-androideabi","i686-linux-android","x86_64-linux-android") }
           else { @("$(@{aarch64='aarch64-linux-android';armv7='armv7-linux-androideabi';i686='i686-linux-android';x86_64='x86_64-linux-android'}[$Target])") }
foreach ($t in $targets) {
    $rc = Run "rustup" @("target", "add", $t)
    if ($rc -ne 0) { Fail "rustup target add $t failed" }
}

# ---------- 2. tauri-cli ----------
Step "check tauri-cli"
$hasTauri = $false
try { $v = (& cargo tauri --version) 2>$null; if ($LASTEXITCODE -eq 0 -and $v -match "^tauri-cli 2") { $hasTauri = $true; Add-Content $Log $v } } catch {}
if (-not $hasTauri) {
    Step "installing tauri-cli (try prebuilt, fallback to cargo install)"
    $ok = $false
    foreach ($name in @("cargo-tauri-x86_64-pc-windows-msvc.zip", "cargo-tauri-windows-x86_64.zip")) {
        try {
            $zip = Join-Path $env:TEMP $name
            Invoke-WebRequest "https://github.com/tauri-apps/tauri/releases/latest/download/$name" -OutFile $zip -TimeoutSec 120
            $dst = "$env:USERPROFILE\.cargo\bin"
            New-Item -ItemType Directory -Force -Path $dst | Out-Null
            Expand-Archive -Path $zip -DestinationPath $dst -Force
            if (Test-Path "$dst\cargo-tauri.exe") { $ok = $true; break }
        } catch { Add-Content $Log "prebuilt $name failed: $($_.Exception.Message)" }
    }
    if (-not $ok) {
        Add-Content $Log "falling back to: cargo install tauri-cli --locked (this takes a while)"
        $rc = Run "cargo" @("install", "tauri-cli", "--version", "^2", "--locked")
        if ($rc -ne 0) { Fail "tauri-cli install failed" }
    }
    Refresh-Path
}

# ---------- 3. Android SDK / NDK / JDK ----------
Step "locate Android SDK"
$sdk = $env:ANDROID_HOME
if (-not $sdk -or -not (Test-Path $sdk)) { $sdk = "$env:LOCALAPPDATA\Android\Sdk" }
if (-not (Test-Path $sdk)) { Fail "Android SDK not found. Set ANDROID_HOME or install via Android Studio (default: %LOCALAPPDATA%\Android\Sdk)" }
$env:ANDROID_HOME = $sdk
Add-Content $Log "ANDROID_HOME=$sdk"

Step "locate NDK"
$ndkRoot = Join-Path $sdk "ndk"
$ndk = $null
if (Test-Path $ndkRoot) {
    $ndk = Get-ChildItem $ndkRoot -Directory | Sort-Object { [version]($_.Name -replace '[^0-9.].*$','') } | Select-Object -Last 1
}
if (-not $ndk) {
    Step "NDK missing - trying headless install via sdkmanager"
    $sdkmgr = @(
        "$sdk\cmdline-tools\latest\bin\sdkmanager.bat",
        "$sdk\cmdline-tools\bin\sdkmanager.bat",
        "$sdk\tools\bin\sdkmanager.bat"
    ) | Where-Object { Test-Path $_ } | Select-Object -First 1
    if (-not $sdkmgr) { Fail "NDK not installed and sdkmanager not found. In Android Studio SDK Manager tick 'NDK (Side by side)' once, or install cmdline-tools." }
    Add-Content $Log "accepting licenses..."
    & cmd /c "echo y| `"$sdkmgr`" --licenses" *>> $Log
    $rc = Run "cmd" @("/c", "`"$sdkmgr`" `"ndk;27.1.12297006`"")
    if ($rc -ne 0) { $rc = Run "cmd" @("/c", "`"$sdkmgr`" `"ndk;26.1.10909125`"") }
    if ($rc -ne 0) { Fail "sdkmanager NDK install failed" }
    $ndk = Get-ChildItem $ndkRoot -Directory | Sort-Object { [version]($_.Name -replace '[^0-9.].*$','') } | Select-Object -Last 1
}
$env:NDK_HOME = $ndk.FullName
$env:ANDROID_NDK_HOME = $ndk.FullName
$env:ANDROID_NDK_ROOT = $ndk.FullName
Add-Content $Log "NDK_HOME=$($ndk.FullName)"

Step "locate JDK"
$javaCands = @($env:JAVA_HOME,
    "C:\Program Files\Android\Android Studio\jbr",
    "C:\Program Files\Android\Android Studio\jre") | Where-Object { $_ -and (Test-Path "$_\bin\java.exe") }
if (-not $javaCands) { Fail "no JDK found. Install Android Studio bundled JBR or set JAVA_HOME" }
$env:JAVA_HOME = @($javaCands) | Select-Object -First 1
Add-Content $Log "JAVA_HOME=$($env:JAVA_HOME)"

# ---------- 4. OCR models (optional) ----------
Step "check OCR models"
if (-not (Test-Path (Join-Path $Root "ui\models\det.onnx"))) {
    try {
        & powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot "fetch_models.ps1") *>> $Log
    } catch {
        Add-Content $Log "WARN: model download failed - APK will build without OCR models (OCR shows install hint)"
    }
} else { Add-Content $Log "models present" }

# ---------- 5. android init ----------
$gen = Join-Path $Root "src-tauri\gen\android"
if ($Reinit -and (Test-Path $gen)) { Step "removing old gen\android"; Remove-Item -Recurse -Force $gen }
if (-not (Test-Path $gen)) {
    Step "cargo tauri android init"
    $rc = Run "cargo" @("tauri", "android", "init")
    if ($rc -ne 0) { Fail "android init failed (see build.log)" }
} else { Step "gen\android exists - skip init" }

Step "generate icons"
$rc = Run "cargo" @("tauri", "icon", "app-icon.png")
if ($rc -ne 0) { Add-Content $Log "WARN: icon generation failed (non-fatal)" }

# ---------- 6. patches ----------
Step "patch AndroidManifest (camera permission)"
$manifest = Join-Path $gen "app\src\main\AndroidManifest.xml"
if (-not (Test-Path $manifest)) { Fail "AndroidManifest.xml not found at $manifest" }
$mtxt = Get-Content $manifest -Raw -Encoding utf8
if ($mtxt -notmatch "android.permission.CAMERA") {
    $ins = "    <uses-permission android:name=`"android.permission.CAMERA`" />`r`n    <uses-feature android:name=`"android.hardware.camera`" android:required=`"false`" />`r`n    <application"
    $mtxt = $mtxt -replace "\s*<application", "`r`n$ins"
    Set-Content -Path $manifest -Value $mtxt -Encoding utf8
    Add-Content $Log "camera permission added"
} else { Add-Content $Log "camera permission already present" }

Step "patch FileProvider paths (share to WeChat etc.)"
# 复用 Tauri 自带的 FileProvider：只需给它的 file_paths.xml 加 root-path。
# （注意不能再注册第二个同类名 provider——运行时只有一个实例响应，另一个的配置会失效）
$fp = Join-Path $gen "app\src\main\res\xml\file_paths.xml"
if (Test-Path $fp) {
    $fpx = Get-Content $fp -Raw -Encoding utf8
    if ($fpx -notmatch "root-path") {
        $fpx = $fpx -replace "</paths>", "  <root-path name=`"skroot`" path=`".`" />`r`n</paths>"
        Set-Content -Path $fp -Value $fpx -Encoding utf8
        Add-Content $Log "root-path added to file_paths.xml"
    } else { Add-Content $Log "root-path already present" }
} else { Add-Content $Log "WARN: file_paths.xml not found" }
# 清理旧版本残留的重复 provider 声明
$mtxt = Get-Content $manifest -Raw -Encoding utf8
if ($mtxt -match "skshare") {
    $mtxt = $mtxt -replace "(?s)\s*<provider[^>]*?skshare.*?</provider>", ""
    Set-Content -Path $manifest -Value $mtxt -Encoding utf8
    Add-Content $Log "removed legacy skshare provider"
}

Step "patch MainActivity (runtime camera permission)"
$mainDir = Join-Path $gen "app\src\main\java\com\ng\scanking"
$main = Join-Path $mainDir "MainActivity.kt"
if (-not (Test-Path $main)) { Fail "MainActivity.kt not found at $main" }
$kt = @"
package com.ng.scanking

import android.Manifest
import android.content.Context
import android.content.pm.PackageManager
import android.os.Bundle
import android.util.Log
import androidx.core.app.ActivityCompat
import androidx.core.content.ContextCompat

class MainActivity : TauriActivity() {
  companion object {
    @JvmStatic
    private external fun nativeInit(context: Context)
  }

  override fun onCreate(savedInstanceState: Bundle?) {
    super.onCreate(savedInstanceState)
    // 把 Application Context 交给 Rust（分享/相册/打开文件需要）
    try {
      nativeInit(applicationContext)
    } catch (e: Throwable) {
      Log.e("ScanKing", "nativeInit failed", e)
    }
    if (ContextCompat.checkSelfPermission(this, Manifest.permission.CAMERA)
        != PackageManager.PERMISSION_GRANTED) {
      ActivityCompat.requestPermissions(this, arrayOf(Manifest.permission.CAMERA), 1001)
    }
  }
}
"@
Set-Content -Path $main -Value $kt -Encoding utf8
Add-Content $Log "MainActivity.kt written"

Step "ensure androidx.core dependency"
$appGradle = @("app\build.gradle.kts", "app\build.gradle") | ForEach-Object { Join-Path $gen $_ } | Where-Object { Test-Path $_ } | Select-Object -First 1
if ($appGradle) {
    $g = Get-Content $appGradle -Raw -Encoding utf8
    if ($g -notmatch "androidx\.core:core-ktx") {
        if ($appGradle.EndsWith(".kts")) { $dep = "    implementation(`"androidx.core:core-ktx:1.13.1`")" }
        else { $dep = "    implementation 'androidx.core:core-ktx:1.13.1'" }
        $g = $g -replace "(dependencies\s*\{)", "`$1`r`n$dep"
        Set-Content -Path $appGradle -Value $g -Encoding utf8
        Add-Content $Log "core-ktx added to $appGradle"
    } else { Add-Content $Log "core-ktx already present" }
} else { Add-Content $Log "WARN: app gradle file not found" }

if ($Release) {
    Step "configure release signing"
    $ks = Join-Path $Root "scanking.keystore"
    $ksProps = Join-Path $gen "keystore.properties"
    if (-not (Test-Path $ks)) {
        $pw = -join ((48..57) + (65..90) + (97..122) | Get-Random -Count 20 | ForEach-Object { [char]$_ })
        $keytool = Join-Path $env:JAVA_HOME "bin\keytool.exe"
        $rc = Run $keytool @("-genkeypair", "-v", "-keystore", "`"$ks`"", "-alias", "scanking",
            "-keyalg", "RSA", "-keysize", "2048", "-validity", "10000",
            "-storepass", $pw, "-keypass", $pw,
            "-dname", "`"CN=ScanKing,OU=Dev,O=ScanKing,C=CN`"")
        if ($rc -ne 0) { Fail "keytool keystore generation failed" }
        $ksFwd = $ks -replace "\\", "/"
        Set-Content -Path $ksProps -Value "storeFile=$ksFwd`nstorePassword=$pw`nkeyAlias=scanking`nkeyPassword=$pw" -Encoding ascii
        Add-Content $Log "keystore + keystore.properties generated (KEEP THEM SAFE, both are gitignored)"
    } elseif (-not (Test-Path $ksProps)) {
        Fail "scanking.keystore exists but gen\android\keystore.properties is missing - create it manually (storeFile/storePassword/keyAlias/keyPassword)"
    }

    # wire signing into app/build.gradle.kts (idempotent)
    $bg = Join-Path $gen "app\build.gradle.kts"
    if (Test-Path $bg) {
        $g = Get-Content $bg -Raw -Encoding utf8
        if ($g -notmatch "keystore\.properties") {
            # 逐条查重：Tauri 模板可能已自带部分 import
            if ($g -notmatch "import java\.io\.FileInputStream") { $g = "import java.io.FileInputStream`r`n" + $g }
            if ($g -notmatch "import java\.util\.Properties") { $g = "import java.util.Properties`r`n" + $g }
            $loader = "val keystorePropertiesFile = rootProject.file(`"keystore.properties`")`r`nval keystoreProperties = Properties()`r`nif (keystorePropertiesFile.exists()) { keystoreProperties.load(FileInputStream(keystorePropertiesFile)) }`r`n"
            $g = $g -replace "(?m)^android \{", "$loader`r`nandroid {"
            $sign = "    signingConfigs {`r`n        create(`"release`") {`r`n            if (keystorePropertiesFile.exists()) {`r`n                keyAlias = keystoreProperties[`"keyAlias`"] as String`r`n                keyPassword = keystoreProperties[`"keyPassword`"] as String`r`n                storeFile = file(keystoreProperties[`"storeFile`"] as String)`r`n                storePassword = keystoreProperties[`"storePassword`"] as String`r`n            }`r`n        }`r`n    }"
            $g = $g -replace "(?m)^android \{", "android {`r`n$sign"
            $g = $g -replace "(getByName\(`"release`"\)\s*\{)", "`$1`r`n            signingConfig = signingConfigs.getByName(`"release`")"
            Set-Content -Path $bg -Value $g -Encoding utf8
            Add-Content $Log "signing wired into build.gradle.kts"
        } else { Add-Content $Log "signing already wired" }
    }

    # keep FileProvider API from being stripped by R8 (called via JNI only)
    $pg = Join-Path $gen "app\proguard-rules.pro"
    if ((Test-Path $pg) -and ((Get-Content $pg -Raw) -notmatch "FileProvider")) {
        Add-Content $pg "`n-keep class androidx.core.content.FileProvider { *; }"
        Add-Content $Log "proguard keep rule added"
    }
}

if ($UseMirror) {
    Step "apply China mirrors (gradle + maven)"
    $wrap = Join-Path $gen "gradle\wrapper\gradle-wrapper.properties"
    if (Test-Path $wrap) {
        $w = Get-Content $wrap -Raw -Encoding utf8
        $w2 = $w -replace "https\\?://services\.gradle\.org/distributions/", "https\://mirrors.cloud.tencent.com/gradle/"
        if ($w2 -ne $w) { Set-Content -Path $wrap -Value $w2 -Encoding utf8; Add-Content $Log "gradle dist mirror set" }
    }
    $mirrorKts = 'maven { url = uri("https://mirrors.cloud.tencent.com/nexus/repository/maven-public/") }'
    $mirrorGroovy = 'maven { url "https://mirrors.cloud.tencent.com/nexus/repository/maven-public/" }'
    Get-ChildItem $gen -Recurse -File -Include "*.gradle", "*.gradle.kts" | ForEach-Object {
        $f = $_.FullName
        $c = Get-Content $f -Raw -Encoding utf8
        if ($c -match "repositories\s*\{" -and $c -notmatch "mirrors\.cloud\.tencent\.com") {
            $mir = if ($f.EndsWith(".kts")) { $mirrorKts } else { $mirrorGroovy }
            $c = $c -replace "(repositories\s*\{)", "`$1`r`n        $mir"
            Set-Content -Path $f -Value $c -Encoding utf8
            Add-Content $Log "maven mirror -> $f"
        }
    }
}

# ---------- 7. build ----------
$mode = if ($Release) { "release" } else { "debug" }
# Force small dev builds via env vars - these override Cargo.toml, so the build
# stays small even if the profile section in Cargo.toml gets lost/reverted.
$env:CARGO_PROFILE_DEV_DEBUG = "0"
$env:CARGO_PROFILE_DEV_STRIP = "symbols"
# Always repackage the APK from scratch: Android's incremental packager updates
# the old APK in place and can leave huge dead space inside the zip.
$apkOut = Join-Path $gen "app\build\outputs\apk"
if (Test-Path $apkOut) { Remove-Item -Recurse -Force $apkOut; Add-Content $Log "cleared stale APK outputs" }
Step "cargo tauri android build ($mode, target=$Target) - the long part, watch scripts\build.log"
$buildArgs = @("tauri", "android", "build", "--apk")
if (-not $Release) { $buildArgs += "--debug" }
if ($Target -ne "all") { $buildArgs += @("--target", $Target) }
$rc = Run "cargo" $buildArgs
if ($rc -ne 0) { Fail "android build failed (see build.log tail)" }

# ---------- 8. collect APK ----------
Step "collect APK"
$outDir = Join-Path $gen "app\build\outputs\apk"
$apk = Get-ChildItem $outDir -Recurse -Filter "*.apk" -ErrorAction SilentlyContinue | Sort-Object LastWriteTime | Select-Object -Last 1
if (-not $apk) { Fail "no APK produced under $outDir" }
$dest = Join-Path $Root ("ScanKing-" + $mode + ".apk")
Copy-Item $apk.FullName $dest -Force
Add-Content $Log "APK: $($apk.FullName)"
Add-Content $Log "COPIED TO: $dest"
$apkMB = [math]::Round((Get-Item $dest).Length / 1MB, 1)
$soMsg = ""
$so = Join-Path $Root "target\aarch64-linux-android\$mode\libscanking_lib.so"
if (Test-Path $so) { $soMsg = " | libscanking_lib.so: $([math]::Round((Get-Item $so).Length / 1MB, 1)) MB" }
Write-Host ""
Write-Host "=== BUILD OK ===" -ForegroundColor Green
Write-Host "APK: $dest ($apkMB MB)$soMsg" -ForegroundColor Green
Add-Content $Log "APK size: $apkMB MB$soMsg"
Add-Content $Log "=== BUILD OK ==="
