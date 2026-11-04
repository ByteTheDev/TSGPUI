import ts from 'typescript';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import {spawnSync} from 'node:child_process';
import {readFile, writeFile, mkdir, cp, stat, readdir} from 'node:fs/promises';
import {bundleBackend} from './backend.mjs';

const runtimeDir = path.join(path.dirname(fileURLToPath(import.meta.url)), 'electron-compat', 'runtime');
const builtins = new Map(['electron','fs','fs/promises','path','os','events','child_process','process','https'].flatMap(n => [[n,n === 'fs/promises' ? 'fs' : n],['node:'+n,n === 'fs/promises' ? 'fs' : n]]));
const slash = p => p.replaceAll('\\','/');
const exists = async p => !!(await stat(p).catch(() => null));
const inside = (root,p) => p === root || p.startsWith(root + path.sep);
const literal = n => n && (ts.isStringLiteralLike(n) || ts.isNoSubstitutionTemplateLiteral(n)) ? n.text : undefined;
const relImport = (from,to) => { let p = slash(path.relative(path.dirname(from),to)).replace(/\.ts$/, ''); return p.startsWith('.') ? p : './'+p; };

export async function importElectron(input, {out, build = false, force = false} = {}) {
  const root = path.resolve(input), dest = path.resolve(out || root + '-tsgpui');
  if (inside(root,dest) || inside(dest,root)) throw new Error('Output and source directories must not contain one another');
  const pkg = JSON.parse(await readFile(path.join(root,'package.json'),'utf8'));
  if (await exists(dest) && !force) throw new Error(`Output already exists: ${dest}; pass --force to reuse it`);
  if (build) {
    if (!pkg.scripts?.build) throw new Error('No build script found in package.json');
    const bun = await exists(path.join(root,'bun.lock')) || await exists(path.join(root,'bun.lockb'));
    const cmd = bun ? 'bun' : 'npm';
    console.log(`Running ${cmd} run build in ${root}`);
    const found = process.platform === 'win32' ? spawnSync('where.exe',[cmd === 'npm' ? 'npm.cmd' : 'bun.exe'],{encoding:'utf8'}).stdout?.split(/\r?\n/).find(Boolean) : null;
    const executable = process.platform === 'win32' && cmd === 'npm' ? path.join(path.dirname(found || process.execPath),'node_modules','npm','bin','npm-cli.js') : (found || cmd);
    const result = spawnSync(process.platform === 'win32' && cmd === 'npm' ? process.execPath : executable, process.platform === 'win32' && cmd === 'npm' ? [executable,'run','build'] : ['run','build'], {cwd:root, stdio:'inherit'});
    if (result.error || result.status !== 0) throw new Error(`Build script failed: ${result.error?.message || result.status}`);
  }
  const main = path.resolve(root,pkg.main || 'index.js');
  if (!inside(root,main) || !await exists(main)) throw new Error(`Main process entry not found: ${pkg.main || 'index.js'}`);
  const notes = [], rewrites = [], visited = new Set(), modules = new Map(), hosts = new Set(), commands = new Set();
  const usage = {fs:false, relativeFs:false, window:false, notifications:false, ipc:false};
  let renderer, preload, width=900, height=600, title, titleBarStyle;
  const sourceDir = path.join(dest,'src'), mainDir = path.join(sourceDir,'main');
  const warn = (file,node,ast,msg) => notes.push(`${slash(path.relative(root,file))}:${ast.getLineAndCharacterOfPosition(node.getStart(ast)).line+1}: ${msg}`);
  async function resolveLocal(file,spec) {
    const base=path.resolve(path.dirname(file),spec);
    if (!inside(root,base)) return null;
    for (const p of [base,base+'.js',base+'.ts',base+'.mjs',base+'.cjs',path.join(base,'index.js'),path.join(base,'index.ts')]) if ((await stat(p).catch(()=>null))?.isFile()) return p;
    return null;
  }
  const targetFor = file => path.join(mainDir, path.relative(root,file).replace(/\.(?:mjs|cjs|js|jsx|tsx)$/i,'.ts'));
  async function visit(file) {
    if (visited.has(file)) return;
    visited.add(file);
    const source=await readFile(file,'utf8'), ast=ts.createSourceFile(file,source,ts.ScriptTarget.Latest,true, /\.tsx?$/.test(file) ? ts.ScriptKind.TS : ts.ScriptKind.JS);
    const edits=[], dependencies=[], imports=[];
    const staticPath = n => {
      const v=literal(n); if(v !== undefined) return v;
      if(ts.isIdentifier(n) && n.text==='__dirname') return path.dirname(file);
      if(ts.isCallExpression(n) && /^(?:path\.)?(?:join|resolve)$/.test(n.expression.getText(ast))) {
        const parts=n.arguments.map(staticPath); if(parts.every(p=>p !== undefined)) return path.resolve(path.dirname(file),...parts);
      }
    };
    for (const d of ast.parseDiagnostics) warn(file,ast,ast,`Parse error: ${ts.flattenDiagnosticMessageText(d.messageText,' ')}`);
    function scan(node) {
      if (ts.isNewExpression(node) && /(?:^|\.)BrowserWindow$/.test(node.expression.getText(ast))) {
        usage.window=true;
        const obj=node.arguments?.[0];
        if (obj && ts.isObjectLiteralExpression(obj)) for (const prop of obj.properties) if (ts.isPropertyAssignment(prop)) {
          const key=prop.name.getText(ast).replace(/^['"]|['"]$/g,'');
          if (key==='width' || key==='height') { const n=Number(prop.initializer.getText(ast)); if (Number.isInteger(n) && n>=200 && n<=4096) {if(key==='width') width=n; else height=n;} else warn(file,prop,ast,`Dynamic or invalid window ${key}`); }
          if (key==='title') title=literal(prop.initializer);
          if (key==='titleBarStyle') titleBarStyle=literal(prop.initializer);
          if (key==='webPreferences' && ts.isObjectLiteralExpression(prop.initializer)) for (const p of prop.initializer.properties) if (ts.isPropertyAssignment(p) && p.name.getText(ast)==='preload') {
            const value=staticPath(p.initializer);
            if(value !== undefined) preload=path.resolve(path.dirname(file),value); else warn(file,p,ast,'Dynamic preload path requires manual migration');
          }
        }
      }
      if (ts.isCallExpression(node)) {
        const expr=node.expression.getText(ast), arg=literal(node.arguments[0]);
        if (/\.loadFile$/.test(expr)) { const value=node.arguments[0] && staticPath(node.arguments[0]); if(value) renderer=path.resolve(root,value); else warn(file,node,ast,'Dynamic loadFile target requires manual migration'); }
        if (/\.loadURL$/.test(expr)) { if(arg?.startsWith('file:')) { try {renderer=fileURLToPath(arg);} catch {warn(file,node,ast,'Invalid loadURL file target');} } else warn(file,node,ast,`loadURL target ${arg || '(dynamic)'} requires a local renderer build`); }
        if (/^(?:fetch|axios(?:\.[\w]+)?|https\.(?:get|request))$/.test(expr)) {
          const addHost = (value, isUrl) => { try { const u = new URL(isUrl ? value : 'https://'+value); hosts.add(u.hostname.toLowerCase()); } catch { warn(file,node,ast,`Network value requires review: ${value}`); } };
          const first=node.arguments[0];
          if(arg !== undefined) addHost(arg,true);
          else if(first && ts.isObjectLiteralExpression(first)) {
            const props=first.properties.filter(p=>ts.isPropertyAssignment(p) && ['hostname','host','url'].includes(p.name.getText(ast).replace(/^['"]|['"]$/g,'')));
            if(!props.length) warn(file,node,ast,'Network options lack a literal host; review');
            for(const p of props) { const v=literal(p.initializer); if(v === undefined) warn(file,p,ast,'Dynamic network URL requires review'); else addHost(v,p.name.getText(ast).replace(/^['"]|['"]$/g,'')==='url'); }
          } else warn(file,node,ast,'Dynamic network URL requires review');
        }
        if (/(?:^|\.)(?:spawn|execFile|exec|fork|spawnSync|execFileSync|execSync)$/.test(expr)) { if(arg && /^[\w.-]+$/.test(arg)) commands.add(arg); else warn(file,node,ast,'Dynamic or shell process command requires review'); }
        if (/^(?:fs|filesystem)(?:\.promises)?\./.test(expr)) { usage.fs=true; if(arg !== undefined && !path.isAbsolute(arg)) { usage.relativeFs=true; warn(file,node,ast,`Literal relative filesystem path ${arg} requires review; granting ./data placeholder`); } else warn(file,node,ast,'Filesystem path requires review; userData and dialog-selected files are auto-granted'); }
        if (/^(?:ipcMain|ipcRenderer)\./.test(expr)) usage.ipc=true;
        if (/^(?:dialog|shell|clipboard|app|BrowserWindow|Notification)\./.test(expr) && !/^(?:app\.(?:whenReady|on|quit|getPath|getName|getVersion)|dialog\.(?:showOpenDialog|showSaveDialog|showMessageBox)|shell\.openExternal|clipboard\.(?:readText|writeText)|BrowserWindow\.(?:getAllWindows|fromWebContents)|Notification\.isSupported)$/.test(expr)) warn(file,node,ast,`Review potentially unsupported API ${expr}`);
      }
      if (ts.isNewExpression(node) && /(?:^|\.)Notification$/.test(node.expression.getText(ast))) usage.notifications=true;
      let spec, specNode, isRequire=false;
      if ((ts.isImportDeclaration(node) || ts.isExportDeclaration(node)) && node.moduleSpecifier && ts.isStringLiteral(node.moduleSpecifier)) {spec=node.moduleSpecifier.text; specNode=node.moduleSpecifier;}
      if (ts.isCallExpression(node) && ts.isIdentifier(node.expression) && node.expression.text==='require') {isRequire=true; spec=literal(node.arguments[0]); specNode=node.arguments[0]; if(!spec) { warn(file,node,ast,'Dynamic require is unsupported'); edits.push([node.getStart(ast),node.end,'(() => { throw new Error("Dynamic require is unsupported"); })()']); }}
      if (spec) dependencies.push({spec,specNode,node,isRequire});
      ts.forEachChild(node,scan);
    }
    scan(ast);
    for (const dep of dependencies) {
      let target, kind=builtins.get(dep.spec);
      if (kind) {target=path.join(sourceDir,'tsgpui-runtime',kind+'.ts'); if(kind==='fs') usage.fs=true;}
      else if (dep.spec.startsWith('.')) {const resolved=await resolveLocal(file,dep.spec); if(resolved) {target=targetFor(resolved); await visit(resolved);} else warn(file,dep.node,ast,`Unresolved module ${dep.spec}`);}
      else warn(file,dep.node,ast,`Unsupported module ${dep.spec}`);
      if (!target) {target=path.join(sourceDir,'unsupported.ts');}
      const replacement=relImport(targetFor(file),target);
      rewrites.push(`${slash(path.relative(root,file))}:${ast.getLineAndCharacterOfPosition(dep.node.getStart(ast)).line+1}: ${dep.spec} → ${replacement}`);
      if(dep.isRequire) {const alias=`__tsgpui_import_${imports.length}`; imports.push(`import * as ${alias} from ${JSON.stringify(replacement)};`); edits.push([dep.node.getStart(ast),dep.node.end,alias]);}
      else edits.push([dep.specNode.getStart(ast),dep.specNode.end,JSON.stringify(replacement)]);
    }
    let converted=source;
    for (const [start,end,value] of edits.sort((a,b)=>b[0]-a[0])) converted=converted.slice(0,start)+value+converted.slice(end);
    // Node gives each module its own __dirname and __filename. The WebView backend has no file system
    // layout, so define them per module as virtual paths that mirror the original project layout.
    const virtualFile='/'+slash(path.relative(root,file));
    const definitions=[];
    if(/\b__dirname\b/.test(source)) definitions.push(`const __dirname = ${JSON.stringify(path.posix.dirname(virtualFile))};`);
    if(/\b__filename\b/.test(source)) definitions.push(`const __filename = ${JSON.stringify(virtualFile)};`);
    modules.set(file,imports.join('\n')+'\n'+(definitions.length ? definitions.join('\n')+'\n' : '')+converted);
  }
  await visit(main);
  if (!renderer) for (const candidate of ['dist/index.html','build/index.html','out/index.html']) if(await exists(path.join(root,candidate))) { renderer=path.join(root,candidate); notes.push(`Renderer inferred from ${candidate}; verify the selected entry.`); break; }
  if (!renderer || !inside(root,renderer) || !await exists(renderer)) throw new Error('No built renderer entry found; build the Electron app first or pass --build');
  if (preload) {if(inside(root,preload) && await exists(preload)) await visit(preload); else notes.push(`Preload ${preload} was not found; migrate it manually.`);}
  await mkdir(sourceDir,{recursive:true});
  const rendererRoot=path.dirname(renderer);
  await cp(rendererRoot,path.join(sourceDir,'renderer'),{recursive:true,filter:p=>!inside(dest,path.resolve(p))});
  const htmlSource=await readFile(renderer,'utf8');
  const stylesheets=[];
  for (const match of htmlSource.replace(/<!--[\s\S]*?-->/g,'').matchAll(/<link\b[^>]*>/gi)) {
    const attrs=Object.fromEntries([...match[0].matchAll(/([^\s=<>"']+)\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s>]+))/g)].map(m=>[m[1].toLowerCase(),m[2] ?? m[3] ?? m[4]]));
    if (!attrs.rel?.split(/\s+/).some(x=>x.toLowerCase()==='stylesheet') || !attrs.href || /^(?:[a-z][a-z\d+.-]*:|\/\/|\/)/i.test(attrs.href)) continue;
    let relative;
    try { relative=decodeURIComponent(attrs.href.split(/[?#]/)[0]); } catch { continue; }
    const file=path.resolve(rendererRoot,relative);
    if (inside(rendererRoot,file) && (await stat(file).catch(()=>null))?.isFile()) {
      const target='src/renderer/'+slash(path.relative(rendererRoot,file));
      if (!stylesheets.includes(target)) stylesheets.push(target);
    }
  }
  async function reviewRenderer(dir) {
    for(const item of await readdir(dir,{withFileTypes:true})) {
      const p=path.join(dir,item.name); if(item.isDirectory()) {await reviewRenderer(p); continue;}
      if(!item.isFile() || !/\.(?:js|mjs|cjs|html)$/.test(item.name)) continue;
      const content=await readFile(p,'utf8');
      for(const match of content.matchAll(/\b(?:require\s*\(\s*['"](?:electron|(?:node:)?(?:fs|path|os|events|child_process|process))|from\s*['"](?:electron|(?:node:)?(?:fs|path|os|events|child_process|process)))/g)) {
        const line=content.slice(0,match.index).split('\n').length;
        notes.push(`${slash(path.relative(root,p))}:${line}: Renderer Node/Electron import requires migration or bundling for the WebView.`);
      }
    }
  }
  await reviewRenderer(rendererRoot);
  for (const [file,code] of modules) {const target=targetFor(file); await mkdir(path.dirname(target),{recursive:true}); await writeFile(target,code);}
  await writeFile(path.join(sourceDir,'unsupported.ts'),'export default {};\n');
  await mkdir(path.join(sourceDir,'tsgpui-runtime'),{recursive:true});
  const runtimeFiles=await readdir(runtimeDir).catch(()=>[]);
  for (const file of runtimeFiles.filter(f=>f.endsWith('.ts'))) await cp(path.join(runtimeDir,file),path.join(sourceDir,'tsgpui-runtime',file));
  for(const file of ['electron.ts','path.ts','fs.ts','os.ts','events.ts','child_process.ts','process.ts','host.ts']) if(!runtimeFiles.includes(file)) notes.push(`Compatibility runtime missing ${file}; copy it into src/tsgpui-runtime before building.`);
  await writeFile(path.join(sourceDir,'backend.ts'),`import ${JSON.stringify(relImport(path.join(sourceDir,'backend.ts'),targetFor(main)))};\nexport default { state: {}, actions: {} };\n`);
  let preloadNote='No preload was detected.';
  if(preload && modules.has(preload)) {
    const entry=path.join(sourceDir,'preload-entry.ts');
    await writeFile(entry,`import ${JSON.stringify(relImport(entry,targetFor(preload)))};\nexport default { state: {}, actions: {} };\n`);
    try {
      const bundled=await bundleBackend(entry,dest,async (base,file)=>{if(!inside(base,file)) throw new Error('Module outside output');});
      const htmlFile=path.join(sourceDir,'renderer',path.basename(renderer));
      let html=await readFile(htmlFile,'utf8');
      const script=`<script data-tsgpui-preload>\n(()=>{const previous=globalThis.__tsgpui_app;${bundled.js.replace(/<\/script/gi,'<\\/script')}globalThis.__tsgpui_app=previous;})();\n</script>\n`;
      const match=/<script\b/i.exec(html); html=match ? html.slice(0,match.index)+script+html.slice(match.index) : html.replace(/<\/head\s*>/i,script+'</head>');
      if(!html.includes('data-tsgpui-preload')) html=script+html;
      await writeFile(htmlFile,html);
      preloadNote='Preload is bundled into an inline script before the first renderer script; review Electron isolation and preload assumptions.';
    } catch(e) {preloadNote=`Preload could not be bundled: ${e.message}. Migrate it manually.`;}
  }
  const name=(pkg.name || 'ImportedApp').replace(/[^a-zA-Z0-9_-]/g,'-').replace(/^[^a-zA-Z]+/,'') .slice(0,64) || 'ImportedApp';
  const version=/^\d{1,4}\.\d{1,4}\.\d{1,4}$/.test(pkg.version || '') ? pkg.version : '1.0.0';
  if(version!==pkg.version) notes.push(`Package version ${pkg.version || '(missing)'} was normalized to ${version}.`);
  const permissions={filesystem:{read:usage.relativeFs?['./data']:[],write:usage.relativeFs?['./data']:[]},processes:[...commands].sort(),network:[...hosts].sort(),notifications:usage.notifications,window:usage.window,ipc:usage.ipc};
  const css=stylesheets[0] || 'src/import.css';
  if (!stylesheets.length) await writeFile(path.join(sourceDir,'import.css'),'');
  const config={name,title:title || pkg.productName || pkg.name || name,version,renderer:'webview',window:{width,height},html:`src/renderer/${path.basename(renderer)}`,css,backend:'src/backend.ts',permissions};
  await writeFile(path.join(dest,'tsgpui.config.json'),JSON.stringify(config,null,2)+'\n');
  const report=`# Electron import report\n\nSource: ${root}\n\n## Stylesheets\n\nConfig css: ${css}${stylesheets.length ? '' : ' (empty placeholder because no linked local stylesheet was found)'}.\n\nCopied linked stylesheets:\n${stylesheets.map(x=>'- '+x).join('\n') || '- None'}\n\n## Rewritten specifiers\n\n${rewrites.map(x=>'- '+x).join('\n') || '- None'}\n\n## Unsupported and review items\n\n${notes.map(x=>'- '+x).join('\n') || '- None detected; review dynamic code and Electron API usage.'}\n\n## Derived permissions — review every grant\n\n\`\`\`json\n${JSON.stringify(permissions,null,2)}\n\`\`\`\n\n${usage.relativeFs?'The ./data filesystem root is a placeholder for detected relative paths; review original paths and access.\n':usage.fs?'No ./data root was granted; userData and dialog-selected files are auto-granted. Review other filesystem paths.\n':''}\n## Manual steps\n\n- ${preloadNote}\n- Review Electron APIs, renderer Node integration, dynamic paths, package dependencies, build/Forge settings, and permissions.\n- Original titleBarStyle: ${titleBarStyle || '(not specified)'}.\n- Original electron-builder configuration: ${pkg.build ? 'present' : 'not present'}; Electron Forge configuration: ${pkg.config?.forge ? 'present' : 'not present'}.\n- Run \`tsgpui build\` in this directory.\n`;
  await writeFile(path.join(dest,'TSGPUI_IMPORT_REPORT.md'),report);
  console.log(`Imported Electron project to ${dest}. Review TSGPUI_IMPORT_REPORT.md.`);
  return {out:dest,config,report};
}
