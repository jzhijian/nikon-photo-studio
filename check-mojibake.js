const fs = require('fs');
const path = require('path');
const root = 'D:/Code files/nikon/nikon-photo-studio/src';
const files = [];
(function walk(d){ for (const e of fs.readdirSync(d,{withFileTypes:true})) { const p = path.join(d,e.name); if (e.isDirectory()) walk(p); else if (/\.(tsx?|json|css)$/.test(e.name)) files.push(p); } })(root);
// mojibake signatures: chars that result from UTF-8 read as GBK or vice versa
const badRe = /[\u00C2\u00C3\u00E2\u93B7\u93C0\u93D0\u93BB\u93AF\u93B5\u93C3\u93D6\u93E2\u93BA\u93C6\u93D2\u93E0\u951B\u6FE1\u7487\u7586\u5D84\u59AF\u89E6\u93C8\u93D5\u939B\u939C\u939F]/;
let total = 0;
for (const f of files) {
  const t = fs.readFileSync(f, 'utf8');
  const m = t.match(new RegExp(badRe.source, 'g'));
  if (m) { console.log('BAD', f, 'count=' + m.length, 'sample=' + JSON.stringify(m.slice(0,5))); total += m.length; }
}
console.log('total bad chars:', total);
