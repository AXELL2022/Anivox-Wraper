import fs from 'node:fs';
import path from 'node:path';

export function extractReleaseNotes(changelog, tag) {
  const version = tag.replace(/^v/, '');
  const lines = changelog.replace(/\r\n?/g, '\n').split('\n');
  const escaped = version.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  const heading = new RegExp(`^##[ \\t]+\\[${escaped}\\](?:[ \\t]|$)`);
  const start = lines.findIndex(line => heading.test(line));
  if (start < 0) throw new Error(`Missing changelog entry for ${version}`);
  let end = start + 1;
  while (end < lines.length && !/^##\s/.test(lines[end])) end++;
  const body = lines.slice(start + 1, end).join('\n').replace(/\n?---\s*$/, '').trim();
  if (!body || !body.replace(/[\s*-]/g, '')) throw new Error(`Empty changelog entry for ${version}`);
  return `${lines[start]}\n${body}`;
}

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
  tag = process.env.RELEASE_TAG || 'v0.2.4',
  repo = process.env.REPO_NAME || 'AXELL2022/Anivox-Wraper',
  notesPath = 'release-notes.md',
} = {}) {
  const notes = fs.readFileSync(notesPath, 'utf8');
  if (!notes.trim()) throw new Error('Empty release notes');
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
    notes,
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
  if (process.argv.includes('--extract-notes')) {
    const tag = process.env.RELEASE_TAG;
    if (!tag) throw new Error('RELEASE_TAG is required');
    fs.writeFileSync('release-notes.md', extractReleaseNotes(fs.readFileSync('CHANGELOG.md', 'utf8'), tag) + '\n', 'utf8');
  } else {
    generateManifest();
  }
}
