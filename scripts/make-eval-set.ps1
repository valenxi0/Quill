# Builds a reproducible eval set in %LOCALAPPDATA%\Quill\Eval with Windows'
# built-in speech synthesizer: <name>.wav (16 kHz mono PCM) + <name>.txt.
# Synthetic speech is cleaner than a real microphone, so WER here is a floor;
# add your own recordings (same naming) for realistic numbers.
Add-Type -AssemblyName System.Speech
$dir = Join-Path $env:LOCALAPPDATA "Quill\Eval"
New-Item -ItemType Directory -Force $dir | Out-Null

$clips = [ordered]@{
  "01-short"      = "Sounds good, see you soon."
  "02-meeting"    = "Can we move the meeting to Thursday afternoon instead?"
  "03-numbers"    = "The invoice came to twenty five dollars and fifty cents on March third."
  "04-fillers"    = "Um, so I think we should, uh, ship the update on Monday."
  "05-negation"   = "Please do not merge this branch until the tests pass."
  "06-email"      = "Hi Sarah, thanks for the quick reply. I will send the revised draft by Friday morning."
  "07-tech"       = "The build failed because the Vulkan SDK was missing from the path variable."
  "08-question"   = "Did you get a chance to review the pull request I sent yesterday?"
  "09-list"       = "We need three things: a new logo, an updated landing page, and a short launch video."
  "10-time"       = "Let's meet at four thirty pm in the main conference room."
  "11-long"       = "I wanted to give everyone a quick update on the project. The design review went well, and the team agreed on the new navigation. Engineering has started on the settings screen, and we expect a first build next week. The main risk is the audio pipeline on older laptops, so we are adding a lighter engine. If you have feedback, please send it before Wednesday so we can include it in the next sprint. Thanks, everyone, for the hard work this month."
  "12-dictation"  = "Remember to buy milk, eggs, and bread on the way home tonight."
}

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
$synth.Dispose()
Write-Output "Wrote $($clips.Count) clips to $dir"
