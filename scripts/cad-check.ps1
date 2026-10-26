# Open DXF and DWG files in AutoCAD's own engine, headless, and say whether each one opens.
#
#   scripts\cad-check.ps1 export.dxf other.dwg ...
#
# It uses accoreconsole.exe from an installed AutoCAD or the free DWG TrueView (found under
# "C:\Program Files\Autodesk"). Each file is opened and closed again without saving. A file
# AutoCAD refuses is reported with AutoCAD's reason, and the script exits with 1.
# Windows PowerShell 5.1 or later.

[CmdletBinding(PositionalBinding = $false)]
param(
    [Parameter(Mandatory = $true, ValueFromRemainingArguments = $true)]
    [string[]] $Files,
    [int] $TimeoutSeconds = 90
)

$console = Get-ChildItem "C:\Program Files\Autodesk" -Recurse -Filter accoreconsole.exe -ErrorAction SilentlyContinue |
    Select-Object -First 1
if ($null -eq $console) {
    Write-Error "accoreconsole.exe not found: install AutoCAD or DWG TrueView (free, from Autodesk)."
    exit 2
}

$work = Join-Path ([IO.Path]::GetTempPath()) ("drift-cad-check-" + [Guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Path $work | Out-Null
$script = Join-Path $work "quit.scr"
Set-Content -Path $script -Value "_.QUIT`r`nY`r`n" -Encoding ascii

$refused = 0
foreach ($file in $Files) {
    if (-not (Test-Path -LiteralPath $file -PathType Leaf)) {
        Write-Output "MISSING    $file"
        $refused++
        continue
    }
    $full = (Resolve-Path -LiteralPath $file).Path
    $log = Join-Path $work "out.txt"
    $arguments = "/i `"$full`" /s `"$script`" /l en-US"
    $p = Start-Process -FilePath $console.FullName -ArgumentList $arguments -RedirectStandardOutput $log -PassThru -NoNewWindow
    if (-not $p.WaitForExit($TimeoutSeconds * 1000)) {
        $p.Kill()
        Write-Output "TIMED OUT  $file"
        $refused++
        continue
    }
    $text = (Get-Content $log -Raw) -replace "`0", ""
    if ($text -match "Regenerating model") {
        Write-Output "opens      $file"
    } else {
        $why = ($text -split "`n" | Where-Object { $_ -match "Undefined|Invalid|error|damaged|discarded" } |
            ForEach-Object { $_.Trim() }) -join " / "
        Write-Output "REFUSED    $file  ($why)"
        $refused++
    }
}
Remove-Item -Recurse -Force $work
if ($refused -gt 0) { exit 1 }
