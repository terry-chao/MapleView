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

    release 包会检查 exe 的 PE 子系统必须是图形界面，防止回归出「双击先弹
    一个控制台黑框」的版本。

    包里的 exe 文件名跟着系统语言走：中文环境是「枫阅.exe」，其他环境是
    mapleview.exe，跟 crates/app/src/brand.rs 决定窗口标题的规则一致。zip
    自己的名字始终是 ASCII，免得下载地址和脚本引用随语言变。

.EXAMPLE
    pwsh tools/package-windows.ps1

.EXAMPLE
    pwsh tools/package-windows.ps1 -IncludeCli -KeepStaging

.EXAMPLE
    pwsh tools/package-windows.ps1 -AppFileName mapleview.exe
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

    # 包里的 exe 文件名。不传就按系统语言决定，中文环境用「枫阅.exe」。
    [string]$AppFileName,

    [string]$OutputRoot = 'dist'
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

<#
.SYNOPSIS
    读出 PE 文件 optional header 里的 Subsystem 字段。

.DESCRIPTION
    2 表示 Windows 图形界面，3 表示控制台。exe 是哪个子系统决定双击时会不会
    先冒出一个黑框，而这一点只写在 PE 头里，从文件名或大小都看不出来。
#>
function Get-PeSubsystem {
    param([Parameter(Mandatory)][string]$Path)

    $stream = [IO.File]::OpenRead($Path)
    try {
        $reader = [IO.BinaryReader]::new($stream)
        $stream.Position = 0x3c
        $peOffset = $reader.ReadInt32()
        $stream.Position = $peOffset
        if ($reader.ReadUInt32() -ne 0x00004550) { # 'PE\0\0'
            throw "$Path 不是有效的 PE 文件"
        }
        # optional header 跟在 4 字节签名和 20 字节 COFF 头之后，其中的
        # Subsystem 字段在偏移 68（PE32 与 PE32+ 都一样）。
        $stream.Position = $peOffset + 24 + 68
        $reader.ReadUInt16()
    }
    finally {
        $stream.Dispose()
    }
}

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

    # brand.rs 用 GetUserDefaultLocaleName 决定窗口标题是「枫阅」还是
    # 「MapleView」，.NET 的 CurrentCulture 取的就是同一个值，所以照抄它的
    # 判断，exe 名字和标题不会一个中文一个英文。
    if (-not $AppFileName) {
        $AppFileName = if ([Globalization.CultureInfo]::CurrentCulture.Name -like 'zh*') {
            '枫阅.exe'
        }
        else {
            'mapleview.exe'
        }
    }

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
        # 只有 app 需要检查：CLI 本来就是控制台程序，黑框是它的正常形态。
        if ($binary -eq 'mapleview.exe' -and $Profile -eq 'release') {
            $subsystem = Get-PeSubsystem $from
            if ($subsystem -ne 2) {
                throw "$binary 的 PE 子系统是 $subsystem（2 = 图形界面，3 = 控制台）；" +
                    'release 包不该带控制台窗口，检查 crates/app/src/main.rs 的 windows_subsystem 属性'
            }
        }
        # app 在中文环境下换个名字，CLI 保持原名。
        $target = if ($binary -eq 'mapleview.exe') { $AppFileName } else { $binary }
        Copy-Item -LiteralPath $from -Destination (Join-Path $stage $target)
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
    Write-Host "  exe      $exeSize  $AppFileName" -ForegroundColor DarkGray
    if ($Profile -eq 'release') {
        Write-Host '           图形子系统，双击不弹控制台' -ForegroundColor DarkGray
    }
    Write-Host "  zip      $zipSize" -ForegroundColor DarkGray
    Write-Host "  sha256   $hash" -ForegroundColor DarkGray
    Write-Host ''
    Write-Host "产物：$(Resolve-Path -Relative $zip)" -ForegroundColor Green
}
finally {
    Pop-Location
}
