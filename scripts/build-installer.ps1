$ErrorActionPreference = "Stop"
Set-Location (Split-Path -Parent $PSScriptRoot)

# 1. Generate installer/pulih.ico matching crates/pulih-gui/src/theme.rs::icon()
$icoPath = Join-Path $PWD "installer\pulih.ico"
if (-not (Test-Path $icoPath)) {
    Write-Host "Generating installer\pulih.ico..."
    $N = 64
    $bmpHeaderSize = 40
    $pixelBytes = $N * $N * 4
    $maskRowBytes = [int][Math]::Ceiling($N / 32.0) * 4
    $maskBytes = $maskRowBytes * $N
    $imageSize = $bmpHeaderSize + $pixelBytes + $maskBytes

    $ms = New-Object System.IO.MemoryStream
    $bw = New-Object System.IO.BinaryWriter($ms)

    # ICONDIR (6 bytes)
    $bw.Write([uint16]0)   # Reserved
    $bw.Write([uint16]1)   # Type: 1 = ICO
    $bw.Write([uint16]1)   # Count: 1 image

    # ICONDIRENTRY (16 bytes)
    $bw.Write([byte]$N)    # Width
    $bw.Write([byte]$N)    # Height
    $bw.Write([byte]0)     # ColorCount
    $bw.Write([byte]0)     # Reserved
    $bw.Write([uint16]1)   # Planes
    $bw.Write([uint16]32)  # BitCount
    $bw.Write([uint32]$imageSize)
    $bw.Write([uint32]22)  # Offset (6 + 16)

    # BITMAPINFOHEADER (40 bytes)
    $bw.Write([uint32]$bmpHeaderSize)
    $bw.Write([int32]$N)
    $bw.Write([int32]($N * 2)) # Height * 2 for XOR + AND masks
    $bw.Write([uint16]1)       # Planes
    $bw.Write([uint16]32)      # BitCount
    $bw.Write([uint32]0)       # Compression (BI_RGB)
    $bw.Write([uint32]$pixelBytes)
    $bw.Write([int32]0)
    $bw.Write([int32]0)
    $bw.Write([uint32]0)
    $bw.Write([uint32]0)

    # Bottom-up BGRA pixels matching theme::icon()
    $sqrt2 = [Math]::Sqrt(2.0)
    for ($row = $N - 1; $row -ge 0; $row--) {
        for ($x = 0; $x -lt $N; $x++) {
            $fx = $x + 0.5
            $fy = $row + 0.5
            $r = 14.0
            $cx = [Math]::Min([Math]::Max($fx, $r), $N - $r)
            $cy = [Math]::Min([Math]::Max($fy, $r), $N - $r)
            $d = [Math]::Sqrt(($fx - $cx) * ($fx - $cx) + ($fy - $cy) * ($fy - $cy))
            $a = [Math]::Min([Math]::Max($r + 0.5 - $d, 0.0), 1.0)
            if ($a -le 0.0) {
                $bw.Write([byte]0); $bw.Write([byte]0); $bw.Write([byte]0); $bw.Write([byte]0)
                continue
            }
            $t = ($fx + $fy) / (2.0 * $N)
            $cr = 30.0 + 20.0 * $t
            $cg = 120.0 - 40.0 * $t
            $cb = 235.0 - 20.0 * $t
            $inDoc = ($fx -ge 17.0 -and $fx -lt 41.0) -and ($fy -ge 12.0 -and $fy -lt 48.0)
            if ($inDoc) {
                $cr = 245.0; $cg = 248.0; $cb = 255.0
                if (($fx -ge 21.0 -and $fx -lt 37.0) -and (([Math]::Abs($fy - 20.0) -lt 1.2) -or ([Math]::Abs($fy - 26.0) -lt 1.2) -or ([Math]::Abs($fy - 32.0) -lt 1.2))) {
                    $cr = 150.0; $cg = 175.0; $cb = 220.0
                }
            }
            $mx = 38.0; $my = 37.0
            $md = [Math]::Sqrt(($fx - $mx) * ($fx - $mx) + ($fy - $my) * ($fy - $my))
            $ring = [Math]::Abs($md - 9.5) -lt 2.6
            $hx = $fx - $mx; $hy = $fy - $my
            $along = ($hx + $hy) / $sqrt2
            $across = [Math]::Abs($hx - $hy) / $sqrt2
            $handle = ($along -ge 11.0 -and $along -lt 21.0) -and ($across -lt 3.0)
            if ($ring -or $handle) {
                $cr = 255.0; $cg = 196.0; $cb = 61.0
            } elseif ($md -lt 7.0 -and $inDoc) {
                $cr = 219.0; $cg = 234.0; $cb = 254.0
            }
            $bw.Write([byte]$cb)
            $bw.Write([byte]$cg)
            $bw.Write([byte]$cr)
            $bw.Write([byte]($a * 255.0))
        }
    }

    # 1-bit AND mask (all 0s since alpha channel is present)
    $bw.Write((New-Object byte[] $maskBytes))
    $bw.Flush()
    [System.IO.File]::WriteAllBytes($icoPath, $ms.ToArray())
}

# 2. Configure Rust environment and build release binaries
$binDir = "$env:USERPROFILE\.rustup\toolchains\stable-x86_64-pc-windows-gnu\lib\rustlib\x86_64-pc-windows-gnu\bin"
$selfContained = "$binDir\self-contained"
if (Test-Path $binDir) {
    if (-not (Test-Path "$binDir\llvm-dlltool.exe") -and (Test-Path "$binDir\llvm-ar.exe")) {
        Copy-Item "$binDir\llvm-ar.exe" "$binDir\llvm-dlltool.exe" -Force
    }
    $env:PATH = "$env:USERPROFILE\.cargo\bin;$binDir;$selfContained;$env:PATH"
    if (Test-Path "$binDir\llvm-dlltool.exe") {
        $env:RUSTFLAGS = "-C dlltool=$binDir\llvm-dlltool.exe"
    }
}
$env:CARGO_HTTP_CHECK_REVOKE = "false"

Write-Host "Building release binaries with Cargo..."
cargo build --release

# 3. Compile Windows Installer with Inno Setup
$isccCandidates = @(
    "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe",
    "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
    "$env:ProgramFiles\Inno Setup 6\ISCC.exe"
)
$iscc = $isccCandidates | Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $iscc) {
    throw "Inno Setup 6 (ISCC.exe) not found."
}

Write-Host "Packaging installer with Inno Setup..."
& $iscc "installer\pulih.iss"
Write-Host "Done! Installer generated at dist\Pulih-Setup-1.2.0.exe"

