const fs = require('fs');
const path = require('path');

function replaceEscapes(str) {
  let result = str.replace(/\\u\{([0-9A-Fa-f]+)\}/g, (m, hex) => {
    try { return String.fromCodePoint(parseInt(hex, 16)); }
    catch(e) { return m; }
  });
  result = result.replace(/\\u([0-9A-Fa-f]{4})/g, (m, hex) => {
    try { return String.fromCharCode(parseInt(hex, 16)); }
    catch(e) { return m; }
  });
  return result;
}

function processFile(fp) {
  const content = fs.readFileSync(fp, 'utf8');
  const fixed = replaceEscapes(content);
  if (fixed !== content) {
    fs.writeFileSync(fp, fixed, 'utf8');
    return true;
  }
  return false;
}

function walk(dir) {
  let fixedFiles = [];
  for (const f of fs.readdirSync(dir, {withFileTypes: true})) {
    const fp = path.join(dir, f.name);
    if (f.isDirectory()) fixedFiles.push(...walk(fp));
    else if (f.name.endsWith('.tsx') || f.name.endsWith('.ts')) {
      if (processFile(fp)) fixedFiles.push(fp);
    }
  }
  return fixedFiles;
}

const fixed = walk('src');
console.log('Fixed ' + fixed.length + ' files:');
fixed.forEach(f => console.log('  ' + f));