import { cp, mkdir, rm, writeFile } from 'node:fs/promises';
import { join } from 'node:path';

const root = process.cwd();
const out = join(root, '.vercel-out');
await rm(out, { recursive: true, force: true });
await mkdir(out, { recursive: true });
await cp(join(root, 'web'), join(out, 'web'), { recursive: true });
await cp(join(root, 'examples'), join(out, 'examples'), { recursive: true });
await writeFile(join(out, 'index.html'), `<!doctype html>
<html lang="zh-CN"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<meta http-equiv="refresh" content="0; url=./web/">
<title>MarkdownX · interactive books</title>
<style>html,body{margin:0;height:100%;background:#f6f2e9;color:#241f1a;font:16px/1.5 system-ui,sans-serif;display:grid;place-items:center}a{color:#8f3f1d}</style>
</head><body><p>Opening <a href="./web/">MarkdownX Reader</a>…</p>
<script>location.replace('./web/');</script></body></html>`, 'utf8');
console.log(`built ${out}`);
