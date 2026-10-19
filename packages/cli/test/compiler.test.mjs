import test from 'node:test';
import assert from 'node:assert/strict';
import os from 'node:os';
import path from 'node:path';
import vm from 'node:vm';
import { mkdtemp, writeFile, readFile, rm, mkdir } from 'node:fs/promises';
import { parseHTML } from '../src/html.mjs';
import { compileCSS, compileStylesheet, styleTree } from '../src/css.mjs';
import { compileBackend } from '../src/backend.mjs';
import { compileProject, buildPackage, validateConfig } from '../src/compiler.mjs';
import {packageExecutable,extractEmbeddedBundleForTest,MAGIC} from '../src/pack.mjs';

const html='<body><main class="card"><h1>Hi</h1><button id="go" data-onclick="increment">Go</button><span data-bind="count">0</span></main></body>';
const css='body { color:#fff; } .card { display:flex; gap:12px; } .card button { color:#abcdef; } #go { padding:10px; }';
const backend=`export default { state:{ count: 0 }, actions:{ increment(api) { api.set('count', api.get('count')+1); } } };`;
const config={name:'test-app',title:'Test',width:500,height:400};

test('HTML parses a nested native tree and action attributes',()=>{
  const root=parseHTML(html);
  assert.equal(root.tag,'body');
  assert.equal(root.children[0].children[1].attrs['data-onclick'],'increment');
  assert.equal(root.children[0].children[2].attrs['data-bind'],'count');
});
test('HTML rejects scripts, duplicate IDs, and unsupported attributes',()=>{
  assert.throws(()=>parseHTML('<body><script>bad()</script></body>'),/Unsupported HTML element/);
  assert.throws(()=>parseHTML('<body><div id="a"></div><div id="a"></div></body>'),/Duplicate HTML id/);
  assert.throws(()=>parseHTML('<body><button onclick="evil()">x</button></body>'),/Unsupported HTML attribute/);
});
test('HTML decodes text entities',()=>{
  const root=parseHTML('<body><p>A &amp; B &#10003;</p></body>');
  assert.equal(root.children[0].children[0].text,'A & B ✓');
});
test('CSS class and descendant selectors apply with CSS specificity and inherited color',()=>{
  const root=styleTree(parseHTML(html),compileCSS(css));
  assert.equal(root.style.color,'#ffffff');
  const main=root.children[0],btn=main.children[1];
  assert.equal(main.style.gap,'12px');
  assert.equal(btn.style.color,'#abcdef');
  assert.equal(btn.style.padding,'10px');
  assert.equal(main.children[0].style.color,'#ffffff');
});
test('CSS unsupported declarations are warnings by default and errors in strict mode',()=>{
  const lenient=compileStylesheet('p{transition: all 1s; color: #fff} p{calc: nope}');
  assert.equal(lenient.rules.length,2);
  assert.match(lenient.warnings[0],/line 1: 'transition' is not supported yet/);
  assert.match(lenient.warnings[1],/unknown CSS property 'calc'/);
  assert.throws(()=>compileStylesheet('p{transition: all 1s}',{strict:true}),/'transition' is not supported yet/);
});
test('Backend transpiles TS and actions mutate state in a JS VM',()=>{
  const js=compileBackend(`type S = {count:number}; const app: S = {count:0}; export default {state:app,actions:{increment(api: any){api.set('count',api.get('count')+1)}}};`);
  const context=vm.createContext({});
  vm.runInContext(js,context,{timeout:1000});
  const model=context.__tsgpui_app;
  const api={ get:k=>model.state[k],set:(k,v)=>{model.state[k]=v;} };
  model.actions.increment(api);
  assert.equal(model.state.count,1);
});
test('Backend rejects runtime imports and require',()=>{
  assert.throws(()=>compileBackend('import fs from "fs"; export default {}'),/only `import type`/);
  assert.throws(()=>compileBackend('const a=require("os"); export default {}'),/not supported/);
});
test('Compiler creates strict package schema',()=>{
  const doc=compileProject({html,css,backend,config});
  assert.equal(doc.format,'tsgpui-1');
  assert.equal(doc.root.tag,'body');
  assert.ok(doc.backend_js.includes('__tsgpui_app'));
});
test('Config rejects invalid paths/name/window sizes',()=>{
  assert.throws(()=>validateConfig({name:'../bad',html:'x',css:'y',backend:'z'}),/Invalid name/);
  assert.throws(()=>validateConfig({name:'Good',html:'x',css:'y',backend:'z',window:{width:-1,height:500}}),/Window dimensions/);
});
test('Compiles project from files, then packages and extracts native overlay',async()=>{
  const temp=await mkdtemp(path.join(os.tmpdir(),'tsgpui-test-'));
  try {
    await mkdir(path.join(temp,'src'));
    await writeFile(path.join(temp,'tsgpui.config.json'),JSON.stringify({...config,html:'src/index.html',css:'src/style.css',backend:'src/backend.ts',outDir:'dist'}));
    await writeFile(path.join(temp,'src/index.html'),html);
    await writeFile(path.join(temp,'src/style.css'),css);
    await writeFile(path.join(temp,'src/backend.ts'),backend);
    const compiled=await buildPackage(temp);
    assert.equal(compiled.bundle.format,'tsgpui-1');
    const host=path.join(temp,'host.exe'),out=path.join(temp,'app.exe');
    const fakeHost=Buffer.alloc(1024,0);
    fakeHost.write('MZ');
    await writeFile(host,fakeHost);
    await packageExecutable(host,compiled.bundleFile,out);
    const binary=await readFile(out);
    assert.equal(binary.subarray(-8).toString('ascii'),MAGIC.toString('ascii'));
    assert.equal(extractEmbeddedBundleForTest(binary).title,'Test');
    assert.equal((await readFile(host)).length,1024);
  } finally {await rm(temp,{recursive:true,force:true});}
});
