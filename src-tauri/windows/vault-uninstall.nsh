; Runs the Vault wizard for interactive EXE removal before any application files are removed.
!macro NSIS_HOOK_PREUNINSTALL
  ${IfNot} ${Silent}
  ${AndIf} $PassiveMode <> 1
  ${AndIf} $UpdateMode <> 1
    ClearErrors
    ExecWait '"$INSTDIR\${MAINBINARYNAME}.exe" --vault-uninstall-nsis' $0
    ; Only the explicit Continue action authorizes the waiting uninstaller.
    ${If} ${Errors}
    ${OrIf} $0 <> 42
      SetErrorLevel 1
      Abort
    ${EndIf}
  ${EndIf}
!macroend
