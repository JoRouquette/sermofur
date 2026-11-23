# Installs the Rust `smf` from a GitHub Release of Sermofur, on Windows x64 (Windows PowerShell
# 5.1 or PowerShell 7).
#
#   irm https://raw.githubusercontent.com/JoRouquette/sermofur/main/rust/dist/install.ps1 | iex
#   .\install.ps1 [-Tag vX.Y.Z]
#
# Without a tag (parameter or SMF_VERSION), the latest Release is installed. The archive is
# checked against the SHA256SUMS of the same Release; any mismatch stops the install before
# anything is written. No administrator rights: the folder is added to the user PATH.
# Environment:
#   SMF_INSTALL_DIR      target folder (default: %LOCALAPPDATA%\Programs\smf\bin)
#   SMF_RELEASES_URL     releases base (default: https://github.com/JoRouquette/sermofur/releases)
#   SMF_NO_MODIFY_PATH   set to 1 to leave the user PATH unchanged
param([string]$Tag = $env:SMF_VERSION)

# Everything runs inside this function: under `irm | iex`, the preferences and working
# variables stay out of the caller's session (only $Tag and this function remain), and a
# failure throws instead of closing it.
function Install-Smf([string]$Tag) {
    $ErrorActionPreference = 'Stop'
    $ProgressPreference = 'SilentlyContinue'

    $releases = if ($env:SMF_RELEASES_URL) { $env:SMF_RELEASES_URL.TrimEnd('/') } else { 'https://github.com/JoRouquette/sermofur/releases' }
    $dir = if ($env:SMF_INSTALL_DIR) { $env:SMF_INSTALL_DIR } else { Join-Path $env:LOCALAPPDATA 'Programs\smf\bin' }
    # Relative to the PowerShell location, not to the process folder.
    $dir = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($dir)

    # Windows on Arm runs the x64 build through emulation.
    $arch = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture
    if ($env:OS -ne 'Windows_NT' -or ($arch -ne 'X64' -and $arch -ne 'Arm64')) {
        throw "install.ps1: no smf build for this system ($arch): use install.sh on Linux x64 or macOS arm64"
    }
    $target = 'windows-x64'

    [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12

    if (-not $Tag) {
        # /releases/latest redirects to /releases/tag/<tag>: no API call, no rate limit.
        try {
            $response = Invoke-WebRequest -Uri "$releases/latest" -Method Head -UseBasicParsing
        } catch {
            throw "install.ps1: cannot reach $releases/latest: $($_.Exception.Message)"
        }
        $final = if ($response.BaseResponse.ResponseUri) { $response.BaseResponse.ResponseUri } else { $response.BaseResponse.RequestMessage.RequestUri }
        $Tag = ([string]$final).TrimEnd('/').Split('/')[-1]
    }
    if (-not $Tag.StartsWith('v')) { $Tag = "v$Tag" }
    if ($Tag -notmatch '^v[0-9]+\.[0-9]+\.[0-9]+$') { throw "install.ps1: no release tag found (got '$Tag')" }

    $archive = "smf-$($Tag.Substring(1))-$target.zip"
    $tmp = Join-Path ([System.IO.Path]::GetTempPath()) ("smf-install-" + [guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $tmp | Out-Null
    $exe = Join-Path $dir 'smf.exe'
    try {
        Write-Host "install.ps1: downloading $archive ($Tag)"
        try {
            Invoke-WebRequest -Uri "$releases/download/$Tag/$archive" -OutFile (Join-Path $tmp $archive) -UseBasicParsing
        } catch {
            throw "install.ps1: cannot download ${archive}: the Release $Tag carries no smf archive for $target (yet?)"
        }
        try {
            Invoke-WebRequest -Uri "$releases/download/$Tag/SHA256SUMS" -OutFile (Join-Path $tmp 'SHA256SUMS') -UseBasicParsing
        } catch {
            throw "install.ps1: cannot download SHA256SUMS: $($_.Exception.Message)"
        }

        $expected = $null
        foreach ($line in Get-Content -LiteralPath (Join-Path $tmp 'SHA256SUMS')) {
            $fields = $line -split '\s+', 2
            if ($fields.Count -eq 2 -and $fields[1].TrimStart('*') -ceq $archive) { $expected = $fields[0] }
        }
        if (-not $expected) { throw "install.ps1: SHA256SUMS has no line for $archive" }
        $actual = (Get-FileHash -LiteralPath (Join-Path $tmp $archive) -Algorithm SHA256).Hash
        if ($actual -ne $expected) { throw "install.ps1: checksum mismatch for ${archive}: nothing installed" }

        Expand-Archive -LiteralPath (Join-Path $tmp $archive) -DestinationPath (Join-Path $tmp 'x')
        # Start the new binary before it replaces a working one.
        $printed = & (Join-Path $tmp 'x\smf.exe') --version
        if ($LASTEXITCODE -ne 0) { throw "install.ps1: the downloaded smf.exe does not run on this system: nothing replaced" }
        New-Item -ItemType Directory -Path $dir -Force | Out-Null
        # A running smf.exe (the daemon) cannot be overwritten, but it can be renamed; a unique
        # name keeps an older copy still in use from blocking this update.
        $old = $null
        if (Test-Path -LiteralPath $exe) {
            $old = "$exe.$([guid]::NewGuid().ToString('N')).old"
            Move-Item -LiteralPath $exe -Destination $old
        }
        try {
            Copy-Item -LiteralPath (Join-Path $tmp 'x\smf.exe') -Destination $exe
        } catch {
            if ($old) { Move-Item -LiteralPath $old -Destination $exe -Force }
            throw
        }
        Get-ChildItem -LiteralPath $dir -Filter 'smf.exe.*.old' | Remove-Item -Force -ErrorAction SilentlyContinue
    } finally {
        Remove-Item -LiteralPath $tmp -Recurse -Force -ErrorAction SilentlyContinue
    }

    Write-Host "install.ps1: installed $printed to $exe"

    # Read and write the raw registry value: %VAR% entries stay unexpanded, REG_EXPAND_SZ stays.
    $key = Get-Item -LiteralPath 'HKCU:\Environment'
    $raw = [string]$key.GetValue('Path', '', 'DoNotExpandEnvironmentNames')
    $entries = @($raw -split ';' | Where-Object { $_ })
    $expanded = @($entries | ForEach-Object { [Environment]::ExpandEnvironmentVariables($_).TrimEnd('\') })
    if ($expanded -notcontains $dir.TrimEnd('\')) {
        if ($env:SMF_NO_MODIFY_PATH -eq '1') {
            Write-Host "install.ps1: $dir is not in the user PATH (SMF_NO_MODIFY_PATH=1: left unchanged)."
        } else {
            Set-ItemProperty -LiteralPath 'HKCU:\Environment' -Name Path -Value (($entries + $dir) -join ';') -Type ExpandString
            # Tell running programs (Explorer, new terminals) that the environment changed.
            [Environment]::SetEnvironmentVariable('SMF_INSTALL_NOTIFY', '1', 'User')
            [Environment]::SetEnvironmentVariable('SMF_INSTALL_NOTIFY', $null, 'User')
            Write-Host "install.ps1: added $dir to the user PATH; open a new terminal to use smf."
        }
    }
}

Install-Smf -Tag $Tag
