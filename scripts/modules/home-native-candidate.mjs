// Isolated signed candidate and Native Messaging registration for browser checks.
import { spawn, spawnSync } from 'node:child_process';
import { copyFileSync, writeFileSync, mkdirSync, chmodSync, existsSync, readFileSync, appendFileSync } from 'node:fs';
import { join } from 'node:path';
import { buildExtension, STABLE_EXTENSION_ID } from '../extension-package.mjs';
export function prepareNativeCandidate({ root, out, fakeHome, config, reportOut, env, isolatedPaths, soak, providerPort }) {
function run(cmd, args, cwd = root, buildEnv = env) {
  const result = spawnSync(cmd, args, { env: buildEnv, cwd, encoding: 'utf8', timeout: 120000 });
  if (result.status !== 0) throw Error(`${cmd} failed: ${(result.stderr || result.stdout).slice(-2000)}`);
}
  console.log('Preparing isolated signed candidate and Model Host process fixture…');
  run('rtk', ['proxy', process.execPath, 'scripts/apps/refresh-dev-product-source.mjs', ...(soak ? ['--release'] : [])]);
  // Production Files Host correctly refuses the development trust root. Keep that
  // defense; this browser candidate uses development Core + Release App Runtime.
  run('rtk', ['env', '-u', 'CARGO_TARGET_DIR', 'cargo', 'build', '-p', 'native-file-host']);
  const fileHost = join(out, 'native-file-host');
  copyFileSync(join(root, 'target/debug/native-file-host'), fileHost);
  // These UI regressions use an initialized Core store; empty-store concurrent startup is a separate acceptance path.
  const bootstrap=spawnSync(fileHost,[`chrome-extension://${STABLE_EXTENSION_ID}/`],{env,input:Buffer.alloc(0),encoding:'utf8',timeout:10000});
  if(bootstrap.status!==0||/initialization failed/.test(bootstrap.stderr||''))throw Error(`isolated Core initialization failed: ${bootstrap.stderr||bootstrap.error||bootstrap.status}`);
  const fixture = join(out, 'model-host-process.test');
  run('rtk', ['go', 'test', '-c', '-o', fixture, '.'], join(root, 'model-host'), process.env);
  const state = { schemaVersion: 3, revision: 1, defaultModel: { providerId: 'process-provider', modelId: 'process-model' }, providers: [{ id: 'process-provider', kind: 'custom', name: 'Synthetic provider', baseUrl: `http://127.0.0.1:${providerPort}/v1`, protocol: 'openai_chat', enabled: true, allowLan: true, secretRef: 'process-provider', models: [{ id: 'process-model', displayName: 'Synthetic model', enabled: true, manual: true }] }], accounts: [], gateway: { state: 'stopped', resident: false, accessKeys: [{ id: 'key_default', name: 'Synthetic key', secretRef: 'gateway:access-key', enabled: true }], settings: { routingStrategy: 'round_robin', requestRetry: 0, maxRetryIntervalSeconds: 1 } } };
  writeFileSync(join(config, 'state.json'), JSON.stringify(state));
  const ext = join(out, 'extension'); buildExtension(ext, root);
  const profile = join(out, 'chrome-profile');
  const hostDir = join(profile, 'NativeMessagingHosts'); mkdirSync(hostDir, { recursive: true });
  for (const [name, bin, extra] of [
    ['com.natives.file_manager', fileHost, []],
    ['com.natives.local.app_runtime', join(fakeHome, '.natives-local/product-source/hosts/natives-app-runtime'), []],
    ['com.natives.model_host', fixture, ['-test.run=^TestModelHostProcessHelper$', '--']],
  ]) {
    const wrapper = join(out, `${name}.mjs`);
    writeFileSync(wrapper, `#!${process.execPath}\nimport {spawn} from 'node:child_process';\nimport {appendFileSync} from 'node:fs';\nconst child=spawn(${JSON.stringify(bin)}, [...${JSON.stringify(extra)}, ...process.argv.slice(2)], {env:{...process.env,...${JSON.stringify(isolatedPaths)}},stdio:['inherit','inherit','pipe']});\nchild.stderr.on('data',chunk=>{appendFileSync(${JSON.stringify(join(reportOut, `${name}.stderr.log`))},chunk);process.stderr.write(chunk);});\nappendFileSync(${JSON.stringify(join(out, 'spawns.jsonl'))},JSON.stringify({host:${JSON.stringify(name)},pid:child.pid})+'\\n');\nchild.on('exit',code=>process.exit(code||0));\n`);
    chmodSync(wrapper, 0o755);
    writeFileSync(join(hostDir, `${name}.json`), JSON.stringify({ name, description: 'Isolated investment acceptance', path: wrapper, type: 'stdio', allowed_origins: [`chrome-extension://${STABLE_EXTENSION_ID}/`] }));
  }
  return { profile, ext };
}

// CDP noDefaults 保留浏览器真实焦点／可见性，不由 Playwright 强制所有页可见。
export async function launchSoakBrowser({ chromium, chrome, profile, ext, env, reportOut, report }) {
  const child = spawn(chrome, ['--headless=new', '--no-startup-window', '--remote-debugging-port=0', `--user-data-dir=${profile}`,
    `--disable-extensions-except=${ext}`, `--load-extension=${ext}`,
    '--disable-features=DisableLoadExtensionCommandLineSwitch', '--use-mock-keychain',
    '--password-store=basic', '--no-first-run'], {env,stdio:['ignore','ignore','pipe']});
  child.stderr.on('data', chunk => appendFileSync(join(reportOut,'chrome.stderr.log'),chunk));
  child.once('exit',(code,signal) => { report.browserExit = {code,signal,at:new Date().toISOString()}; });
  try {
    const file = join(profile, 'DevToolsActivePort');
    const deadline = Date.now() + 15000;
    while (!existsSync(file)) {
      if (child.exitCode !== null || Date.now() >= deadline) throw Error('isolated Chrome CDP startup failed');
      await new Promise(resolve => setTimeout(resolve, 100));
    }
    const port = readFileSync(file, 'utf8').split('\n')[0];
    const browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`, {noDefaults:true});
    return { context:browser.contexts()[0], child };
  } catch (error) { child.kill(); throw error; }
}

export const installNativeTimings = () => {
    if (!globalThis.chrome?.runtime?.connectNative) return;
    const connect = chrome.runtime.connectNative.bind(chrome.runtime);
    window.nativeTimings = [];
    window.nativeFailures = [];
    chrome.runtime.connectNative = name => {
      const port = connect(name), pending = new Map(), send = port.postMessage.bind(port);
      port.postMessage = request => { pending.set(request.id, { method: request.method, started: performance.now() }); return send(request); };
      port.onMessage.addListener(reply => { const item = pending.get(reply.id); if (item) { pending.delete(reply.id); window.nativeTimings.push({ host: name, method: item.method, ms: performance.now() - item.started }); } });
      port.onDisconnect.addListener(() => {const error=chrome.runtime.lastError?.message;if(error)window.nativeFailures.push({host:name,error});});
      return port;
    };
  };
