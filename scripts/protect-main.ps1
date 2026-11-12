# Turns on branch protection for main. The owner runs this once, by hand, after the
# merge-gate PR and the flaky-test quarantine PR are both on main:
#
#   powershell -ExecutionPolicy Bypass -File scripts\protect-main.ps1
#
# What it sets (and why):
# - Every change to main goes through a pull request. 0 approving reviews: agents and the
#   owner share one GitHub account, and nobody can approve their own PR. The owner's merge is
#   the approval for gated PRs; the "Solver review" status is the review for solver ones.
# - The required checks below must pass, on a branch up to date with main (strict).
# - enforce_admins: the rules bind admins too, so neither the owner nor an agent using the
#   owner's token can merge red or push straight to main.
# - No force pushes to main, no deleting it. Merged branches are deleted.
#
# Agents must not run this (it is a security setting); the project's Claude Code settings
# deny the API calls it makes. Undo, if ever needed, from GitHub's branch settings page.

$ErrorActionPreference = 'Stop'
$repo = 'chrisplange/Drift'

$checks = @(
    'Format and lint',
    'Test (ubuntu-latest)',
    'Test (windows-latest)',
    'Test (macos-latest)',
    'Licences and advisories',
    'Third-party notices',
    'Blender add-on',
    'Merge-gate scripts',
    'Heavy drag matrix and pins (ubuntu)',
    'Tier',
    'Solver review'
)
# Not required: "Blender add-on (headless Blender 4.2)" downloads Blender and is allowed to
# fail; "Web build" is skipped on most PRs by its own rules.

Write-Host "Branch protection for $repo main will require:"
$checks | ForEach-Object { Write-Host "  - $_" }
Write-Host "strict (up to date with main), enforce_admins, no force pushes, no deletion."
Write-Host ""
Write-Host "Run this only after the flaky-test quarantine PR is merged, or flaky tests will block merges."
$answer = Read-Host "Type yes to apply"
if ($answer -ne 'yes') { Write-Host 'Nothing changed.'; exit 1 }

$body = @{
    required_status_checks        = @{ strict = $true; contexts = $checks }
    enforce_admins                = $true
    required_pull_request_reviews = @{ required_approving_review_count = 0; dismiss_stale_reviews = $false }
    restrictions                  = $null
    allow_force_pushes            = $false
    allow_deletions               = $false
    required_linear_history       = $false
    required_conversation_resolution = $false
} | ConvertTo-Json -Depth 5

# UTF-8 without a byte-order mark: PowerShell 5.1's Out-File would add one.
$tmp = [IO.Path]::GetTempFileName()
[IO.File]::WriteAllText($tmp, $body, (New-Object Text.UTF8Encoding($false)))
try {
    gh api -X PUT "repos/$repo/branches/main/protection" --input $tmp | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "setting branch protection failed" }
} finally {
    Remove-Item $tmp -ErrorAction SilentlyContinue
}

gh api -X PATCH "repos/$repo" -F delete_branch_on_merge=true | Out-Null
if ($LASTEXITCODE -ne 0) { throw "turning on delete_branch_on_merge failed" }

Write-Host ""
Write-Host "Now set on main:"
gh api "repos/$repo/branches/main/protection" --jq '"  strict: \(.required_status_checks.strict)\n  enforce_admins: \(.enforce_admins.enabled)\n  force pushes: \(.allow_force_pushes.enabled)\n  deletions: \(.allow_deletions.enabled)\n  required checks:\n" + ([.required_status_checks.contexts[] | "    - " + .] | join("\n"))'
