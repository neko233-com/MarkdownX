!include "LogicLib.nsh"

!ifndef MDX_DEFAULT_INSTALL_DIR
    !define MDX_DEFAULT_INSTALL_DIR "C:\App\MarkdownX"
!endif
!ifndef MDX_UNINSTALL_KEY
    !define MDX_UNINSTALL_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\com.neko233.markdownx"
!endif

Var MarkdownXInstalledDir

Function CheckMarkdownXUpgradeDirectory
    ClearErrors
    ${If} $MarkdownXInstalledDir != ""
        GetFullPathName $0 "$INSTDIR\."
        GetFullPathName $1 "$MarkdownXInstalledDir\."
        ${If} $0 != $1
            SetErrors
        ${EndIf}
    ${EndIf}
FunctionEnd

!macro customInit
    ReadRegStr $MarkdownXInstalledDir HKCU "${MDX_UNINSTALL_KEY}" "InstallLocation"
    ${If} $MarkdownXInstalledDir == ""
        ; NSIS strips /D= from $CMDLINE before calling .onInit.
        System::Call 'kernel32::GetCommandLineW() w .r0'
        ClearErrors
        ${GetOptions} $0 "/D=" $1
        ${If} ${Errors}
            StrCpy $INSTDIR "${MDX_DEFAULT_INSTALL_DIR}"
        ${EndIf}
        ClearErrors
    ${Else}
        Call CheckMarkdownXUpgradeDirectory
        ${If} ${Errors}
            MessageBox MB_OK|MB_ICONINFORMATION "To change the installation directory, uninstall MarkdownX, then run setup and choose a new directory." /SD IDOK
            SetErrorLevel 2
            Abort
        ${EndIf}
    ${EndIf}
!macroend

Function .onVerifyInstDir
    Call CheckMarkdownXUpgradeDirectory
    ${If} ${Errors}
        Abort
    ${EndIf}
FunctionEnd

!macro customUnInstall
    ; MyGo has already closed the app and unregistered it. Delete only the
    ; files shipped by this package, then return before its recursive RMDir.
    SetOutPath "$TEMP"
    Delete "$INSTDIR\MarkdownX.exe"
    Delete "$INSTDIR\.mygo-files"
    Delete "$INSTDIR\licenses\go-mermaid-LICENSE.txt"
    Delete "$INSTDIR\licenses\go-yaml-LICENSE.txt"
    Delete "$INSTDIR\licenses\go-yaml-NOTICE.txt"
    Delete "$INSTDIR\licenses\gofpdi-LICENSE.txt"
    Delete "$INSTDIR\licenses\gopdf-LICENSE.txt"
    Delete "$INSTDIR\licenses\open-sans-OFL.txt"
    Delete "$INSTDIR\licenses\pkg-errors-LICENSE.txt"
    Delete "$INSTDIR\licenses\termtex-LICENSE.txt"
    Delete "$INSTDIR\Uninstall.exe"
    RMDir "$INSTDIR\licenses"
    RMDir "$INSTDIR"
    Return
!macroend
