<#
.SYNOPSIS
    打包 Windows 发行版。

.DESCRIPTION
    编译 release 可执行文件，连同 LICENSE 与 README 打成 zip，并写出 SHA256
    校验值。产物落在 dist/ 下：

        dist/MapleView-0.1.0-windows-x64.zip
        dist/MapleView-0.1.0-windows-x64.zip.sha256

    可执行文件的图标与版本信息由 crates/app/build.rs 在链接期写进 PE 资源，
    所以包里只需要 exe 本身，不需要额外的运行时文件。

.EXAMPLE
    pwsh tools/package-windows.ps1

.EXAMPLE
    pwsh tools/package-windows.ps1 -IncludeCli -KeepStaging
#>
[CmdletBinding()]
param(
    # release 是发布用的配置；debug 只在你确认要发调试包时才用。
    [ValidateSet('release', 'debug')]
    [string]$Profile = 'release',

    # 一并打包 mapleview-cli。
    [switch]$IncludeCli,

    # 跳过编译，直接用 target/ 里已有的产物重新打包。
    [switch]$SkipBuild,

    # 保留解包前的暂存目录，方便先手动跑一下再决定要不要发。
    [switch]$KeepStaging,

    [string]$OutputRoot = 'dist'
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$root = Split-Path -Parent $PSScriptRoot
Push-Location $root
try {
    # --- 版本与目标架构 ---------------------------------------------------
    $metadata = cargo metadata --format-version 1 --no-deps --offline | ConvertFrom-Json
    $app = $metadata.packages | Where-Object { $_.name -eq 'mapleview-app' }
    if (-not $app) { throw 'workspace 里找不到 mapleview-app' }
    $version = $app.version

    $hostLine = (rustc -vV | Select-String -Pattern '^host:\s*(.+)$').Matches
    if ($hostLine.Count -eq 0) { throw 'rustc -vV 没有报出 host 三元组' }
    $triple = $hostLine[0].Groups[1].Value.Trim()
    $arch = switch -Regex ($triple) {
        '^aarch64' { 'arm64' }
        '^i686' { 'x86' }
        default { 'x64' }
    }

    $name = "MapleView-$version-windows-$arch"
    $binDir = Join-Path $root (Join-Path 'target' $Profile)
    $outRoot = if ([IO.Path]::IsPathRooted($OutputRoot)) { $OutputRoot } else { Join-Path $root $OutputRoot }
    $stage = Join-Path $outRoot $name
    $zip = Join-Path $outRoot "$name.zip"

    Write-Host "打包 $name（$triple，$Profile）" -ForegroundColor Cyan

    # --- 编译 -------------------------------------------------------------
    if ($SkipBuild) {
        Write-Host '跳过编译，复用 target/ 里的产物'
    }
    else {
        $cargoArgs = @('build', '--locked')
        if ($Profile -eq 'release') { $cargoArgs += '--release' }
        $cargoArgs += @('-p', 'mapleview-app')
        if ($IncludeCli) { $cargoArgs += @('-p', 'mapleview-cli') }
        Write-Host "> cargo $($cargoArgs -join ' ')"
        & cargo @cargoArgs
        if ($LASTEXITCODE -ne 0) { throw "cargo build 失败，退出码 $LASTEXITCODE" }
    }

    # --- 暂存 -------------------------------------------------------------
    if (Test-Path -LiteralPath $stage) { Remove-Item -LiteralPath $stage -Recurse -Force }
    New-Item -ItemType Directory -Path $stage -Force | Out-Null

    $binaries = @('mapleview.exe')
    if ($IncludeCli) { $binaries += 'mapleview-cli.exe' }
    foreach ($binary in $binaries) {
        $from = Join-Path $binDir $binary
        if (-not (Test-Path -LiteralPath $from)) {
            throw "缺少 $from；先不带 -SkipBuild 跑一次"
        }
        Copy-Item -LiteralPath $from -Destination $stage
    }
    foreach ($doc in 'LICENSE', 'README.md') {
        $from = Join-Path $root $doc
        if (Test-Path -LiteralPath $from) { Copy-Item -LiteralPath $from -Destination $stage }
    }

    # --- 压缩与校验 -------------------------------------------------------
    New-Item -ItemType Directory -Path (Split-Path -Parent $zip) -Force | Out-Null
    if (Test-Path -LiteralPath $zip) { Remove-Item -LiteralPath $zip -Force }
    Compress-Archive -LiteralPath $stage -DestinationPath $zip -CompressionLevel Optimal

    $hash = (Get-FileHash -LiteralPath $zip -Algorithm SHA256).Hash.ToLowerInvariant()
    "$hash  $([IO.Path]::GetFileName($zip))" |
        Set-Content -LiteralPath "$zip.sha256" -Encoding ascii

    if (-not $KeepStaging) { Remove-Item -LiteralPath $stage -Recurse -Force }

    # --- 汇报 -------------------------------------------------------------
    $zipSize = '{0:N1} MB' -f ((Get-Item -LiteralPath $zip).Length / 1MB)
    $exeSize = Get-ChildItem -LiteralPath (Join-Path $binDir 'mapleview.exe') |
        ForEach-Object { '{0:N1} MB' -f ($_.Length / 1MB) }
    Write-Host ''
    Write-Host "  exe      $exeSize" -ForegroundColor DarkGray
    Write-Host "  zip      $zipSize" -ForegroundColor DarkGray
    Write-Host "  sha256   $hash" -ForegroundColor DarkGray
    Write-Host ''
    Write-Host "产物：$(Resolve-Path -Relative $zip)" -ForegroundColor Green
}
finally {
    Pop-Location
}
