import { readFile } from 'node:fs/promises';

const files = {
  packageJson: new URL('../package.json', import.meta.url),
  tauriConfig: new URL('../src-tauri/tauri.conf.json', import.meta.url),
  cargoToml: new URL('../src-tauri/Cargo.toml', import.meta.url),
};

const packageVersion = JSON.parse(await readFile(files.packageJson, 'utf8')).version;
const tauriVersion = JSON.parse(await readFile(files.tauriConfig, 'utf8')).version;
const cargo = await readFile(files.cargoToml, 'utf8');
const cargoVersion = cargo.match(/^version\s*=\s*"([^"]+)"\s*$/m)?.[1];

if (!cargoVersion) {
  throw new Error('No se encontró la versión del paquete en src-tauri/Cargo.toml.');
}

const versions = { 'package.json': packageVersion, 'src-tauri/tauri.conf.json': tauriVersion, 'src-tauri/Cargo.toml': cargoVersion };
if (new Set(Object.values(versions)).size !== 1) {
  throw new Error(`Las versiones no coinciden: ${Object.entries(versions).map(([file, version]) => `${file}=${version}`).join(', ')}`);
}

console.log(`Versiones sincronizadas: ${packageVersion}`);
