@echo off
rem Builds quill_theme.dll (32-bit, like the NSIS installer) into ..\plugins.
rem Needs Visual Studio Build Tools. Run from any prompt.
setlocal
set NSIS=%LOCALAPPDATA%\tauri\NSIS
call "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvarsall.bat" x64_x86 >nul || exit /b 1
cd /d "%~dp0"
if not exist ..\plugins\x86-unicode mkdir ..\plugins\x86-unicode
cl /nologo /O1 /GS- /W3 /LD /MT /DUNICODE /D_UNICODE /I "%NSIS%\Examples\Plugin\nsis" quill_theme.c ^
  /link /OUT:..\plugins\x86-unicode\quill_theme.dll "%NSIS%\Examples\Plugin\nsis\pluginapi-x86-unicode.lib" ^
  /NODEFAULTLIB:LIBC.lib user32.lib gdi32.lib comctl32.lib uxtheme.lib dwmapi.lib || exit /b 1
del /q quill_theme.obj quill_theme.exp quill_theme.lib 2>nul
