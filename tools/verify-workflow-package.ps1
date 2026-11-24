param(
    [Parameter(Mandatory)]
    [string] $Bundle,
    [Parameter(Mandatory)]
    [ValidatePattern('^[0-9a-f]{40}$')]
    [string] $SourceCommit,
    [switch] $Smoke
)

$ErrorActionPreference = 'Stop'
$root = (Resolve-Path -LiteralPath $Bundle).Path
$manifest = Get-Content -LiteralPath (Join-Path $root 'manifest.json') -Raw | ConvertFrom-Json
$expected = @(
    'reel.exe'
    'reel-tig-job.exe'
    'reel-tig-speech.exe'
    'reel-workflow.exe'
    'reel-workflow-native.exe'
    'reel-workflow-http.exe'
)
if ($manifest.schema -cnotin @('reel.workflow-package.v1', 'reel.workflow-package.v2', 'reel.workflow-package.v3', 'reel.workflow-package.v4', 'reel.workflow-package.v5') -or
    $manifest.source_commit -ne $SourceCommit) {
    throw 'Unexpected package schema or source commit.'
}
if ($manifest.schema -cne 'reel.workflow-package.v1') {
    $expected += 'reel-episode-captions.exe'
}
if ($manifest.schema -cin @('reel.workflow-package.v3', 'reel.workflow-package.v4', 'reel.workflow-package.v5')) {
    $expected += 'reel-workflow-ui.exe'
}
if ($manifest.schema -cin @('reel.workflow-package.v4', 'reel.workflow-package.v5')) {
    $expected += 'reel-episode-conform.exe'
}
if ($manifest.schema -ceq 'reel.workflow-package.v5') {
    $expected += 'reel-publication.exe'
}
$names = @($manifest.files | ForEach-Object { $_.name } | Sort-Object)
if (($names -join ',') -cne (($expected | Sort-Object) -join ',')) {
    throw 'Unexpected binary inventory.'
}
foreach ($entry in $manifest.files) {
    $path = Join-Path $root $entry.name
    $file = Get-Item -LiteralPath $path
    if ($file.PSIsContainer -or $file.Length -ne $entry.bytes -or
        (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash -ne $entry.sha256) {
        throw "Package bytes differ for $($entry.name)."
    }
}
if ($Smoke) {
    foreach ($name in $expected) {
        $help = & (Join-Path $root $name) --help
        if ($LASTEXITCODE -ne 0 -or !$help) {
            throw "Help smoke check failed for $name."
        }
    }
}

Write-Output "Verified $($expected.Count) executables from $SourceCommit."
