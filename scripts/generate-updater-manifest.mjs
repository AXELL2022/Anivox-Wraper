import fs from 'node:fs';
import path from 'node:path';

export function findFiles(dir) {
  const results = [];
  if (!fs.existsSync(dir)) return results;
  const list = fs.readdirSync(dir, { withFileTypes: true });
  for (const entry of list) {
    const fullPath = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      results.push(...findFiles(fullPath));
    } else {
      results.push(fullPath);
    }
  }
  return results;
}

export function generateManifest({
  inputDir = 'release-artifacts',
  outputDir = 'dist-release',
  tag = process.env.RELEASE_TAG || 'v0.1.3',
  repo = process.env.REPO_NAME || 'AXELL2022/Anivox-Wraper',
} = {}) {
  const version = tag.replace(/^v/, '');
  fs.mkdirSync(outputDir, { recursive: true });

  const allFiles = findFiles(inputDir);
  console.log(`Found ${allFiles.length} files in ${inputDir}:`, allFiles);

  const copiedFiles = new Map();
  for (const file of allFiles) {
    const baseName = path.basename(file);
    const dest = path.join(outputDir, baseName);
    fs.copyFileSync(file, dest);
    copiedFiles.set(baseName, dest);
  }

  const platforms = {};

  // Search for signatures and attach corresponding bundles
  for (const [baseName, destPath] of copiedFiles.entries()) {
    if (!baseName.endsWith('.sig')) continue;

    const signature = fs.readFileSync(destPath, 'utf8').trim();
    const targetBaseName = baseName.slice(0, -4); // remove .sig

    const downloadUrl = `https://github.com/${repo}/releases/download/${tag}/${targetBaseName}`;

    if (
      baseName.includes('setup.exe.sig') ||
      baseName.includes('nsis.zip.sig') ||
      (baseName.includes('.exe.sig') && !platforms['windows-x86_64']) ||
      (baseName.includes('.msi') && !platforms['windows-x86_64'])
    ) {
      platforms['windows-x86_64'] = {
        signature,
        url: downloadUrl,
      };
      console.log(`Configured windows-x86_64 updater asset: ${targetBaseName}`);
    } else if (
      baseName.includes('AppImage.tar.gz.sig') ||
      baseName.includes('AppImage.sig') ||
      baseName.includes('linux')
    ) {
      platforms['linux-x86_64'] = {
        signature,
        url: downloadUrl,
      };
      console.log(`Configured linux-x86_64 updater asset: ${targetBaseName}`);
    }
  }

  const manifest = {
    version,
    notes: `Release ${tag}`,
    pub_date: new Date().toISOString(),
    platforms,
  };

  const manifestJson = JSON.stringify(manifest, null, 2);
  const outManifestPath = path.join(outputDir, 'latest.json');
  fs.writeFileSync(outManifestPath, manifestJson, 'utf8');
  console.log(`Generated updater manifest at ${outManifestPath}:\n${manifestJson}`);

  return manifest;
}

// Execute directly if run as a script
if (process.argv[1] && path.resolve(process.argv[1]) === path.resolve(import.meta.filename || '')) {
  generateManifest();
}
