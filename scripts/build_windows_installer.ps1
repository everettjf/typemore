[CmdletBinding()]
param(
  [string]$Compil32Path = "D:\Program Files (x86)\Inno Setup 6\Compil32.exe",
  [switch]$SkipNpmInstall
)

$ErrorActionPreference = "Stop"

function Invoke-Step {
  param(
    [Parameter(Mandatory = $true)]
    [string]$FilePath,
    [string[]]$ArgumentList = @(),
    [string]$WorkingDirectory = $PWD.Path
  )

  Write-Host ">> $FilePath $($ArgumentList -join ' ')"
  Push-Location $WorkingDirectory
  try {
    & $FilePath @ArgumentList
    if ($LASTEXITCODE -ne 0) {
      throw "Command failed with exit code $LASTEXITCODE"
    }
  } finally {
    Pop-Location
  }
}

function Find-LibclangPath {
$pythonScript = @"
import importlib.util
import pathlib

spec = importlib.util.find_spec('clang')
if spec is not None and spec.origin:
    native_dir = pathlib.Path(spec.origin).resolve().parent / 'native'
    candidate = native_dir / 'libclang.dll'
    if candidate.exists():
        print(native_dir)
"@

  try {
    $detected = (& python -c $pythonScript).Trim()
    if ($detected) {
      return $detected
    }
  } catch {
  }

  $fallbacks = @(
    "C:\Program Files\LLVM\bin",
    "$env:LOCALAPPDATA\Programs\LLVM\bin"
  )

  foreach ($candidate in $fallbacks) {
    if (Test-Path (Join-Path $candidate "libclang.dll")) {
      return $candidate
    }
  }

  throw "Unable to locate libclang.dll. Install LLVM or run: python -m pip install --user libclang"
}

$repoRoot = Split-Path -Parent $PSScriptRoot
$packageJson = Get-Content (Join-Path $repoRoot "package.json") -Raw | ConvertFrom-Json
$appVersion = $packageJson.version
$releaseDir = Join-Path $repoRoot "src-tauri\target\release"
$stageDir = Join-Path $releaseDir "inno-stage"
$outputDir = Join-Path $releaseDir "inno-output"
$issPath = Join-Path $PSScriptRoot "windows-installer.iss"
$compilerDir = Split-Path -Parent $Compil32Path
$isccPath = Join-Path $compilerDir "ISCC.exe"
$compilerPath = if (Test-Path $isccPath) { $isccPath } else { $Compil32Path }
$usesCompil32 = [System.IO.Path]::GetFileName($compilerPath).Equals("Compil32.exe", [System.StringComparison]::OrdinalIgnoreCase)

if (-not (Test-Path $compilerPath)) {
  throw "Inno Setup compiler not found: $compilerPath"
}

$env:LIBCLANG_PATH = Find-LibclangPath
Write-Host "Using LIBCLANG_PATH=$env:LIBCLANG_PATH"

if (-not $SkipNpmInstall) {
  Invoke-Step -FilePath "npm" -ArgumentList @("install") -WorkingDirectory $repoRoot
}

# Build the production Windows binary through Tauri so the executable
# includes the custom protocol required to load the bundled frontend.
Invoke-Step -FilePath "npm" -ArgumentList @("run", "tauri", "--", "build", "--no-bundle", "--no-sign") -WorkingDirectory $repoRoot

if (Test-Path $stageDir) {
  Remove-Item $stageDir -Recurse -Force
}
New-Item -ItemType Directory -Path $stageDir | Out-Null
New-Item -ItemType Directory -Path $outputDir -Force | Out-Null

$runtimeFiles = @(
  Join-Path $releaseDir "TypeMore.exe"
)

foreach ($file in $runtimeFiles) {
  if (-not (Test-Path $file)) {
    throw "Missing runtime file: $file"
  }
  Copy-Item $file -Destination $stageDir -Force
}

Get-ChildItem $releaseDir -File -Filter "*.dll" | ForEach-Object {
  Copy-Item $_.FullName -Destination $stageDir -Force
}

$compilerArgs = @(
  "/DAppVersion=$appVersion",
  "/DRepoRoot=$repoRoot",
  "/DSourceDir=$stageDir",
  "/DOutputDir=$outputDir",
  $issPath
)

if ($usesCompil32) {
  $compilerArgs = @("/cc") + $compilerArgs
}

Invoke-Step -FilePath $compilerPath -ArgumentList $compilerArgs -WorkingDirectory $repoRoot

$installer = Get-ChildItem $outputDir -File -Filter "TypeMore-Setup-*.exe" | Sort-Object LastWriteTime -Descending | Select-Object -First 1
if (-not $installer) {
  throw "Installer was not generated in $outputDir"
}

Write-Host ""
Write-Host "Installer created: $($installer.FullName)"
