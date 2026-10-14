# Installing Quill

Quill lets you type with your voice in any app on Windows 10 or 11. Hold a shortcut, speak, let go, and your words are typed where your cursor is. Everything runs on your own PC.

## 1. Download

Download the Windows installer:

**[Download Quill-Setup.exe](https://github.com/valenxi0/Quill/releases/latest/download/Quill-Setup.exe)**

It always gets the newest version. If the repository is still private, you need to sign in with an account that has access; public downloads do not require a GitHub account.

## 2. Install

Double-click the downloaded file.

- **If Windows shows "Windows protected your PC"** (a blue box): click **More info**, then **Run anyway**. Windows shows this for apps it hasn't seen many people install yet.
- **If Windows says "Smart App Control blocked an app"**, see [Smart App Control](#smart-app-control) below.

The installer doesn't need an admin password. On its last page, leave **Run Quill** ticked, and tick **Create desktop shortcut** if you want one. Then click **Finish**.

## 3. Setup (about a minute)

Quill opens a short setup:

1. **Welcome**: how Quill works.
2. **Your shortcut**: the keys you'll hold to dictate. The default is **Ctrl+Alt+M**. Click it and press new keys to change it. Then choose:
   - **Push to talk**: hold the keys while you talk, and let go to finish.
   - **Toggle**: press once to start, and again to finish.
3. **Choose an engine**: the speech engine downloads once and then works offline. You must download one to continue.
   - **Parakeet + S1-mini** (recommended, 1.15 GB): the best results for English, with automatic clean-up.
   - **Whisper Tiny** (78 MB): the smallest and fastest, for slow connections or older PCs.
   - **Whisper Turbo**: best if you dictate in several languages.
   - **Cohere Transcribe**: experimental.

   You can continue while the download finishes.
4. **Done**: try it.

## 4. Use it

1. Click into any text box: a chat, an email, Word, or Notepad.
2. Hold **Ctrl+Alt+M** (or your shortcut) and speak. A small bar at the bottom of the screen shows Quill is listening.
3. Let go. Your words appear.

Press **Esc twice within one second** to cancel a recording or stop transcription. A single press does not cancel dictation.

Quill starts with Windows and waits in the **tray**: the icons near the clock, bottom right. You may need to click the **^** arrow to see it. Click Quill's icon to open Settings, where you can change the shortcut or engine, add words it should spell your way, and see your stats.

## Updates

Quill checks for updates daily when its GitHub release feed is accessible. In **Settings → Setup → Updates**, check manually or install an available update. You can also download **Quill-Setup.exe** again and run it over your copy. Your settings and downloaded engines are kept.

To hear about new versions, open [the repo](https://github.com/valenxi0/Quill) and click **Watch → Custom → Releases**. GitHub will email you each time one is published. In Quill, **Settings → Setup → Updates → Releases page** opens the same place.

## Troubleshooting

- **Nothing happens when I press the shortcut.** Check that Quill's icon is in the tray. If it isn't, open Quill from the Start menu.
- **The shortcut stays on the default.** Click the shortcut button, hold Ctrl, Alt, Shift or Win, then press another key, such as Space. Modifier keys alone do not save a shortcut. Keep Settings in focus and check for an error below the button. If Quill reports a keyboard hook error, quit it from the tray and reopen it.
- **"Microphone access is off"**: open **Settings → Privacy & security → Microphone** and turn on both **Microphone access** and **Let desktop apps access your microphone**.
- **The text was copied but not pasted.** Some windows, like those run as administrator, don't accept typing from other apps. The text is still on your clipboard, so press **Ctrl+V**.

## Smart App Control

Smart App Control is a Windows 11 security feature that only allows apps from verified publishers.

Quill's releases aren't code-signed yet. Quill has applied to the SignPath Foundation for free signing; see the [code signing policy](CODE-SIGNING.md).

- **If the release is signed** (the release notes say so), Quill installs normally.
- **If it isn't signed yet**, Smart App Control blocks it with no "run anyway" option. You can check whether yours is on under **Windows Security → App & browser control → Smart App Control**.

  If it blocks Quill, you can turn it off under **Windows Security > App & browser control > Smart App Control settings > Off**, then retry installation. This disables that protection for all apps. Updated Windows versions let you turn it back on; older versions may require a Windows reset or reinstall. See [Microsoft's Smart App Control FAQ](https://support.microsoft.com/en-us/windows/security/threat-malware-protection/smart-app-control-frequently-asked-questions).

## Uninstall

**Settings → Apps → Installed apps → Quill → Uninstall.** To also remove the downloaded speech engines (several GB) and your settings, tick **Delete the application data** in the uninstaller.
