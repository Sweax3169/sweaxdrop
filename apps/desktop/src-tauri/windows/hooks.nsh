; SweaxDrop installer hooks.
; The firewall rule is added here, during the elevated install the user
; approved, and removed on uninstall. The app itself never changes
; firewall settings. It allows inbound connections to sweaxdrop.exe only.

!macro NSIS_HOOK_POSTINSTALL
  nsExec::ExecToLog 'netsh advfirewall firewall delete rule name="SweaxDrop"'
  nsExec::ExecToLog 'netsh advfirewall firewall add rule name="SweaxDrop" description="SweaxDrop file transfer (TCP and UDP)" dir=in action=allow program="$INSTDIR\sweaxdrop.exe" enable=yes profile=any'
  ; "Send to" entry for the current user
  SetShellVarContext current
  CreateShortCut "$SENDTO\SweaxDrop.lnk" "$INSTDIR\sweaxdrop.exe" "--send" "$INSTDIR\sweaxdrop.exe" 0
  SetShellVarContext all
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  nsExec::ExecToLog 'netsh advfirewall firewall delete rule name="SweaxDrop"'
  SetShellVarContext current
  Delete "$SENDTO\SweaxDrop.lnk"
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "SweaxDrop"
  SetShellVarContext all
!macroend
