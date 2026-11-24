// Run with Windows Node.js: node tools/build-mode-icons.cjs <evidence-directory>
// Uses an isolated, headless Edge profile. No npm packages or user browser data.
const fs = require('node:fs');
const path = require('node:path');
const os = require('node:os');
const { spawn } = require('node:child_process');
const assert = require('node:assert/strict');
const res = path.resolve(__dirname, '../crates/client/res');
const out = process.argv[2];
if (!out || process.platform !== 'win32') throw new Error('Windows Node and evidence directory required');
fs.mkdirSync(out, { recursive: true });
// Keep the former 32px default first for callers that request size zero.
const sizes = [32, 16, 20, 24, 40, 48, 64, 96, 128, 256];
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));

function ico(frames) {
  const header = Buffer.alloc(6 + frames.length * 16);
  header.writeUInt16LE(1, 2);
  header.writeUInt16LE(frames.length, 4);
  let offset = header.length;
  frames.forEach(({ size, png }, i) => {
    const n = 6 + i * 16;
    header[n] = header[n + 1] = size === 256 ? 0 : size;
    header.writeUInt16LE(1, n + 4);
    header.writeUInt16LE(32, n + 6);
    header.writeUInt32LE(png.length, n + 8);
    header.writeUInt32LE(offset, n + 12);
    offset += png.length;
  });
  return Buffer.concat([header, ...frames.map(f => f.png)]);
}

async function main() {
  const edge = path.join(process.env['ProgramFiles(x86)'], 'Microsoft/Edge/Application/msedge.exe');
  const profile = fs.mkdtempSync(path.join(os.tmpdir(), 'azookey-icon-render-'));
  const browser = spawn(edge, ['--headless=new', '--remote-debugging-port=0',
    '--remote-debugging-address=127.0.0.1', `--user-data-dir=${profile}`,
    '--no-first-run', '--no-default-browser-check', 'about:blank'], { stdio: 'ignore' });
  let launchError;
  browser.on('error', error => { launchError = error; });
  let ws, call;
  try {
    let pages;
    for (let i = 0; i < 200; i++) {
      if (launchError) throw launchError;
      try {
        const port = fs.readFileSync(path.join(profile, 'DevToolsActivePort'), 'utf8').split('\n')[0];
        pages = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json();
        if (pages.some(p => p.type === 'page')) break;
      } catch {}
      await sleep(100);
    }
    assert(pages?.some(p => p.type === 'page'), 'isolated browser did not start');
    ws = new WebSocket(pages.find(p => p.type === 'page').webSocketDebuggerUrl);
    await new Promise((resolve, reject) => {
      ws.addEventListener('open', resolve, { once: true });
      ws.addEventListener('error', reject, { once: true });
    });
    let id = 0;
    const pending = new Map();
    ws.addEventListener('message', event => {
      const reply = JSON.parse(event.data);
      pending.get(reply.id)?.(reply);
    });
    call = (method, params = {}) => new Promise((resolve, reject) => {
      const n = ++id;
      const timer = setTimeout(() => { pending.delete(n); reject(new Error(`Timeout: ${method}`)); }, 10000);
      pending.set(n, reply => {
        pending.delete(n); clearTimeout(timer);
        reply.error ? reject(new Error(JSON.stringify(reply.error))) : resolve(reply.result);
      });
      ws.send(JSON.stringify({ id: n, method, params }));
    });
    const evaluate = async expression => {
      const result = await call('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true });
      if (result.exceptionDetails) throw new Error(JSON.stringify(result.exceptionDetails));
      return result.result.value;
    };
    const report = [], masks = new Map(), outputs = [];
    for (const mode of ['kana', 'latin']) {
      const source = fs.readFileSync(path.join(res, `mode_${mode}.svg`), 'utf8');
      for (const color of ['black', 'white']) {
        const frames = [];
        for (const size of sizes) {
          const rendered = await evaluate(`(async () => {
            const doc = new DOMParser().parseFromString(${JSON.stringify(source)}, 'image/svg+xml');
            if (doc.querySelector('parsererror, script, image, use')) throw new Error('Invalid standalone SVG');
            const svg = doc.documentElement;
            svg.setAttribute('width', '${size}'); svg.setAttribute('height', '${size}');
            svg.style.color = '${color}';
            const image = new Image();
            image.src = 'data:image/svg+xml;charset=utf-8,' + encodeURIComponent(new XMLSerializer().serializeToString(svg));
            await image.decode();
            const canvas = document.createElement('canvas'); canvas.width = canvas.height = ${size};
            const ctx = canvas.getContext('2d'); ctx.drawImage(image, 0, 0);
            const pixels = ctx.getImageData(0, 0, ${size}, ${size}).data;
            const alpha = [], bounds = [${size}, ${size}, -1, -1];
            let coverage = 0, wrongColor = 0;
            for (let i = 0; i < pixels.length; i += 4) {
              alpha.push(pixels[i + 3]); coverage += pixels[i + 3] / 255;
              if (pixels[i + 3]) {
                const x = (i / 4) % ${size}, y = Math.floor(i / 4 / ${size});
                bounds[0] = Math.min(bounds[0], x); bounds[1] = Math.min(bounds[1], y);
                bounds[2] = Math.max(bounds[2], x); bounds[3] = Math.max(bounds[3], y);
                if ([pixels[i], pixels[i+1], pixels[i+2]].some(v => v !== ${color === 'black' ? 0 : 255})) wrongColor++;
              }
            }
            return {png:canvas.toDataURL('image/png').split(',')[1], alpha, bounds, coverage, wrongColor};
          })()`);
          assert(rendered.coverage > size * size * 0.05 && rendered.coverage < size * size * 0.5, 'blank or solid icon');
          assert.equal(rendered.wrongColor, 0, 'guide colors must not survive');
          const key = `${mode}:${size}`;
          if (color === 'black') masks.set(key, rendered.alpha);
          else assert.deepEqual(rendered.alpha, masks.get(key), 'theme geometry differs');
          const png = Buffer.from(rendered.png, 'base64');
          fs.writeFileSync(path.join(out, `${mode}_${color}_${size}.png`), png);
          frames.push({ size, png });
          report.push({ mode, color, size, bounds: rendered.bounds, coverage: rendered.coverage });
        }
        outputs.push({ filename: `${mode}_` + color + '.ico', bytes: ico(frames) });
      }
    }
    for (const size of sizes) assert.notDeepEqual(masks.get(`kana:${size}`), masks.get(`latin:${size}`));
    // Write resources only after every size and theme passes validation.
    for (const output of outputs) fs.writeFileSync(path.join(res, output.filename), output.bytes);
    const tiles = ['white', 'black'].map(background => {
      const color = background === 'white' ? 'black' : 'white';
      return `<section style="background:${background};color:${color};padding:24px">` +
        ['kana', 'latin'].map(mode => '<div style="display:flex;align-items:center;gap:24px;margin:20px">' +
          [16, 24, 32, 64, 128].map(size => `<img width="${size}" height="${size}" src="data:image/png;base64,${fs.readFileSync(path.join(out, `${mode}_${color}_${size}.png`)).toString('base64')}">`).join('') + '</div>').join('') + '</section>';
    }).join('');
    await call('Emulation.setDeviceMetricsOverride', { width: 720, height: 740, deviceScaleFactor: 1, mobile: false });
    await evaluate(`document.body.style.margin = '0'; document.body.innerHTML = ${JSON.stringify(tiles)}`);
    await sleep(200);
    const shot = await call('Page.captureScreenshot', { format: 'png' });
    fs.writeFileSync(path.join(out, 'preview.png'), Buffer.from(shot.data, 'base64'));
    fs.writeFileSync(path.join(out, 'icons.json'), JSON.stringify(report, null, 2));
    console.log(`Verified ${report.length} icon images; wrote four multi-resolution ICO resources.`);
  } finally {
    if (call) await call('Browser.close').catch(() => {});
    ws?.close();
    await sleep(1000);
    if (browser.exitCode === null) browser.kill();
    try { fs.rmSync(profile, { recursive: true, force: true, maxRetries: 5, retryDelay: 200 }); }
    catch { console.warn(`Isolated browser profile retained: ${profile}`); }
  }
}
main().catch(error => { console.error(error); process.exitCode = 1; });
