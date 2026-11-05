# Build dist\RealityHop.exe, the one file that is released. Needs: pip install -r requirements.txt
# It carries the program as a folder (RealityHop.exe + app-<version>, zipped): that is what gets installed, and it
# starts without unpacking anything. The setup and the updater (RealityHop.exe --unpack-to) put the folder in place.
$ErrorActionPreference = 'Stop'
Set-Location $PSScriptRoot
# Version resource: Task Manager, the startup apps list and the file's Properties show "Reality Hop tunnel manager" and the version
$v = (Get-Content src\version.py -Raw) -replace "(?s).*VERSION = '([^']+)'.*", '$1'
$n = ((@([regex]::Matches($v, '\d+') | ForEach-Object { $_.Value }) + @('0', '0', '0', '0'))[0..3]) -join ', '
New-Item -ItemType Directory -Force build | Out-Null
[IO.File]::WriteAllText("$PWD\build\version-info.txt", @"
VSVersionInfo(
  ffi=FixedFileInfo(filevers=($n), prodvers=($n)),
  kids=[StringFileInfo([StringTable('040904B0', [
    StringStruct('FileDescription', 'Reality Hop tunnel manager'), StringStruct('ProductName', 'Reality Hop'),
    StringStruct('Comments', 'Keeps a sing-box VLESS Reality tunnel working on this PC and a GL.iNet router: tests nodes, swaps dead ones, runs in the tray.'),
    StringStruct('FileVersion', '$v'), StringStruct('ProductVersion', '$v'),
    StringStruct('OriginalFilename', 'RealityHop.exe'), StringStruct('InternalName', 'RealityHop'),
    StringStruct('LegalCopyright', '(c) 2026 KillerKingSolo, CC BY-NC-ND 4.0, github.com/KillerKingSolo/RealityHop')])]),
    VarFileInfo([VarStruct('Translation', [1033, 1200])])])
"@)   # no byte order mark: PyInstaller reads it as Python
$common = @('--noconfirm', '--noconsole', '--name', 'RealityHop', '--version-file', 'build\version-info.txt',
            '--icon', 'src\resources\app.ico', '--add-data', 'src\resources;resources', '--paths', 'src')
$ErrorActionPreference = 'Continue'   # PyInstaller logs to stderr; a failed step is caught by its exit code
# the program files go in app-<version>: an update unpacks the new folder next to the running one
python -m PyInstaller @common --onedir --contents-directory "app-$v" --distpath build\onedir --workpath build\work-onedir src\realityhop.pyw
if ($LASTEXITCODE) { throw 'folder build failed' }
Remove-Item build\RealityHop-app.zip -ErrorAction SilentlyContinue
python -c "import os, sys, zipfile; s, d = sys.argv[1:]; z = zipfile.ZipFile(d, 'w', zipfile.ZIP_DEFLATED, compresslevel=9); [z.write(os.path.join(r, f), os.path.relpath(os.path.join(r, f), s).replace(os.sep, '/')) for r, _, fs in os.walk(s) for f in fs]; z.close()" build\onedir\RealityHop build\RealityHop-app.zip
if ($LASTEXITCODE) { throw 'zip failed' }
python -m PyInstaller @common --onefile --workpath build\work-onefile --add-data "build\RealityHop-app.zip;." src\realityhop.pyw
if ($LASTEXITCODE) { throw 'one-file build failed' }
Write-Host "dist\RealityHop.exe  SHA-256 $((Get-FileHash dist\RealityHop.exe -Algorithm SHA256).Hash.ToLower())"
