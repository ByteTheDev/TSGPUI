# Installs the rendrix CLI on Windows into Bun's bin directory:
#
#   irm https://raw.githubusercontent.com/ByteTheDev/Rendrix/main/packages/cli/install/install.ps1 | iex
#
# Environment variables:
#   RENDRIX_VERSION         install this release tag, for example 0.3.0 (default: the latest release)
#   RENDRIX_URL             install the archive at this URL instead (a mirror, or a local test server)
#   RENDRIX_NO_MODIFY_PATH  set to 1 to leave the user PATH unchanged
#   BUN_INSTALL           Bun's home directory (default: %USERPROFILE%\.bun)
#
# The archive is unpacked to %BUN_INSTALL%\install\rendrix and a rendrix.cmd launcher is written to
# %BUN_INSTALL%\bin. The launcher runs the CLI with Bun when present, otherwise Node.js 20+.
# This is a function so that `irm | iex` does not leave $ErrorActionPreference changed in your session.

function Install-Rendrix {
  $ErrorActionPreference = 'Stop'
  $ProgressPreference = 'SilentlyContinue'
  $repoUrl = 'https://github.com/ByteTheDev/Rendrix'
  $archiveName = 'rendrix-cli.tgz'

  if ($env:OS -ne 'Windows_NT') { throw 'install.ps1 is for Windows; use install.sh on macOS and Linux.' }
  # Use Windows' own bsdtar by full path: a Git for Windows GNU tar earlier on PATH misreads C:\ paths.
  $tarExe = Join-Path $env:SystemRoot 'System32\tar.exe'
  if (-not (Test-Path $tarExe)) { throw 'tar.exe is required (Windows 10 build 1803 or newer).' }

  $hasBun = [bool](Get-Command bun -ErrorAction SilentlyContinue)
  $hasNode = $false
  if (Get-Command node -ErrorAction SilentlyContinue) {
    $nodeMajor = [int](& node -p "process.versions.node.split('.')[0]")
    $hasNode = $nodeMajor -ge 20
  }
  if (-not ($hasBun -or $hasNode)) { throw 'rendrix needs Bun or Node.js 20 or newer on PATH.' }

  if ($env:RENDRIX_URL) {
    $url = $env:RENDRIX_URL
  } elseif ($env:RENDRIX_VERSION) {
    $url = "$repoUrl/releases/download/v$($env:RENDRIX_VERSION.TrimStart('v'))/$archiveName"
  } else {
    $url = "$repoUrl/releases/latest/download/$archiveName"
  }

  # GitHub requires TLS 1.2; Windows PowerShell 5.1 may default to older protocols.
  [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12

  $tmp = Join-Path ([IO.Path]::GetTempPath()) ('rendrix-' + [guid]::NewGuid().ToString('N'))
  New-Item -ItemType Directory -Path $tmp | Out-Null
  try {
    $archive = Join-Path $tmp $archiveName
    Write-Host "rendrix: downloading $url"
    Invoke-WebRequest -Uri $url -OutFile $archive -UseBasicParsing
    Invoke-WebRequest -Uri "$url.sha256" -OutFile "$archive.sha256" -UseBasicParsing

    $expected = ((Get-Content -Path "$archive.sha256" -Raw).Trim() -split '\s+')[0]
    $actual = (Get-FileHash -Path $archive -Algorithm SHA256).Hash
    if ($actual -ne $expected) {
      throw "checksum mismatch for $archiveName (expected $expected, got $actual); nothing was installed."
    }

    $extract = Join-Path $tmp 'extract'
    New-Item -ItemType Directory -Path $extract | Out-Null
    & $tarExe -xzf $archive -C $extract --strip-components 1
    if ($LASTEXITCODE -ne 0) { throw 'could not unpack the archive.' }
    if (-not (Test-Path (Join-Path $extract 'dist\rendrix.mjs'))) { throw 'archive does not contain dist\rendrix.mjs.' }

    $bunHome = if ($env:BUN_INSTALL) { $env:BUN_INSTALL } else { Join-Path $env:USERPROFILE '.bun' }
    $pkgDir = Join-Path $bunHome 'install\rendrix'
    $binDir = Join-Path $bunHome 'bin'
    New-Item -ItemType Directory -Force -Path (Split-Path $pkgDir) | Out-Null
    New-Item -ItemType Directory -Force -Path $binDir | Out-Null
    if (Test-Path $pkgDir) { Remove-Item -Recurse -Force $pkgDir }
    Move-Item -Path $extract -Destination $pkgDir

    # The launcher finds the CLI relative to itself, so the install can move with $BUN_INSTALL.
    $shimLines = @(
      '@echo off',
      'rem rendrix launcher written by the Rendrix installer. Run install.ps1 again to update it.',
      'set "CLI=%~dp0..\install\rendrix\dist\rendrix.mjs"',
      'where /q bun',
      'if errorlevel 1 goto node',
      'bun "%CLI%" %*',
      'exit /b %errorlevel%',
      ':node',
      'node "%CLI%" %*',
      'exit /b %errorlevel%'
    )
    $shim = Join-Path $binDir 'rendrix.cmd'
    Set-Content -Path $shim -Value ($shimLines -join "`r`n") -Encoding Ascii

    if ($env:RENDRIX_NO_MODIFY_PATH -ne '1') {
      $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
      $entries = @($userPath -split ';' | Where-Object { $_ } | ForEach-Object { $_.TrimEnd('\') })
      if ($entries -notcontains $binDir.TrimEnd('\')) {
        $newPath = if ($userPath) { "$($userPath.TrimEnd(';'));$binDir" } else { $binDir }
        [Environment]::SetEnvironmentVariable('Path', $newPath, 'User')
        $env:Path = "$env:Path;$binDir"
        Write-Host "rendrix: added $binDir to your user PATH (new terminals will pick it up)"
      }
    }

    $version = & $shim --version
    Write-Host "rendrix: installed $version to $pkgDir"
    Write-Host "rendrix: next, run 'rendrix doctor' to check the toolchain"
  } finally {
    Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
  }
}

Install-Rendrix
