; The Windows installer replaces programs that are running: the agent service
; and the tray icon. It stops them first and starts the service again after.

!macro NSIS_HOOK_PREINSTALL
  nsExec::Exec 'sc.exe stop BancAgent'
  Sleep 3000
  nsExec::Exec 'taskkill.exe /F /IM banc-session.exe'
!macroend

!macro NSIS_HOOK_POSTINSTALL
  nsExec::Exec 'sc.exe start BancAgent'
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  nsExec::Exec 'sc.exe stop BancAgent'
  Sleep 3000
  nsExec::Exec 'sc.exe delete BancAgent'
  nsExec::Exec 'taskkill.exe /F /IM banc-session.exe'
  DeleteRegValue HKLM "Software\Microsoft\Windows\CurrentVersion\Run" "BancSession"
!macroend
