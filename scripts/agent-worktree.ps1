[CmdletBinding()]
param(
    [ValidateSet('new', 'list', 'remove', 'claim', 'release', 'status')]
    [string]$Action = 'list',
    [string]$Name,
    [string]$Area,
    [string]$Base = 'main',
    [switch]$Force
)

$ErrorActionPreference = 'Stop'

function Get-GitValue([string[]]$gitArgs) {
    $out = & git @gitArgs 2>$null
    if ($LASTEXITCODE -ne 0) { throw "git $($gitArgs -join ' ') failed" }
    ($out | Out-String).Trim()
}

$repo = Get-GitValue @('rev-parse', '--show-toplevel')
$common = Get-GitValue @('rev-parse', '--git-common-dir')
if (-not [IO.Path]::IsPathRooted($common)) { $common = Join-Path $repo $common }
$common = (Resolve-Path -LiteralPath $common).Path
$claimsDir = Join-Path $common 'claims'
$worktreesDir = Join-Path (Split-Path $repo -Parent) 'worktrees'
$claimLifetime = [TimeSpan]::FromHours(12)

function Get-Claims {
    if (-not (Test-Path -LiteralPath $claimsDir)) { return @() }
    Get-ChildItem -LiteralPath $claimsDir -Filter '*.json' -ErrorAction SilentlyContinue |
        ForEach-Object {
            $c = Get-Content -LiteralPath $_.FullName -Raw -Encoding UTF8 | ConvertFrom-Json
            $since = [datetime]::Parse($c.since)
            [pscustomobject]@{
                Area   = $c.area
                Agent  = $c.agent
                Branch = $c.branch
                Since  = $since
                Age    = (Get-Date) - $since
                File   = $_.FullName
            }
        }
}

function Show-Worktrees {
    $paths = @(& git worktree list --porcelain | Where-Object { $_ -like 'worktree *' } | ForEach-Object { $_.Substring(9) })
    foreach ($p in $paths) {
        $branch = & git -C $p rev-parse --abbrev-ref HEAD 2>$null
        $dirty = @(& git -C $p status --porcelain 2>$null).Count
        $claims = (Get-Claims | Where-Object { $_.Branch -eq "agent/$(Split-Path $p -Leaf)" }) | Measure-Object
        "{0,-52} {1,-34} dirty={2}" -f $p, $branch, $dirty
    }
}

function Show-Claims {
    $claims = @(Get-Claims)
    if ($claims.Count -eq 0) { "no active claims"; return }
    foreach ($c in $claims | Sort-Object Area) {
        $state = if ($c.Age -gt $claimLifetime) { "STALE" } else { "active" }
        "{0,-12} {1,-16} {2,-24} {3}" -f $c.Area, $c.Agent, $c.Branch, $state
    }
}

switch ($Action) {
    'new' {
        if (-not $Name) { throw '-Name is required' }
        $branch = "agent/$Name"
        $path = Join-Path $worktreesDir $Name
        if (Test-Path -LiteralPath $path) { throw "worktree already exists: $path" }
        New-Item -ItemType Directory -Force -Path $worktreesDir | Out-Null
        $exists = & git rev-parse --verify --quiet "refs/heads/$branch"
        if ($LASTEXITCODE -eq 0 -and $exists) {
            & git worktree add $path $branch
        } else {
            & git worktree add -b $branch $path $Base
        }
        "worktree: $path"
        "branch:   $branch"
    }
    'list' { Show-Worktrees }
    'status' {
        Show-Worktrees
        ""
        Show-Claims
    }
    'remove' {
        if (-not $Name) { throw '-Name is required' }
        $path = Join-Path $worktreesDir $Name
        if (-not (Test-Path -LiteralPath $path)) { & git worktree prune; "already removed"; return }
        & git worktree remove $path
        if ($LASTEXITCODE -ne 0) { throw "worktree remove failed; commit or discard its changes first" }
        "removed worktree $path (branch agent/$Name kept)"
    }
    'claim' {
        if (-not $Name -or -not $Area) { throw '-Name and -Area are required' }
        New-Item -ItemType Directory -Force -Path $claimsDir | Out-Null
        $conflict = Get-Claims | Where-Object {
            $_.Area -eq $Area -and $_.Agent -ne $Name -and $_.Age -lt $claimLifetime
        }
        if ($conflict -and -not $Force) {
            $who = ($conflict | ForEach-Object { "$($_.Agent) since $($_.Since.ToString('HH:mm'))" }) -join ', '
            throw "area '$Area' is claimed by $who; pass -Force to override (records a takeover)"
        }
        $branch = & git rev-parse --abbrev-ref HEAD 2>$null
        $body = @{
            area   = $Area
            agent  = $Name
            branch = "$branch"
            since  = (Get-Date).ToString('yyyy-MM-ddTHH:mm:sszzz')
            pid    = $PID
        } | ConvertTo-Json
        $file = Join-Path $claimsDir ("{0}__{1}.json" -f $Area, $Name)
        Set-Content -LiteralPath $file -Value $body -Encoding UTF8
        "claimed $Area for $Name"
    }
    'release' {
        if (-not $Name -or -not $Area) { throw '-Name and -Area are required' }
        $file = Join-Path $claimsDir ("{0}__{1}.json" -f $Area, $Name)
        if (Test-Path -LiteralPath $file) {
            Remove-Item -LiteralPath $file -Force
            "released $Area"
        } else {
            "no claim for $Area by $Name"
        }
    }
}
