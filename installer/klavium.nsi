; Klavium — установщик для Windows (NSIS 3, Unicode).
; Сборка (из корня репозитория):
;   makensis /DVERSION=0.1.0 /DSRCDIR=..\target\release /DOUTFILE=..\dist\KlaviumSetup-0.1.0.exe installer\klavium.nsi
; Установка per-user (без UAC): %LOCALAPPDATA%\Programs\Klavium.

Unicode true
!include "MUI2.nsh"
!include "LogicLib.nsh"
!include "FileFunc.nsh"

!ifndef VERSION
  !define VERSION "0.1.0"
!endif
!ifndef SRCDIR
  !define SRCDIR "..\target\release"
!endif
!ifndef OUTFILE
  !define OUTFILE "..\dist\KlaviumSetup-${VERSION}.exe"
!endif
!ifndef ICONFILE
  !define ICONFILE "..\crates\klavium-app\resources\klavium.ico"
!endif

!define APPNAME "Klavium"
!define PUBLISHER "Klavium"
!define UNINST_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\Klavium"
!define APP_REG_KEY "Software\Klavium"

Name "${APPNAME} ${VERSION}"
BrandingText "${APPNAME} ${VERSION}"
OutFile "${OUTFILE}"
RequestExecutionLevel user
InstallDir "$LOCALAPPDATA\Programs\Klavium"
InstallDirRegKey HKCU "${APP_REG_KEY}" "InstallDir"
SetCompressor /SOLID lzma
ShowInstDetails show
ShowUninstDetails show

VIProductVersion "${VERSION}.0"
VIAddVersionKey "ProductName" "${APPNAME}"
VIAddVersionKey "FileDescription" "Установщик ${APPNAME} ${VERSION}"
VIAddVersionKey "FileVersion" "${VERSION}.0"
VIAddVersionKey "ProductVersion" "${VERSION}.0"
VIAddVersionKey "CompanyName" "${PUBLISHER}"
VIAddVersionKey "LegalCopyright" "© ${PUBLISHER}"

!define MUI_ICON "${ICONFILE}"
!define MUI_UNICON "${ICONFILE}"
!define MUI_ABORTWARNING

!define MUI_WELCOMEPAGE_TITLE "Установка ${APPNAME} ${VERSION}"
!define MUI_WELCOMEPAGE_TEXT "Klavium — мгновенное исправление раскладки клавиатуры и голосовой ввод.$\r$\n$\r$\nУстановка для текущего пользователя (без прав администратора): программа разместится в папке «${APPNAME}» в вашем профиле, данные (настройки, модели, статистика) — в подпапке data."
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_COMPONENTS
!insertmacro MUI_PAGE_INSTFILES
!define MUI_FINISHPAGE_RUN "$INSTDIR\klavium.exe"
!define MUI_FINISHPAGE_RUN_TEXT "Запустить Klavium"
!insertmacro MUI_PAGE_FINISH

!define MUI_UNPAGE_CONFIRM
!define MUI_UNPAGE_INSTFILES
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

!insertmacro MUI_LANGUAGE "Russian"
!insertmacro MUI_LANGUAGE "English"

; ---------- Установка ----------

Section "Klavium (обязательно)" SEC_MAIN
  SectionIn RO
  SetOutPath "$INSTDIR"

  ; Закрыть запущенный экземпляр, иначе файл занят.
  DetailPrint "Закрываю запущенный экземпляр Klavium (если есть)…"
  nsExec::ExecToLog 'taskkill /IM klavium.exe /F'
  Sleep 600

  File "${SRCDIR}\klavium.exe"

  ; Данные — в data\ рядом с exe (портативная схема приложения).
  CreateDirectory "$INSTDIR\data"

  ; Uninstaller и записи «Установка и удаление программ».
  WriteUninstaller "$INSTDIR\uninstall.exe"
  WriteRegStr HKCU "${APP_REG_KEY}" "InstallDir" "$INSTDIR"
  WriteRegStr HKCU "${UNINST_KEY}" "DisplayName" "${APPNAME}"
  WriteRegStr HKCU "${UNINST_KEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${UNINST_KEY}" "Publisher" "${PUBLISHER}"
  WriteRegStr HKCU "${UNINST_KEY}" "DisplayIcon" "$INSTDIR\klavium.exe"
  WriteRegStr HKCU "${UNINST_KEY}" "UninstallString" '"$INSTDIR\uninstall.exe"'
  WriteRegStr HKCU "${UNINST_KEY}" "QuietUninstallString" '"$INSTDIR\uninstall.exe" /S'
  WriteRegDWORD HKCU "${UNINST_KEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINST_KEY}" "NoRepair" 1
  ${GetSize} "$INSTDIR" "/S=0K" $0 $1 $2
  IntFmt $0 "0x%08X" $0
  WriteRegDWORD HKCU "${UNINST_KEY}" "EstimatedSize" "$0"

  ; Ярлыки: меню «Пуск».
  CreateDirectory "$SMPROGRAMS\${APPNAME}"
  CreateShortCut "$SMPROGRAMS\${APPNAME}\${APPNAME}.lnk" "$INSTDIR\klavium.exe"
  CreateShortCut "$SMPROGRAMS\${APPNAME}\Удалить ${APPNAME}.lnk" "$INSTDIR\uninstall.exe"
SectionEnd

Section "Автозапуск вместе с Windows" SEC_AUTOSTART
  ; Приложение само управляет записью автозапуска по флагу в config.json;
  ; на свежей установке включаем флаг (существующие настройки не трогаем).
  IfFileExists "$INSTDIR\data\config.json" autostart_skip
    FileOpen $0 "$INSTDIR\data\config.json" w
    FileWrite $0 '{"autostart":{"enabled":true}}'
    FileClose $0
    DetailPrint "Автозапуск включён (запись будет создана при первом запуске)."
    Goto autostart_done
  autostart_skip:
    DetailPrint "config.json уже существует — настройка автозапуска не изменяется."
  autostart_done:
SectionEnd

Section "Ярлык на рабочем столе" SEC_DESKTOP
  CreateShortCut "$DESKTOP\${APPNAME}.lnk" "$INSTDIR\klavium.exe"
SectionEnd

LangString DESC_SEC_MAIN ${LANG_RUSSIAN} "Приложение Klavium (обязательно)."
LangString DESC_SEC_MAIN ${LANG_ENGLISH} "Klavium application (required)."
LangString DESC_SEC_AUTOSTART ${LANG_RUSSIAN} "Запускать Klavium при входе в Windows (тихо, в трее)."
LangString DESC_SEC_AUTOSTART ${LANG_ENGLISH} "Start Klavium at Windows logon (silent, in tray)."
LangString DESC_SEC_DESKTOP ${LANG_RUSSIAN} "Ярлык Klavium на рабочем столе."
LangString DESC_SEC_DESKTOP ${LANG_ENGLISH} "Klavium desktop shortcut."

!insertmacro MUI_FUNCTION_DESCRIPTION_BEGIN
  !insertmacro MUI_DESCRIPTION_TEXT ${SEC_MAIN} $(DESC_SEC_MAIN)
  !insertmacro MUI_DESCRIPTION_TEXT ${SEC_AUTOSTART} $(DESC_SEC_AUTOSTART)
  !insertmacro MUI_DESCRIPTION_TEXT ${SEC_DESKTOP} $(DESC_SEC_DESKTOP)
!insertmacro MUI_FUNCTION_DESCRIPTION_END

; ---------- Удаление ----------

Section "Uninstall"
  ; Закрыть приложение (в т.ч. тихую копию из автозапуска).
  nsExec::ExecToLog 'taskkill /IM klavium.exe /F'
  Sleep 600

  Delete "$INSTDIR\klavium.exe"
  Delete "$INSTDIR\uninstall.exe"

  ; Спросить про данные (настройки, модели, статистика).
  IfFileExists "$INSTDIR\data\*.*" 0 data_done
    IfSilent data_keep
    MessageBox MB_YESNO|MB_ICONQUESTION "Удалить данные Klavium (настройки, статистика, скачанные модели)?" IDYES data_remove IDNO data_keep
    data_remove:
      RMDir /r "$INSTDIR\data"
      DetailPrint "Данные удалены."
      Goto data_done
    data_keep:
      DetailPrint "Данные сохранены в $INSTDIR\data."
  data_done:

  Delete "$SMPROGRAMS\${APPNAME}\${APPNAME}.lnk"
  Delete "$SMPROGRAMS\${APPNAME}\Удалить ${APPNAME}.lnk"
  RMDir "$SMPROGRAMS\${APPNAME}"
  Delete "$DESKTOP\${APPNAME}.lnk"

  ; Запись автозапуска (её пишет само приложение).
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "${APPNAME}"

  DeleteRegKey HKCU "${UNINST_KEY}"
  DeleteRegKey HKCU "${APP_REG_KEY}"

  RMDir "$INSTDIR"
SectionEnd
