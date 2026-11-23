param(
    [Parameter(Mandatory)]
    [string] $OutputDirectory
)

$ErrorActionPreference = 'Stop'
if (-not $IsWindows) { throw 'The complete workflow package requires Windows.' }
$root = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..')).Path
$output = [IO.Path]::GetFullPath($OutputDirectory)
if (Test-Path -LiteralPath $output) { throw 'Choose a new output directory.' }
if (-not (Test-Path -LiteralPath (Split-Path -Parent $output) -PathType Container)) {
    throw 'The output parent must exist.'
}

Push-Location -LiteralPath $root
try {
    $revision = git rev-parse HEAD
    if ($LASTEXITCODE -ne 0) { throw 'Could not resolve the source commit.' }
    $changes = git status --porcelain --untracked-files=all
    if ($LASTEXITCODE -ne 0 -or $changes) { throw 'Build from a clean, committed REEL checkout.' }
    $compiler = rustc --version --verbose
    if ($LASTEXITCODE -ne 0) { throw 'Could not identify the Rust compiler.' }

    $builds = @(
        @{ Manifest = 'Cargo.toml'; Args = @('--features', 'tig-history,speech-provider', '--bin', 'reel', '--bin', 'reel-tig-job', '--bin', 'reel-tig-speech', '--bin', 'reel-episode-captions'); Binaries = @('reel','reel-tig-job','reel-tig-speech','reel-episode-captions') }
        @{ Manifest = 'crates/reel-workflow/Cargo.toml'; Args = @('--bin','reel-workflow'); Binaries = @('reel-workflow') }
        @{ Manifest = 'crates/reel-workflow-native/Cargo.toml'; Args = @('--features','film','--bin','reel-workflow-native'); Binaries = @('reel-workflow-native') }
        @{ Manifest = 'crates/reel-workflow-http/Cargo.toml'; Args = @('--bin','reel-workflow-http'); Binaries = @('reel-workflow-http') }
        @{ Manifest = 'crates/reel-workflow-ui/Cargo.toml'; Args = @('--bin','reel-workflow-ui'); Binaries = @('reel-workflow-ui') }
    )
    # The package owns its build directory and is published only after all
    # binaries and the inventory have been written successfully.
    $stage = Join-Path (Split-Path -Parent $output) ('.reel-package-' + [Guid]::NewGuid())
    [void][IO.Directory]::CreateDirectory($stage)
    $target = Join-Path $stage 'build'
    $bundle = Join-Path $stage 'bundle'
    [void][IO.Directory]::CreateDirectory($bundle)
    $files = foreach ($build in $builds) {
        $arguments = $build.Args
        cargo build --release --locked --manifest-path $build.Manifest --target-dir $target @arguments
        if ($LASTEXITCODE -ne 0) { throw "Build failed. Evidence remains at $stage" }
        foreach ($name in $build.Binaries) {
            $filename = "$name.exe"
            $destination = Join-Path $bundle $filename
            [IO.File]::Copy((Join-Path $target "release/$filename"), $destination)
            $item = Get-Item -LiteralPath $destination
            [ordered]@{
                name = $filename
                bytes = $item.Length
                sha256 = (Get-FileHash -LiteralPath $destination -Algorithm SHA256).Hash.ToLowerInvariant()
            }
        }
    }
    if ((git rev-parse HEAD) -ne $revision -or (git status --porcelain --untracked-files=all)) {
        throw "Source changed during the build. Evidence remains at $stage"
    }
    [ordered]@{
        schema = 'reel.workflow-package.v3'
        source_commit = $revision
        rustc = ($compiler -join "`n")
        files = @($files)
    } | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $bundle 'manifest.json') -Encoding utf8
    [IO.Directory]::Move($bundle, $output)
    Write-Output $output
} finally {
    Pop-Location
}
