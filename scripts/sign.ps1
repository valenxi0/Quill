# Signs one file with Azure Artifact Signing (formerly Trusted Signing), so
# Smart App Control and SmartScreen trust Quill. Does nothing unless the
# signing account is configured, so local and fork builds stay unsigned.
#
# Needs these environment variables (repository secrets in CI):
#   ARTIFACT_SIGNING_ENDPOINT   e.g. https://eus.codesigning.azure.net/
#   ARTIFACT_SIGNING_ACCOUNT    the Artifact Signing account name
#   ARTIFACT_SIGNING_PROFILE    the certificate profile name
#   AZURE_TENANT_ID, AZURE_CLIENT_ID, AZURE_CLIENT_SECRET   an app registration
#     with the "Artifact Signing Certificate Profile Signer" role
param([Parameter(Mandatory = $true)][string]$Path)
$ErrorActionPreference = "Stop"

if (-not $env:ARTIFACT_SIGNING_ENDPOINT) {
  Write-Host "Not signing $Path (no ARTIFACT_SIGNING_ENDPOINT)."
  exit 0
}

$tools = Join-Path ([IO.Path]::GetTempPath()) "quill-signing"
$dlib = Get-ChildItem $tools -Recurse -Filter "Azure.CodeSigning.Dlib.dll" -ErrorAction SilentlyContinue |
  Where-Object { $_.FullName -match "\\x64\\" } | Select-Object -First 1
if (-not $dlib) {
  New-Item -ItemType Directory -Force $tools | Out-Null
  $nupkg = Join-Path $tools "client.zip"
  Invoke-WebRequest "https://www.nuget.org/api/v2/package/Microsoft.Trusted.Signing.Client" -OutFile $nupkg
  Expand-Archive $nupkg -DestinationPath (Join-Path $tools "client") -Force
  $dlib = Get-ChildItem $tools -Recurse -Filter "Azure.CodeSigning.Dlib.dll" | Where-Object { $_.FullName -match "\\x64\\" } | Select-Object -First 1
}

$metadata = Join-Path $tools "metadata.json"
@{
  Endpoint = $env:ARTIFACT_SIGNING_ENDPOINT
  CodeSigningAccountName = $env:ARTIFACT_SIGNING_ACCOUNT
  CertificateProfileName = $env:ARTIFACT_SIGNING_PROFILE
} | ConvertTo-Json | Set-Content $metadata -Encoding utf8

$signtool = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin" -Recurse -Filter signtool.exe |
  Where-Object { $_.FullName -match "\\x64\\" } | Sort-Object FullName -Descending | Select-Object -First 1
& $signtool.FullName sign /v /fd SHA256 /tr "http://timestamp.acs.microsoft.com" /td SHA256 /dlib $dlib.FullName /dmdf $metadata $Path
if ($LASTEXITCODE -ne 0) { throw "signtool failed for $Path" }
