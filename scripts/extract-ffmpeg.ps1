param(
    [Parameter(Mandatory = $true)][string]$ArchivePath,
    [Parameter(Mandatory = $true)][string]$EntryName,
    [Parameter(Mandatory = $true)][string]$Destination
)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.IO.Compression.FileSystem
$archive = [System.IO.Compression.ZipFile]::OpenRead($ArchivePath)
try {
    $entry = $archive.GetEntry($EntryName)
    if ($null -eq $entry -or $entry.Length -gt 256MB) { throw 'Invalid FFmpeg binary entry' }
    # Extract one exact entry to an explicit path; no archive-controlled paths.
    [System.IO.Compression.ZipFileExtensions]::ExtractToFile($entry, $Destination, $false)
} finally {
    $archive.Dispose()
}
