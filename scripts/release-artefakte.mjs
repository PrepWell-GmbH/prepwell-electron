// Stellt nach `npm run dist:mac` die Dateien für das GitHub-Release zusammen (läuft auf macOS, in der CI).
//
//   out/PrepWell.dmg                          Erstinstallation — der Download-Link der Website zeigt auf diesen Namen
//   out/PrepWell.app.tar.gz + out/latest.json Updates für die Tauri-App (tauri-plugin-updater, signiert mit dem Updater-Schlüssel)
//   out/PrepWell-<v>-universal-mac.zip
//   + out/latest-mac.yml                      Brücke für die alten Electron-Installationen: deren electron-updater liest
//                                             latest-mac.yml und spielt das ZIP über Squirrel.Mac ein. So wechseln
//                                             bestehende Nutzer per Auto-Update auf die Tauri-App. Die Brücke bleibt,
//                                             bis niemand mehr eine Electron-Version (≤ 1.0.14) benutzt.

import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { copyFileSync, mkdirSync, readFileSync, readdirSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

const REPO = 'PrepWell-GmbH/prepwell-electron';
const { version } = JSON.parse(readFileSync('package.json', 'utf8'));
const bundle = 'src-tauri/target/universal-apple-darwin/release/bundle';
const out = 'out';

rmSync(out, { recursive: true, force: true });
mkdirSync(out);

// 1. DMG unter festem Namen
const dmg = readdirSync(join(bundle, 'dmg')).find((name) => name.endsWith('.dmg'));
if (!dmg) throw new Error('Kein DMG gefunden — lief `npm run dist:mac` durch?');
copyFileSync(join(bundle, 'dmg', dmg), join(out, 'PrepWell.dmg'));

// 2. Tauri-Updater: Archiv + latest.json
const archiv = 'PrepWell.app.tar.gz';
copyFileSync(join(bundle, 'macos', archiv), join(out, archiv));
const signature = readFileSync(join(bundle, 'macos', `${archiv}.sig`), 'utf8').trim();
const url = `https://github.com/${REPO}/releases/download/v${version}/${archiv}`;
const jetzt = new Date().toISOString();
writeFileSync(
  join(out, 'latest.json'),
  JSON.stringify(
    {
      version,
      pub_date: jetzt,
      // Ein Universal-Binary: beide Architekturen laden dasselbe Archiv.
      platforms: { 'darwin-aarch64': { signature, url }, 'darwin-x86_64': { signature, url } },
    },
    null,
    2,
  ) + '\n',
);

// 3. Brücke für Electron-Installationen: ZIP der signierten, notarisierten App + latest-mac.yml
const zip = `PrepWell-${version}-universal-mac.zip`;
execFileSync('ditto', ['-c', '-k', '--sequesterRsrc', '--keepParent', join(bundle, 'macos', 'PrepWell.app'), join(out, zip)], { stdio: 'inherit' });
const sha512 = createHash('sha512').update(readFileSync(join(out, zip))).digest('base64');
const { size } = statSync(join(out, zip));
writeFileSync(
  join(out, 'latest-mac.yml'),
  [`version: ${version}`, 'files:', `  - url: ${zip}`, `    sha512: ${sha512}`, `    size: ${size}`, `path: ${zip}`, `sha512: ${sha512}`, `releaseDate: '${jetzt}'`, ''].join('\n'),
);

console.log(`Release-Dateien für v${version}:`);
for (const name of readdirSync(out)) console.log(`  ${name}  (${(statSync(join(out, name)).size / 1024 / 1024).toFixed(1)} MB)`);
