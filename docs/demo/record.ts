// Renders demo.html frame by frame and encodes docs/media/demo.mp4 + demo.gif.
//
// Requirements: Node 22.6+ (type stripping), ffmpeg on PATH, and Playwright
// resolvable from PLAYWRIGHT_DIR (a folder with playwright installed), e.g.:
//   mkdir /tmp/pw && cd /tmp/pw && pnpm add playwright && pnpm exec playwright install chromium
//   PLAYWRIGHT_DIR=/tmp/pw node --experimental-strip-types docs/demo/record.ts
import { execFileSync } from 'node:child_process';
import { mkdirSync, rmSync, statSync } from 'node:fs';
import { createRequire } from 'node:module';
import { join, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

const FPS = 30;
const SCALE = 1.5; // 1280x720 page -> 1920x1080 frames
const here = import.meta.dirname;
const mediaDir = resolve(here, '../media');
const framesDir = join(mediaDir, '.frames');

const pwDir = process.env.PLAYWRIGHT_DIR;
const req = createRequire(pwDir ? join(resolve(pwDir), 'package.json') : import.meta.filename);
const { chromium } = req('playwright') as typeof import('playwright');

rmSync(framesDir, { recursive: true, force: true });
mkdirSync(framesDir, { recursive: true });

const browser = await chromium.launch();
const page = await browser.newPage({ viewport: { width: 1280, height: 720 }, deviceScaleFactor: SCALE });
await page.goto(`${pathToFileURL(join(here, 'demo.html')).href}?capture`);
await page.waitForFunction(() => (window as unknown as { __ready?: boolean }).__ready === true);
const duration = await page.evaluate(() => (window as unknown as { __duration: number }).__duration);

const total = Math.round(duration * FPS);
for (let f = 0; f < total; f++) {
  await page.evaluate(t => (window as unknown as { __render: (t: number) => void }).__render(t), f / FPS);
  await page.screenshot({ path: join(framesDir, `${String(f).padStart(4, '0')}.png`) });
  if (f % 60 === 0) console.log(`frame ${f}/${total}`);
}
await browser.close();

const input = ['-y', '-loglevel', 'error', '-framerate', String(FPS), '-i', join(framesDir, '%04d.png')];
const mp4 = join(mediaDir, 'demo.mp4');
const gif = join(mediaDir, 'demo.gif');
execFileSync('ffmpeg', [...input, '-c:v', 'libx264', '-pix_fmt', 'yuv420p', '-crf', '20', '-preset', 'slow', '-movflags', '+faststart', mp4], { stdio: 'inherit' });
execFileSync('ffmpeg', [
  ...input,
  '-vf', 'fps=8,scale=960:-1:flags=lanczos,split[a][b];[a]palettegen=max_colors=48:stats_mode=diff[p];[b][p]paletteuse=dither=none:diff_mode=rectangle',
  '-loop', '0', gif,
], { stdio: 'inherit' });
if (!process.env.KEEP_FRAMES) rmSync(framesDir, { recursive: true, force: true });

for (const f of [mp4, gif]) console.log(`${f}: ${(statSync(f).size / 1024 / 1024).toFixed(2)} MB`);
