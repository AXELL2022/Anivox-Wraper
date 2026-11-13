; Anivox WinUI installer (NSIS 3, Unicode). Built by build-installer.ps1:
;   makensis /DVERSION=X.Y.Z /DSOURCE_DIR=<dotnet publish folder> /DOUTFILE=<setup.exe> anivox.nsi
;
; Per-user install into %LOCALAPPDATA%\Anivox without elevation, like the retired
; Tauri build it replaces. Command line (the same contract as the Tauri NSIS installer,
; whose updater starts this file with "/P /UPDATE /R /ARGS ..."):
;   /P       passive: progress only, no questions, closes itself
;   /S       silent (NSIS built-in)
;   /UPDATE  update run: keep shortcuts as they are, start Anivox afterwards
;   /R       start Anivox afterwards (passive/silent runs)
; The WinUI app itself starts it with "/P /UPDATE".
;
; A previous Tauri install under the same uninstall key is removed with its own
; uninstaller in update mode ("/UPDATE /P _?=<dir>"), which never touches user data.
; Settings, history and logs (%APPDATA% / %LOCALAPPDATA%\com.yasin.anivox) and the
; WebView2 profile are never deleted, also not by this uninstaller.

Unicode true
ManifestDPIAware true
SetCompressor /SOLID lzma
RequestExecutionLevel user

!ifndef VERSION
  !error "VERSION is required (X.Y.Z)"
!endif
!ifndef SOURCE_DIR
  !error "SOURCE_DIR is required (dotnet publish output)"
!endif
!ifndef OUTFILE
  !error "OUTFILE is required"
!endif
; Uninstall key, install folder and shortcut name. Tests pass another value so they
; never touch a real installation.
!ifndef PRODUCT_KEY
  !define PRODUCT_KEY "Anivox"
!endif

!define PRODUCT_NAME "Anivox"
!define PUBLISHER "yasin"
!define MAIN_EXE "anivox-winui.exe"
; Main binary of the retired Tauri build.
!define OLD_EXE "anivox.exe"
!define UNINSTKEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${PRODUCT_KEY}"
; Install location the Tauri installer remembers (Software\<manufacturer>\<product>).
!define OLD_MANUKEY "Software\${PUBLISHER}"
!define OLD_MANUPRODUCTKEY "${OLD_MANUKEY}\${PRODUCT_KEY}"
; Marks the uninstall key as written by this installer (not by Tauri).
!define SHELL_MARK "AnivoxShell"
!define ICON "${__FILEDIR__}\..\..\src-tauri\icons\icon.ico"

Name "${PRODUCT_NAME}"
OutFile "${OUTFILE}"
InstallDir "$LOCALAPPDATA\${PRODUCT_KEY}"
BrandingText " "
ShowInstDetails nevershow
ShowUninstDetails nevershow

VIProductVersion "${VERSION}.0"
VIFileVersion "${VERSION}.0"
VIAddVersionKey "ProductName" "${PRODUCT_NAME}"
VIAddVersionKey "ProductVersion" "${VERSION}"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "FileDescription" "Установщик ${PRODUCT_NAME}"
VIAddVersionKey "CompanyName" "${PUBLISHER}"
VIAddVersionKey "LegalCopyright" "${PUBLISHER}"

!include "MUI2.nsh"
!include "FileFunc.nsh"
!include "LogicLib.nsh"

Var PassiveMode
Var UpdateMode
; A previous install of either build was found.
Var HadInstall

!define MUI_ICON "${ICON}"
!define MUI_UNICON "${ICON}"
!define MUI_ABORTWARNING

!define MUI_PAGE_CUSTOMFUNCTION_PRE SkipIfPassive
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_INSTFILES
!define MUI_PAGE_CUSTOMFUNCTION_PRE SkipIfPassive
!define MUI_FINISHPAGE_RUN "$INSTDIR\${MAIN_EXE}"
!define MUI_FINISHPAGE_RUN_TEXT "Запустить ${PRODUCT_NAME}"
!insertmacro MUI_PAGE_FINISH

!define MUI_PAGE_CUSTOMFUNCTION_PRE un.SkipIfPassive
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

!insertmacro MUI_LANGUAGE "Russian"

; Waits up to 20 s until FILE is no longer running (it can be opened for writing),
; then ends the process by name as a last resort.
!macro WaitForExit FILE NAME
  StrCpy $R8 0
  ${DoWhile} ${FileExists} "${FILE}"
    ClearErrors
    FileOpen $R9 "${FILE}" a
    ${IfNot} ${Errors}
      FileClose $R9
      ${Break}
    ${EndIf}
    ${If} $R8 >= 40
      nsExec::Exec 'taskkill /F /IM "${NAME}"'
      Pop $R9
      Sleep 1000
      ${Break}
    ${EndIf}
    Sleep 500
    IntOp $R8 $R8 + 1
  ${Loop}
!macroend

!macro ParseOptions
  StrCpy $PassiveMode 0
  StrCpy $UpdateMode 0
  ${GetParameters} $R0
  ClearErrors
  ${GetOptions} $R0 "/P" $R1
  ${IfNot} ${Errors}
    StrCpy $PassiveMode 1
  ${EndIf}
  ClearErrors
  ${GetOptions} $R0 "/UPDATE" $R1
  ${IfNot} ${Errors}
    StrCpy $UpdateMode 1
  ${EndIf}
!macroend

; "C:\path" -> C:\path
!macro Unquote VAR
  StrCpy $R9 ${VAR} 1
  ${If} $R9 == '"'
    StrCpy ${VAR} ${VAR} "" 1
    StrCpy $R9 ${VAR} 1 -1
    ${If} $R9 == '"'
      StrCpy ${VAR} ${VAR} -1
    ${EndIf}
  ${EndIf}
!macroend

Function .onInit
  !insertmacro ParseOptions
  ${If} $PassiveMode = 1
    SetAutoClose true
  ${EndIf}
FunctionEnd

Function un.onInit
  !insertmacro ParseOptions
FunctionEnd

Function SkipIfPassive
  ${IfThen} $PassiveMode = 1 ${|} Abort ${|}
FunctionEnd

Function un.SkipIfPassive
  ${IfThen} $PassiveMode = 1 ${|} Abort ${|}
FunctionEnd

; Removes a Tauri install registered under the same uninstall key with its own
; uninstaller in update mode: files and the key go, user data and shortcuts stay
; (the shortcuts are rewritten for the new exe below).
Function RemoveTauriInstall
  ReadRegStr $0 HKCU "${UNINSTKEY}" "${SHELL_MARK}"
  ${If} $0 == "winui"
    Return
  ${EndIf}
  ReadRegStr $1 HKCU "${UNINSTKEY}" "UninstallString"
  ${If} $1 == ""
    Return
  ${EndIf}
  ReadRegStr $2 HKCU "${UNINSTKEY}" "InstallLocation"
  !insertmacro Unquote $2
  ${If} $2 == ""
    StrCpy $2 $1
    !insertmacro Unquote $2
    ${GetParent} $2 $2
  ${EndIf}
  StrCpy $HadInstall 1
  DetailPrint "Удаление прежней версии ${PRODUCT_NAME} ($2)"
  !insertmacro WaitForExit "$2\${OLD_EXE}" "${OLD_EXE}"
  ClearErrors
  ExecWait '$1 /UPDATE /P _?=$2' $3
  ${If} ${Errors}
    DetailPrint "Не удалось запустить удаление прежней версии"
  ${EndIf}
  ; With _?= the old uninstaller cannot delete itself; finish what it left behind.
  Delete "$2\${OLD_EXE}"
  Delete "$2\uninstall.exe"
  ${If} $2 != $INSTDIR
    RMDir "$2"
  ${EndIf}
  DeleteRegKey HKCU "${UNINSTKEY}"
  DeleteRegKey HKCU "${OLD_MANUPRODUCTKEY}"
  DeleteRegKey /ifempty HKCU "${OLD_MANUKEY}"
FunctionEnd

Section "Install"
  StrCpy $HadInstall 0
  ReadRegStr $0 HKCU "${UNINSTKEY}" "${SHELL_MARK}"
  ${If} $0 == "winui"
    StrCpy $HadInstall 1
  ${EndIf}

  !insertmacro WaitForExit "$INSTDIR\${MAIN_EXE}" "${MAIN_EXE}"
  Call RemoveTauriInstall

  ; A previous WinUI install: replace the folder so files dropped by a newer
  ; version do not linger. Only a folder that holds our exe and uninstaller.
  ${If} ${FileExists} "$INSTDIR\${MAIN_EXE}"
  ${AndIf} ${FileExists} "$INSTDIR\uninstall.exe"
    RMDir /r "$INSTDIR"
  ${EndIf}

  SetOutPath "$INSTDIR"
  File /r /x "*.pdb" "${SOURCE_DIR}\*.*"
  WriteUninstaller "$INSTDIR\uninstall.exe"

  ${GetSize} "$INSTDIR" "/S=0K" $0 $1 $2
  IntFmt $0 "0x%08X" $0
  WriteRegStr HKCU "${UNINSTKEY}" "DisplayName" "${PRODUCT_NAME}"
  WriteRegStr HKCU "${UNINSTKEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${UNINSTKEY}" "Publisher" "${PUBLISHER}"
  WriteRegStr HKCU "${UNINSTKEY}" "DisplayIcon" "$\"$INSTDIR\${MAIN_EXE}$\""
  WriteRegStr HKCU "${UNINSTKEY}" "InstallLocation" "$\"$INSTDIR$\""
  WriteRegStr HKCU "${UNINSTKEY}" "UninstallString" "$\"$INSTDIR\uninstall.exe$\""
  WriteRegStr HKCU "${UNINSTKEY}" "QuietUninstallString" "$\"$INSTDIR\uninstall.exe$\" /S"
  WriteRegStr HKCU "${UNINSTKEY}" "URLInfoAbout" "https://github.com/AXELL2022/Anivox-Wraper"
  WriteRegDWORD HKCU "${UNINSTKEY}" "EstimatedSize" "$0"
  WriteRegDWORD HKCU "${UNINSTKEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINSTKEY}" "NoRepair" 1
  WriteRegStr HKCU "${UNINSTKEY}" "${SHELL_MARK}" "winui"

  ; Start menu always; the desktop shortcut on a first install or where one already existed.
  CreateShortcut "$SMPROGRAMS\${PRODUCT_KEY}.lnk" "$INSTDIR\${MAIN_EXE}"
  ${If} $HadInstall = 0
  ${OrIf} ${FileExists} "$DESKTOP\${PRODUCT_KEY}.lnk"
    CreateShortcut "$DESKTOP\${PRODUCT_KEY}.lnk" "$INSTDIR\${MAIN_EXE}"
  ${EndIf}
SectionEnd

Function .onInstSuccess
  ; The finish page offers to start the app in interactive runs.
  ${If} $PassiveMode = 1
  ${OrIf} ${Silent}
    ${GetParameters} $R0
    ClearErrors
    ${GetOptions} $R0 "/R" $R1
    ${If} $UpdateMode = 1
    ${OrIfNot} ${Errors}
      Exec '"$INSTDIR\${MAIN_EXE}"'
    ${EndIf}
  ${EndIf}
FunctionEnd

Section "Uninstall"
  !insertmacro WaitForExit "$INSTDIR\${MAIN_EXE}" "${MAIN_EXE}"
  ; Only a folder that really holds the app is removed.
  ${If} ${FileExists} "$INSTDIR\${MAIN_EXE}"
    RMDir /r /REBOOTOK "$INSTDIR"
  ${Else}
    Delete "$INSTDIR\uninstall.exe"
    RMDir "$INSTDIR"
  ${EndIf}
  ${If} $UpdateMode <> 1
    Delete "$SMPROGRAMS\${PRODUCT_KEY}.lnk"
    Delete "$DESKTOP\${PRODUCT_KEY}.lnk"
  ${EndIf}
  DeleteRegKey HKCU "${UNINSTKEY}"
  ${If} $PassiveMode = 1
    SetAutoClose true
  ${EndIf}
SectionEnd
