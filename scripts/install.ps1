# Cymose Code installer for Windows.
#
#   irm https://cymose.dev/install.ps1 | iex
#
# Downloads the release binary, verifies it against the published checksums,
# and puts it on your PATH. No build toolchain, no admin rights.

$ErrorActionPreference = "Stop"

$repo = "cymosehq/cymose-code"
$installDir = if ($env:CYMOSE_INSTALL_DIR) { $env:CYMOSE_INSTALL_DIR } else { "$env:LOCALAPPDATA\Cymose\bin" }
$target = "x86_64-pc-windows-msvc"
$asset = "cymose-$target.zip"
$base = "https://github.com/$repo/releases/latest/download"

if ([System.Environment]::Is64BitOperatingSystem -eq $false) {
	throw "Cymose Code needs a 64-bit Windows."
}

$tmp = Join-Path $env:TEMP ("cymose-" + [guid]::NewGuid())
New-Item -ItemType Directory -Path $tmp | Out-Null

try {
	Write-Host "Downloading $asset..."
	Invoke-WebRequest -Uri "$base/$asset" -OutFile "$tmp\$asset" -UseBasicParsing

	# Verify. An installer piped into a shell that doesn't check what it
	# downloaded asks you to trust the network as well as the project.
	try {
		Invoke-WebRequest -Uri "$base/SHA256SUMS" -OutFile "$tmp\SHA256SUMS" -UseBasicParsing
		$expected = (Select-String -Path "$tmp\SHA256SUMS" -Pattern ([regex]::Escape($asset)) |
			ForEach-Object { ($_ -split '\s+')[0] } | Select-Object -First 1)
		$actual = (Get-FileHash "$tmp\$asset" -Algorithm SHA256).Hash.ToLower()
		if ($expected -and $expected.ToLower() -ne $actual) {
			throw "Checksum mismatch - refusing to install."
		}
	} catch [System.Net.WebException] {
		Write-Warning "No SHA256SUMS in the release; skipping checksum verification."
	}

	Expand-Archive -Path "$tmp\$asset" -DestinationPath $tmp -Force
	New-Item -ItemType Directory -Path $installDir -Force | Out-Null
	Move-Item -Path "$tmp\cymose.exe" -Destination "$installDir\cymose.exe" -Force

	Write-Host ""
	Write-Host "Installed cymose to $installDir\cymose.exe"

	# Being on PATH is the difference between installed and usable, and the
	# failure is silent otherwise: a successful install that answers "command
	# not found".
	$userPath = [Environment]::GetEnvironmentVariable("Path", "User")
	if ($userPath -notlike "*$installDir*") {
		[Environment]::SetEnvironmentVariable("Path", "$userPath;$installDir", "User")
		Write-Host "Added $installDir to your PATH - open a new terminal for it to take effect."
	}

	Write-Host ""
	Write-Host 'Next: $env:OPENROUTER_API_KEY = "sk-or-v1-..."   then   cymose init; cymose'
} finally {
	Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
}
