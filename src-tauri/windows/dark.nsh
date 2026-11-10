; Quill's dark installer: the Settings window's colors on the stock NSIS wizard.
; Included by installer.nsi before the pages. Welcome and Finish show one
; full-size picture from windows/art; the quill_theme plugin (windows/theme)
; paints the buttons, check boxes, labels and fields of every page.
;
; Needs QUILL_WIN defined as this folder.

!include WinMessages.nsh
!include LogicLib.nsh
!include nsDialogs.nsh

!addplugindir "${QUILL_WIN}\plugins\x86-unicode"
!define QUILL_ART "${QUILL_WIN}\art"

; The Welcome and Finish pages' own ground, under the picture.
!define MUI_BGCOLOR 1C1C1E
!define MUI_TEXTCOLOR F2F2F7

Var QuillDpi

!macro QUILL_DARK_FUNCTIONS UN

; Once the window exists: dark title bar, footer and buttons.
Function ${UN}QuillGuiInit
  quill_theme::Init
  System::Call 'user32::GetDpiForWindow(p$HWNDPARENT)i.s'
  Pop $QuillDpi
  ${If} $QuillDpi < 96
    StrCpy $QuillDpi 96
  ${EndIf}
  Call ${UN}QuillPlaceBranding
FunctionEnd

; Lines the version text up with the Cancel button, at the left margin.
Function ${UN}QuillPlaceBranding
  GetDlgItem $0 $HWNDPARENT 1028
  GetDlgItem $1 $HWNDPARENT 2
  System::Call '*(i,i,i,i)p.r2'
  System::Call 'user32::GetWindowRect(p$1, p$2)'
  System::Call 'user32::MapWindowPoints(p0, p$HWNDPARENT, p$2, i 2)'
  System::Call '*$2(i.r3,i.r4,i.r5,i.r6)'
  System::Call '*(i,i,i,i)p.r7'
  System::Call 'user32::GetClientRect(p$HWNDPARENT, p$7)'
  System::Call '*$7(i,i,i.r8,i)'
  System::Free $2
  System::Free $7
  IntOp $8 $8 - $5 ; the right margin, used on the left
  IntOp $9 $6 - $4 ; button height
  IntOp $1 $QuillDpi * 200
  IntOp $1 $1 / 96
  System::Call 'user32::GetWindowLong(p$0, i -16)i.r2'
  IntOp $2 $2 | 0x200 ; SS_CENTERIMAGE: one line, centered in the box
  System::Call 'user32::SetWindowLong(p$0, i -16, i$2)'
  System::Call 'user32::SetWindowPos(p$0, p0, i$8, i$4, i$1, i$9, i 0x14)'
FunctionEnd

; A middle page: theme what it put on screen, and a sharp header icon.
Function ${UN}QuillDarkPage
  GetDlgItem $0 $HWNDPARENT 1028
  ShowWindow $0 ${SW_SHOW}
  quill_theme::Page
  GetDlgItem $0 $HWNDPARENT 1046
  Push "header"
  Call ${UN}QuillArt
  Pop $1
  ${NSD_SetStretchedImage} $0 $1 $2
FunctionEnd

; The sharpest copy of a picture for this display; it is then stretched to
; fit exactly. Push the name without "@1x.bmp"; pops the path.
Function ${UN}QuillArt
  Exch $R0
  ${If} $QuillDpi > 132
    StrCpy $R0 "$PLUGINSDIR\$R0@2x.bmp"
  ${ElseIf} $QuillDpi > 108
    StrCpy $R0 "$PLUGINSDIR\$R0@1.5x.bmp"
  ${Else}
    StrCpy $R0 "$PLUGINSDIR\$R0@1x.bmp"
  ${EndIf}
  Exch $R0
FunctionEnd

!macroend

!insertmacro QUILL_DARK_FUNCTIONS ""
!insertmacro QUILL_DARK_FUNCTIONS "un."

; Unpacks the header icon. Call from .onInit and un.onInit.
!macro QUILL_UNPACK_HEADER
  InitPluginsDir
  File "/oname=$PLUGINSDIR\header@1x.bmp" "${QUILL_ART}\header@1x.bmp"
  File "/oname=$PLUGINSDIR\header@1.5x.bmp" "${QUILL_ART}\header@1.5x.bmp"
  File "/oname=$PLUGINSDIR\header@2x.bmp" "${QUILL_ART}\header@2x.bmp"
!macroend

; Unpacks the Welcome and Finish pictures and the header. Call from .onInit.
!macro QUILL_UNPACK_ART
  !insertmacro QUILL_UNPACK_HEADER
  File "/oname=$PLUGINSDIR\welcome@1x.bmp" "${QUILL_ART}\welcome@1x.bmp"
  File "/oname=$PLUGINSDIR\welcome@1.5x.bmp" "${QUILL_ART}\welcome@1.5x.bmp"
  File "/oname=$PLUGINSDIR\welcome@2x.bmp" "${QUILL_ART}\welcome@2x.bmp"
  File "/oname=$PLUGINSDIR\finish@1x.bmp" "${QUILL_ART}\finish@1x.bmp"
  File "/oname=$PLUGINSDIR\finish@1.5x.bmp" "${QUILL_ART}\finish@1.5x.bmp"
  File "/oname=$PLUGINSDIR\finish@2x.bmp" "${QUILL_ART}\finish@2x.bmp"
!macroend

; Page callbacks; insert after the MUI pages, whose variables they use.
!macro QUILL_PAGE_FUNCTIONS

; Fills a Welcome or Finish page with one picture and hides its text, which
; is drawn into the picture. Push the page, the picture control, then the
; picture's name.
Function QuillFullPicture
  Exch $R0 ; name
  Exch
  Exch $R1 ; picture control
  Exch 2
  Exch $R2 ; page
  Push $R3
  Push $R4
  System::Call '*(i,i,i,i)p.R4'
  System::Call 'user32::GetClientRect(p$R2, p$R4)'
  System::Call '*$R4(i,i,i.r0,i.r1)'
  System::Free $R4
  System::Call 'user32::SetWindowPos(p$R1, p0, i 0, i 0, i$0, i$1, i 0x14)'
  Push $R0
  Call QuillArt
  Pop $R3
  ${NSD_SetStretchedImage} $R1 $R3 $R4
  StrCpy $R4 0
  ${Do}
    FindWindow $R4 "" "" $R2 $R4
    ${If} $R4 = 0
      ${Break}
    ${EndIf}
    ${If} $R4 <> $R1
      ShowWindow $R4 ${SW_HIDE}
    ${EndIf}
  ${Loop}
  Pop $R4
  Pop $R3
  Pop $R2
  Pop $R0
  Pop $R1
FunctionEnd

Function QuillWelcomeShow
  GetDlgItem $0 $HWNDPARENT 1028
  ShowWindow $0 ${SW_SHOW}
  Push $mui.WelcomePage
  Push $mui.WelcomePage.Image
  Push "welcome"
  Call QuillFullPicture
  quill_theme::Page
FunctionEnd

; Finish: the picture, and "Open Quill" moved down to where the version was.
Function QuillFinishShow
  Push $mui.FinishPage
  Push $mui.FinishPage.Image
  Push "finish"
  Call QuillFullPicture
  ; No desktop shortcut: Quill lives in the tray.
  SendMessage $mui.FinishPage.ShowReadme ${BM_SETCHECK} ${BST_UNCHECKED} 0
  GetDlgItem $0 $HWNDPARENT 1028
  System::Call '*(i,i,i,i)p.r1'
  System::Call 'user32::GetWindowRect(p$0, p$1)'
  System::Call 'user32::MapWindowPoints(p0, p$HWNDPARENT, p$1, i 2)'
  System::Call '*$1(i.r2,i.r3,i.r4,i.r5)'
  System::Free $1
  ShowWindow $0 ${SW_HIDE}
  IntOp $4 $4 - $2
  IntOp $5 $5 - $3
  System::Call 'user32::SetParent(p$mui.FinishPage.Run, p$HWNDPARENT)'
  System::Call 'user32::SetWindowPos(p$mui.FinishPage.Run, p0, i$2, i$3, i$4, i$5, i 0x44)'
  quill_theme::Page
FunctionEnd

!macroend
