$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing

$apps = @()
$seen = @{}
foreach ($process in Get-Process | Where-Object { $_.MainWindowHandle -ne 0 } | Sort-Object ProcessName) {
    $path = $process.Path
    if (-not $path) { continue }
    $exe = [IO.Path]::GetFileName($path).ToLowerInvariant()
    if ($exe -in @('explorer.exe', 'quill.exe') -or $seen.ContainsKey($exe)) { continue }
    $seen[$exe] = $true
    $name = [Diagnostics.FileVersionInfo]::GetVersionInfo($path).FileDescription
    if (-not $name) { $name = $process.ProcessName }
    $iconData = $null
    $icon = $null
    $bitmap = $null
    $stream = $null
    try {
        $icon = [Drawing.Icon]::ExtractAssociatedIcon($path)
        if ($icon) {
            $bitmap = $icon.ToBitmap()
            $stream = [IO.MemoryStream]::new()
            $bitmap.Save($stream, [Drawing.Imaging.ImageFormat]::Png)
            $iconData = 'data:image/png;base64,' + [Convert]::ToBase64String($stream.ToArray())
        }
    } catch {
        [Console]::Error.WriteLine("Could not extract icon for $exe")
    } finally {
        if ($stream) { $stream.Dispose() }
        if ($bitmap) { $bitmap.Dispose() }
        if ($icon) { $icon.Dispose() }
    }
    $apps += @{ exe = $exe; name = $name; icon = $iconData }
}
ConvertTo-Json -InputObject @($apps) -Compress
