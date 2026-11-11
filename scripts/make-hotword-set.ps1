# Builds rare-term clips for the hotword experiment in
# %LOCALAPPDATA%\Quill\EvalTerms with Windows' speech synthesizer:
# <name>.wav (16 kHz mono PCM), <name>.txt, and terms.txt (one term per line).
# Run with `quill-bench --hotwords %LOCALAPPDATA%\Quill\EvalTerms`.
Add-Type -AssemblyName System.Speech
$dir = Join-Path $env:LOCALAPPDATA "Quill\EvalTerms"
New-Item -ItemType Directory -Force $dir | Out-Null

$clips = [ordered]@{
  "t01-siobhan"   = "Please ask Siobhan to send the slides."
  "t02-zendrix"   = "Our new CRM is called Zendrix and it launches Monday."
  "t03-okonkwo"   = "Book a call with Doctor Okonkwo for Tuesday."
  "t04-valkey"    = "We switched the cache to Valkey last week."
  "t05-wojciech"  = "Send the contract to Wojciech before noon."
  "t06-metoprolol"= "The patient takes metoprolol twice a day."
  "t07-quillbox"  = "Deploy the Quillbox service to staging."
  "t08-niamh"     = "Niamh will review the Tauri update."
  "t09-zustand"   = "Add Zustand to the frontend dependencies."
  "t10-dziedzic"  = "Ask Dziedzic about the Grafana dashboard."
  "t11-saoirse"   = "Saoirse moved the files to Nextcloud."
  "t12-ozempic"   = "Order more Ozempic for the clinic."
  "t13-hetzner"   = "The Hetzner server needs a reboot."
  "t14-thandiwe"  = "Ping Thandiwe about the Vitest failures."
  "t15-lumivoro"  = "Lumivoro is our new design system."
  "t16-pydantic"  = "Run the Pydantic migration before Friday."
  "t17-krzysztof" = "Krzysztof wrote the Tailscale config."
  "t18-atorva"    = "Refill the atorvastatin prescription for Mrs Nguyen."
  "t19-supabase"  = "Move the auth tables from Supabase to Postgres."
  "t20-kestrelio" = "The Kestrelio dashboard shows the new signups."
}
$terms = @(
  "Siobhan", "Zendrix", "Okonkwo", "Valkey", "Wojciech", "metoprolol", "Quillbox", "Niamh", "Tauri",
  "Zustand", "Dziedzic", "Grafana", "Saoirse", "Nextcloud", "Ozempic", "Hetzner", "Thandiwe", "Vitest",
  "Lumivoro", "Pydantic", "Krzysztof", "Tailscale", "atorvastatin", "Nguyen", "Supabase", "Kestrelio"
)

$synth = New-Object System.Speech.Synthesis.SpeechSynthesizer
$synth.Rate = 0
$format = New-Object System.Speech.AudioFormat.SpeechAudioFormatInfo(16000, [System.Speech.AudioFormat.AudioBitsPerSample]::Sixteen, [System.Speech.AudioFormat.AudioChannel]::Mono)
foreach ($name in $clips.Keys) {
  $wav = Join-Path $dir "$name.wav"
  $synth.SetOutputToWaveFile($wav, $format)
  $synth.Speak($clips[$name])
  $synth.SetOutputToNull()
  Set-Content -Path (Join-Path $dir "$name.txt") -Value $clips[$name] -Encoding utf8
}
Set-Content -Path (Join-Path $dir "terms.txt") -Value $terms -Encoding utf8
Write-Output "Wrote $($clips.Count) clips and $($terms.Count) terms to $dir"

# Long pauses between sentences, in EvalPauses: sherpa-onnx 1.13.8's TDT beam
# search can drop speech after a pause (k2-fsa/sherpa-onnx#4084).
$pauseDir = Join-Path $env:LOCALAPPDATA "Quill\EvalPauses"
New-Item -ItemType Directory -Force $pauseDir | Out-Null
$pauses = [ordered]@{
  "p01-two"   = @("Okay, let me think about this.", "We should ask Siobhan before we ship it.")
  "p02-three" = @("First item is the budget.", "Second, the Grafana dashboard is down.", "Third, lunch is at noon.")
  "p03-trail" = @("Send the notes to the team.", "Thanks.")
  "p04-start" = @("Hmm.", "The Hetzner invoice came in this morning and it looks higher than last month.")
}
foreach ($name in $pauses.Keys) {
  $wav = Join-Path $pauseDir "$name.wav"
  $prompt = New-Object System.Speech.Synthesis.PromptBuilder
  foreach ($sentence in $pauses[$name]) {
    $prompt.AppendText($sentence)
    $prompt.AppendBreak([TimeSpan]::FromSeconds(2.5))
  }
  $synth.SetOutputToWaveFile($wav, $format)
  $synth.Speak($prompt)
  $synth.SetOutputToNull()
  Set-Content -Path (Join-Path $pauseDir "$name.txt") -Value ($pauses[$name] -join " ") -Encoding utf8
}
Set-Content -Path (Join-Path $pauseDir "terms.txt") -Value $terms -Encoding utf8
$synth.Dispose()
Write-Output "Wrote $($pauses.Count) pause clips to $pauseDir"
