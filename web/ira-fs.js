const chooseButtons = [document.querySelector('#choose-folder'), document.querySelector('#welcome-choose')];
const restoreButton = document.querySelector('#restore-folder');
const welcome = document.querySelector('#welcome');
const unsupported = document.querySelector('#unsupported');
const editor = document.querySelector('#editor-dialog');
const editorContent = document.querySelector('#editor-content');
const editorName = document.querySelector('#editor-name');
const sampleButton = document.querySelector('#sample-workspace');
let rootHandle = null;
let editorPath = null;
const panePaths = ['/', '/'];

const siteThemes = new Set(['archive', 'night-city', 'pro', 'cyberpunk', 'neon-grid', 'crt-green']);
const siteThemeColors = {
  archive: '#ede9dc',
  'night-city': '#f3e600',
  pro: '#0c1118',
  cyberpunk: '#0c1118',
  'neon-grid': '#071018',
  'crt-green': '#061008'
};

function readSiteTheme() {
  try {
    const current = localStorage.getItem('ira-site-theme-v2');
    const previous = localStorage.getItem('ira-site-theme');
    const saved = current || (previous === 'cyberpunk' ? 'pro' : previous);
    return siteThemes.has(saved) ? saved : 'pro';
  } catch {
    return 'pro';
  }
}

function applySiteTheme(theme = readSiteTheme()) {
  const selected = siteThemes.has(theme) ? theme : 'pro';
  document.documentElement.dataset.theme = selected;
  document.documentElement.style.colorScheme = selected === 'archive' || selected === 'night-city' ? 'light' : 'dark';
  const themeColor = document.querySelector('meta[name="theme-color"]');
  if (themeColor) themeColor.content = siteThemeColors[selected];
}

applySiteTheme();
window.addEventListener('storage', event => {
  if (event.key === 'ira-site-theme-v2' || event.key === 'ira-site-theme') applySiteTheme();
});

const bindings = () => window.wasmBindings;
const status = (message, error = false) => bindings()?.ira_status?.(message, error);

function focusTerminal() {
  const focusGrid = () => {
    if (editor.open) return;
    const grid = document.querySelector('#ira-terminal_ratzilla_grid');
    const target = grid || document.querySelector('#ira-terminal');
    if (!target) return;
    if (!target.hasAttribute('tabindex')) target.setAttribute('tabindex', '-1');
    target.focus({preventScroll: true});
  };
  focusGrid();
  requestAnimationFrame(() => {
    focusGrid();
    requestAnimationFrame(focusGrid);
  });
}

document.querySelector('#ira-terminal').addEventListener('focusin', event => {
  if (event.target.id === 'ira-terminal') focusTerminal();
});

function showProblem(message) {
  unsupported.textContent = message;
  unsupported.hidden = false;
  status(message, true);
}

function clearProblem() { unsupported.hidden = true; }

async function waitForBindings() {
  while (typeof bindings()?.ira_load_listing !== 'function') {
    await new Promise(resolve => requestAnimationFrame(resolve));
  }
}

function openDb() {
  return new Promise((resolve, reject) => {
    const request = indexedDB.open('ira-browser', 1);
    request.onupgradeneeded = () => request.result.createObjectStore('handles');
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
  });
}

async function storeHandle(handle) {
  const db = await openDb();
  const tx = db.transaction('handles', 'readwrite');
  tx.objectStore('handles').put(handle, 'root');
}

async function savedHandle() {
  const db = await openDb();
  return new Promise(resolve => {
    const request = db.transaction('handles').objectStore('handles').get('root');
    request.onsuccess = () => resolve(request.result || null);
    request.onerror = () => resolve(null);
  });
}

function parts(path) { return path.split('/').filter(Boolean); }

async function directory(path) {
  let handle = rootHandle;
  for (const name of parts(path)) handle = await handle.getDirectoryHandle(name);
  return handle;
}

async function entryHandle(path) {
  const segments = parts(path);
  const name = segments.pop();
  const parent = await directory('/' + segments.join('/'));
  try { return { handle: await parent.getDirectoryHandle(name), parent, name, kind: 'directory' }; }
  catch { return { handle: await parent.getFileHandle(name), parent, name, kind: 'file' }; }
}

async function sendListing(path, pane = 0) {
  if (!rootHandle) return;
  await waitForBindings();
  const handle = await directory(path);
  const entries = [];
  for await (const [name, child] of handle.entries()) {
    let size = 0, modified = null;
    if (child.kind === 'file') {
      const file = await child.getFile();
      size = file.size;
      modified = Math.floor(file.lastModified / 1000);
    }
    entries.push({name,path:`${path === '/' ? '' : path}/${name}`,is_dir:child.kind === 'directory',size,modified});
  }
  entries.sort((a,b) => Number(b.is_dir)-Number(a.is_dir) || a.name.localeCompare(b.name,undefined,{numeric:true,sensitivity:'base'}));
  panePaths[pane] = path;
  const label = path === '/' ? (rootHandle.name || 'IRA Workspace') : handle.name;
  bindings().ira_load_listing(JSON.stringify(entries), label, path, pane);
  clearProblem();
  welcome.hidden = true;
  focusTerminal();
}

async function refreshPath(path) {
  await Promise.all(panePaths.map((panePath, pane) => panePath === path ? sendListing(path, pane) : null));
}

async function connect(handle) {
  const permission = typeof handle.requestPermission === 'function'
    ? await handle.requestPermission({mode:'readwrite'})
    : 'granted';
  if (permission !== 'granted') throw new Error('Folder permission was not granted.');
  rootHandle = handle;
  await storeHandle(handle);
  await sendListing('/', 0);
  await sendListing('/', 1);
}

async function choose() {
  if (!window.showDirectoryPicker) {
    showProblem('Folder access requires current desktop Chrome or Edge.');
    return;
  }
  try {
    await connect(await window.showDirectoryPicker({mode:'readwrite',id:'ira-root',startIn:'documents'}));
  } catch (error) {
    if (error.name === 'AbortError') return;
    const protectedFolder = error.name === 'SecurityError' || /system files|sensitive|not allowed/i.test(error.message);
    showProblem(protectedFolder
      ? 'Chrome protects the entire Home folder and system folders. Choose a folder inside Home, such as Projects, Documents, Downloads, or Pictures.'
      : `Folder access failed: ${error.message}`);
  }
}

for (const button of chooseButtons) button.addEventListener('click', choose);

async function writeSampleFile(parent, name, content) {
  const handle = await parent.getFileHandle(name, {create:true});
  const file = await handle.getFile();
  if (file.size > 0) return;
  const writable = await handle.createWritable();
  await writable.write(content);
  await writable.close();
}

sampleButton.addEventListener('click', async () => {
  try {
    const privateRoot = await navigator.storage.getDirectory();
    const workspace = await privateRoot.getDirectoryHandle('IRA Workspace', {create:true});
    const source = await workspace.getDirectoryHandle('src', {create:true});
    await writeSampleFile(workspace, 'README.md', '# IRA Browser\n\nThis writable workspace lives only in this browser.');
    await writeSampleFile(workspace, 'notes.txt', 'Use arrow keys to navigate. Press V for previews and + for dual panes.\n');
    await writeSampleFile(source, 'main.rs', 'fn main() {\n    println!("IRA in the browser");\n}\n');
    await connect(workspace);
  } catch (error) { showProblem(`Could not create the browser workspace: ${error.message}`); }
});
restoreButton.addEventListener('click', async () => {
  try { const handle = await savedHandle(); if (handle) await connect(handle); }
  catch (error) { status(error.message, true); }
});

window.iraRequestDirectory = (path, pane) => sendListing(path, pane).catch(error => status(error.message,true));

window.iraRequestPreview = async (path, kind) => {
  try {
    const {handle} = await entryHandle(path);
    const file = await handle.getFile();
    if (kind === 'text') {
      const cap = 256 * 1024;
      const bytes = new Uint8Array(await file.slice(0, cap).arrayBuffer());
      const binary = bytes.includes(0);
      const content = new TextDecoder().decode(bytes);
      bindings().ira_load_text(path, content, binary, file.size > cap);
    } else if (kind === 'image') {
      bindings().ira_load_image(path, new Uint8Array(await file.arrayBuffer()));
    } else if (kind === 'video') {
      const url = URL.createObjectURL(file);
      try {
        const video = document.createElement('video');
        video.muted = true; video.src = url; video.currentTime = 0.1;
        await new Promise((resolve,reject)=>{video.onloadeddata=resolve;video.onerror=reject;});
        const canvas = document.createElement('canvas');
        canvas.width = Math.min(video.videoWidth,1280); canvas.height = Math.round(canvas.width*video.videoHeight/video.videoWidth);
        canvas.getContext('2d').drawImage(video,0,0,canvas.width,canvas.height);
        const blob = await new Promise(resolve=>canvas.toBlob(resolve,'image/jpeg',.86));
        bindings().ira_load_image(path,new Uint8Array(await blob.arrayBuffer()));
      } finally { URL.revokeObjectURL(url); }
    }
  } catch (error) { status(`Preview unavailable: ${error.message}`,true); }
};

window.iraCreateEntry = async path => {
  const name = prompt('Create a file or folder\nAdd an extension for a file (for example notes.md):');
  if (!name?.trim()) return;
  try {
    const target = await directory(path);
    const clean = name.trim();
    if (clean.includes('/')) throw new Error('Create one level at a time.');
    if (/^\.[^.]+$/.test(clean) || !clean.includes('.') || clean.endsWith('.')) await target.getDirectoryHandle(clean,{create:true});
    else await target.getFileHandle(clean,{create:true});
    status(`Created ${clean}`); await refreshPath(path);
  } catch(error) { status(error.message,true); }
};

async function copyRecursive(source, destination, name) {
  if (source.kind === 'file') {
    const out = await destination.getFileHandle(name,{create:true});
    const writable = await out.createWritable();
    await writable.write(await source.getFile()); await writable.close();
  } else {
    const out = await destination.getDirectoryHandle(name,{create:true});
    for await (const [childName,child] of source.entries()) await copyRecursive(child,out,childName);
  }
}

window.iraRenameEntry = async path => {
  const current = parts(path).at(-1); const next = prompt('Rename entry:',current);
  if (!next || next === current) return;
  try {
    if (next.includes('/')) throw new Error('The new name cannot contain a slash.');
    const item = await entryHandle(path);
    await copyRecursive(item.handle,item.parent,next);
    await item.parent.removeEntry(item.name,{recursive:true});
    status(`Renamed ${current} to ${next}`);
    const parentPath='/' + parts(path).slice(0,-1).join('/'); await refreshPath(parentPath || '/');
  } catch(error) { status(error.message,true); }
};

window.iraDeleteEntry = async path => {
  const item = await entryHandle(path);
  if (!confirm(`Delete ${item.name}${item.kind === 'directory' ? ' and everything inside it' : ''}?`)) return;
  try { await item.parent.removeEntry(item.name,{recursive:true}); status(`Deleted ${item.name}`); const p='/' + parts(path).slice(0,-1).join('/'); await refreshPath(p || '/'); }
  catch(error){ status(error.message,true); }
};

window.iraTransferEntry = async (sourcePath,destinationPath,move) => {
  try {
    const source = await entryHandle(sourcePath); const destination = await directory(destinationPath);
    await copyRecursive(source.handle,destination,source.name);
    if (move) await source.parent.removeEntry(source.name,{recursive:true});
    status(`${move?'Moved':'Copied'} ${source.name}`);
    await refreshPath(destinationPath);
    if (move) {
      const sourceParent = '/' + parts(sourcePath).slice(0,-1).join('/');
      if ((sourceParent || '/') !== destinationPath) await refreshPath(sourceParent || '/');
    }
  } catch(error){ status(error.message,true); }
};

window.iraOpenEditor = async path => {
  try {
    const {handle,name} = await entryHandle(path); const file = await handle.getFile();
    if (file.size > 5*1024*1024) throw new Error('Text editing is limited to 5 MB files.');
    editorPath=path; editorName.textContent=name; editorContent.value=await file.text(); editor.showModal(); editorContent.focus();
  } catch(error){ status(error.message,true); }
};

document.querySelector('#save-editor').addEventListener('click',async event=>{
  event.preventDefault();
  try { const {handle}=await entryHandle(editorPath); const writable=await handle.createWritable(); await writable.write(editorContent.value); await writable.close(); status(`Saved ${parts(editorPath).at(-1)}`); editor.close(); window.iraRequestPreview(editorPath,'text'); }
  catch(error){ status(error.message,true); }
});
editor.addEventListener('keydown',event=>{if(event.ctrlKey&&event.key.toLowerCase()==='s'){event.preventDefault();document.querySelector('#save-editor').click();}});

window.addEventListener('TrunkApplicationStarted', async () => {
  if (!window.showDirectoryPicker) showProblem('Folder access requires current desktop Chrome or Edge.');
  const handle=await savedHandle(); if(handle){restoreButton.hidden=false; restoreButton.textContent='Reconnect workspace';}
});
