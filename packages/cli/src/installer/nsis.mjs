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
; TS-GPU NSIS installer for ${name} ${version}
; Signing: tsgpu package --installer nsis --sign with TSGPU_SIGN_COMMAND (certificateSubjectName ${safe(certificateSubjectName)})
; Publish (manual): upload this installer plus the SHA-256 manifest to publishUrl ${safe(publishUrl)}
; In-app updates are supported: configure autoUpdater.setFeedURL(...) to the published manifest URL.
Name "${name} ${version}"
OutFile "${output}"
InstallDir "$PROGRAMFILES\\${name}"
RequestExecutionLevel admin
Section
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
