import {writeFile} from 'node:fs/promises';
import {spawnSync} from 'node:child_process';
import path from 'node:path';
function literal(value) {
  if (!value || /[\r\n"$]/.test(value)) throw new Error('Invalid NSIS metadata or path');
  return value;
}
export function nsisScript({name, version, executable, output, publishUrl, certificateSubjectName}) {
  name = literal(name); version = literal(version); executable = literal(path.resolve(executable)); output = literal(path.resolve(output));
  const exe = literal(path.basename(executable));
  const safe = value => value ? String(value).replace(/[\r\n"$]+/g, '_') : '(unset)';
  return `Unicode true
; TSGPUI NSIS installer for ${name} ${version}
; Signing: tsgpui package --installer nsis --sign with TSGPUI_SIGN_COMMAND (certificateSubjectName ${safe(certificateSubjectName)})
; Publish (manual): upload this installer plus the SHA-256 manifest to publishUrl ${safe(publishUrl)}
; In-app updates are supported: configure autoUpdater.setFeedURL(...) to the published manifest URL.
Name "${name} ${version}"
OutFile "${output}"
InstallDir "$PROGRAMFILES\\${name}"
RequestExecutionLevel admin
Section
  ReadRegStr $0 HKLM "SOFTWARE\\Microsoft\\EdgeUpdate\\Clients\\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}" "pv"
  StrCmp $0 "" checkuser
  StrCmp $0 "0.0.0.0" checkuser runtimeok
checkuser:
  ReadRegStr $0 HKCU "Software\\Microsoft\\EdgeUpdate\\Clients\\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}" "pv"
  StrCmp $0 "" installruntime
  StrCmp $0 "0.0.0.0" installruntime runtimeok
installruntime:
  InitPluginsDir
  inetc::get /SILENT "https://go.microsoft.com/fwlink/p/?LinkId=2124703" "$PLUGINSDIR\\webview2.exe" /END
  Pop $0
  StrCmp $0 "OK" +2
  Abort "Could not download WebView2 runtime"
  ExecWait '"$PLUGINSDIR\\webview2.exe" /silent /install' $0
  IntCmp $0 0 runtimeok
  Abort "WebView2 runtime installation failed"
runtimeok:
  SetOutPath "$INSTDIR"
  File "${executable}"
  CreateDirectory "$SMPROGRAMS\\${name}"
  CreateShortcut "$SMPROGRAMS\\${name}\\${name}.lnk" "$INSTDIR\\${exe}"
  WriteUninstaller "$INSTDIR\\Uninstall.exe"
SectionEnd
Section "Uninstall"
  Delete "$SMPROGRAMS\\${name}\\${name}.lnk"
  RMDir "$SMPROGRAMS\\${name}"
  Delete "$INSTDIR\\${exe}"
  Delete "$INSTDIR\\Uninstall.exe"
  RMDir "$INSTDIR"
SectionEnd
`;
}
export async function buildNsis(options, scriptPath) {
  await writeFile(scriptPath, nsisScript(options));
  const result = spawnSync('makensis', [scriptPath], {encoding:'utf8'});
  if (result.error?.code === 'ENOENT') throw new Error(`makensis is not installed; script written to ${scriptPath}`);
  if (result.error || result.status !== 0) throw new Error(`makensis failed: ${result.stderr || result.stdout || result.error?.message}`);
  return result.stdout;
}
