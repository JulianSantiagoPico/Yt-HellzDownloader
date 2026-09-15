import { readFile, writeFile } from 'node:fs/promises';

const version = process.argv[2];
if (!version || !/^\d+\.\d+\.\d+(?:-[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?$/.test(version)) {
  throw new Error('Uso: npm run version:set -- <semver>; por ejemplo: npm run version:set -- 0.2.0');
}

const packagePath = new URL('../package.json', import.meta.url);
const tauriPath = new URL('../src-tauri/tauri.conf.json', import.meta.url);
const cargoPath = new URL('../src-tauri/Cargo.toml', import.meta.url);

const packageJson = JSON.parse(await readFile(packagePath, 'utf8'));
packageJson.version = version;
await writeFile(packagePath, `${JSON.stringify(packageJson, null, 2)}\n`);

const tauriConfig = JSON.parse(await readFile(tauriPath, 'utf8'));
tauriConfig.version = version;
await writeFile(tauriPath, `${JSON.stringify(tauriConfig, null, 2)}\n`);

const cargo = await readFile(cargoPath, 'utf8');
const updatedCargo = cargo.replace(/^(\[package\][\s\S]*?^version\s*=\s*)"[^"]+"/m, `$1"${version}"`);
if (updatedCargo === cargo) {
  throw new Error('No se pudo actualizar la versión en src-tauri/Cargo.toml.');
}
await writeFile(cargoPath, updatedCargo);

console.log(`Versión actualizada a ${version} en package.json, tauri.conf.json y Cargo.toml.`);
