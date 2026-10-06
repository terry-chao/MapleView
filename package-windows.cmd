@echo off
rem 双击这个文件就能打包 Windows 发行版，等价于命令行里的：
rem     pwsh tools/package-windows.ps1
rem
rem 也可以带参数跑，参数会原样转给 ps1，例如：
rem     package-windows.cmd -IncludeCli -KeepStaging

setlocal
chcp 65001 >nul
cd /d "%~dp0"

rem 优先 PowerShell 7；没有就退回系统自带的 Windows PowerShell。
set "PSH="
where pwsh >nul 2>nul && set "PSH=pwsh"
if not defined PSH (
    where powershell >nul 2>nul && set "PSH=powershell"
)
if not defined PSH (
    echo [错误] 没找到 PowerShell。装一个再回来跑：
    echo         winget install Microsoft.PowerShell
    set "CODE=1"
    goto :report
)

echo 正在打包 MapleView（release，Windows x64）……
echo.
"%PSH%" -NoLogo -NoProfile -ExecutionPolicy Bypass -File "%~dp0tools\package-windows.ps1" %*
set "CODE=%ERRORLEVEL%"

:report
echo.
if "%CODE%"=="0" (
    echo 打包成功，产物在 dist\ 目录（确切路径见上面「产物：」那一行）。
) else (
    echo 打包失败（退出码 %CODE%），错误信息往上翻。
)
echo.
pause
exit /b %CODE%
