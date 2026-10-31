# Build dist\RealityHop.exe (one file). Needs: pip install -r requirements.txt
$ErrorActionPreference = 'Stop'
Set-Location $PSScriptRoot
# Version resource: Task Manager, the startup apps list and the file's Properties show "Reality Hop" and the version
$v = (Get-Content src\version.py -Raw) -replace "(?s).*VERSION = '([^']+)'.*", '$1'
$n = ((@([regex]::Matches($v, '\d+') | ForEach-Object { $_.Value }) + @('0', '0', '0', '0'))[0..3]) -join ', '
New-Item -ItemType Directory -Force build | Out-Null
[IO.File]::WriteAllText("$PWD\build\version-info.txt", @"
VSVersionInfo(
  ffi=FixedFileInfo(filevers=($n), prodvers=($n)),
  kids=[StringFileInfo([StringTable('040904B0', [
    StringStruct('FileDescription', 'Reality Hop'), StringStruct('ProductName', 'Reality Hop'),
    StringStruct('FileVersion', '$v'), StringStruct('ProductVersion', '$v'),
    StringStruct('OriginalFilename', 'RealityHop.exe'), StringStruct('InternalName', 'RealityHop'),
    StringStruct('LegalCopyright', '(c) 2026 KillerKingSolo, CC BY-NC-ND 4.0, github.com/KillerKingSolo/RealityHop')])]),
    VarFileInfo([VarStruct('Translation', [1033, 1200])])])
"@)   # no byte order mark: PyInstaller reads it as Python
python -m PyInstaller --noconfirm --onefile --noconsole --name RealityHop --version-file build\version-info.txt `
    --icon src\resources\app.ico --add-data "src\resources;resources" --paths src src\realityhop.pyw
$h = (Get-FileHash dist\RealityHop.exe -Algorithm SHA256).Hash.ToLower()
Write-Host "dist\RealityHop.exe  SHA-256 $h"
