# Build dist\RealityHop.exe (one file). Needs: pip install -r requirements.txt pyinstaller
$ErrorActionPreference = 'Stop'
Set-Location $PSScriptRoot
python -m PyInstaller --noconfirm --onefile --noconsole --name RealityHop `
    --icon src\resources\app.ico --add-data "src\resources;resources" --paths src src\realityhop.pyw
$h = (Get-FileHash dist\RealityHop.exe -Algorithm SHA256).Hash.ToLower()
"$h  RealityHop.exe" | Set-Content -Encoding ascii dist\RealityHop.exe.sha256
Write-Host "dist\RealityHop.exe  sha256 $h"
