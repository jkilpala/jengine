// Patch Fengari so a script's numbers are the same in the editor, a web
// build and the native Luau (docs/determinism-proposal.md, phase 3). Nobody has to remember to run it: `npm install` / `npm ci` run
// it (`postinstall`), the editor's Vite config runs it on start (and names
// the patch, so a dependency cache made before it is rebuilt - Vite cannot
// see a file patched in place), and both ways of rebuilding the web
// runtime's `lua-runtime.js` run it before esbuild (`build-lua-runtime.*`,
// `jengine_builder::web`).
//
// The editor imports `fengari-web`, whose `main` is a prebuilt, minified
// bundle with Fengari inside it, so the patch is three exact replacements in
// that file, each of which must match once. Two groups, each applied once
// and marked at the top of the file, so an install patched by the first is
// brought up to date by the second:
//
// **Maths** (`jengine-script-maths-1`):
//
// - `luai_nummod` (a float `%`) was `fmod`-based, as Lua 5.3 defines it; it
//   is Luau's `a - floor(a / b) * b` now, which differs in the last bit for
//   fractional operands (`5.3 % 0.1`). JavaScript never fuses the multiply.
// - `^` in the VM and in constant folding was `Math.pow`, the browser's own
//   (V8's and Safari's differ). It is `jengine_core::scripting::script_math::pow`'s
//   rule: exponents 2, 0.5 and 3 as `a * a`, `sqrt(a)`, `a * a * a`, others
//   through the engine's `libm` (`globalThis.__jengine_script_math.pow`,
//   published by both wasm hosts), and `Math.pow` only with no engine loaded
//   (a unit test).
//
// **Numbers** (`jengine-script-numbers-2`) - Luau has one number type, a
// double; Lua 5.3 has 32-bit integers in Fengari besides, and they wrapped:
//
// - Integer `+ - * % //` and unary `-`, in the VM and in constant folding,
//   are worked out as doubles - Luau's arithmetic - and kept as an integer
//   only when the answer still is one that fits; past 2^31 it is a float,
//   as Luau's is. (`2000000000 + 2000000000` was negative.) `% 0` and
//   `// 0` give NaN and infinity, as Luau's do, rather than an error.
// - A number's text is Luau's (`luai_num2str`): the shortest digits that
//   read back to the same number - JavaScript's own shortest round trip -
//   in fixed notation while the decimal point is within -5 .. 21 places,
//   else `1.5e+21`; never a trailing `.0`, `-0` for negative zero, `inf`,
//   `-inf`, `nan`. Lua 5.3 printed 14 digits and `3.0`. `tostring`,
//   `print`, `..` and `%f` in error messages all come through it.
// - `string.format("%d")` (and `%i %o %u %x %X`) takes any number and drops
//   its fraction, as Luau does; Fengari raised "number has no integer
//   representation", which killed a script's `update` for a number Luau
//   printed. Exact below 2^53; past it the browser prints a double's
//   shortest digits where Luau prints its exact integer.
//
// Still apart: bitwise operators exist only in Lua 5.3 (Luau has `bit32`),
// and a numeric `for` over integers stops at 2^31 - 1.
//
// `math.sin` and the rest are not here: `lua-runtime.ts` replaces them in
// each script's `math` table. A Fengari upgrade that moves any of this fails
// the install loudly rather than shipping unpatched maths.

import { readFileSync, writeFileSync, existsSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const file = resolve(here, '../node_modules/fengari-web/dist/fengari-web.bundle.js');
const MATHS = '/*jengine-script-maths-1*/';
const NUMBERS = '/*jengine-script-numbers-2*/';
/** The newest group's mark: the patch's version, which the Vite plugin's
 *  name carries. */
export const MARK = NUMBERS;

const pow = (a, b) =>
  `(${b}===2?${a}*${a}:${b}===.5?Math.sqrt(${a}):${b}===3?${a}*${a}*${a}:` +
  `globalThis.__jengine_script_math?globalThis.__jengine_script_math.pow(${a},${b}):Math.pow(${a},${b}))`;

const MATHS_REPLACEMENTS = [
  [
    't.exports.luai_nummod=function(t,e,n){let r=e%n;return r*n<0&&(r+=n),r',
    't.exports.luai_nummod=function(t,e,n){return e-Math.floor(e/n)*n',
  ],
  ['setfltvalue(Math.pow(e,n))', `setfltvalue(${pow('e', 'n')})`],
  ['case f:return Math.pow(n,a);', `case f:return ${pow('n', 'a')};`],
];

// An integer result stays an integer only while it is one that fits.
const JNUM = '__jengine_jnum';
const NUMBERS_PRELUDE =
  `function ${JNUM}(o,x){(x|0)===x&&!Object.is(x,-0)?o.setivalue(x):o.setfltvalue(x)}` +
  // Luau's `luai_num2str` (lnumprint.cpp), on JavaScript's shortest digits.
  `function __jengine_num2str(x){if(x!==x)return"nan";if(x===1/0)return"inf";if(x===-1/0)return"-inf";` +
  `if(x===0)return Object.is(x,-0)?"-0":"0";let g=x<0?"-":"",m=Math.abs(x).toExponential().split("e"),` +
  `d=m[0].replace(".","").replace(/0+$/,""),k=+m[1]+1,n=d.length,t;` +
  `if(k>=-5&&k<=21)t=k<=0?"0."+"0".repeat(-k)+d:k>=n?d+"0".repeat(k-n):d.slice(0,k)+"."+d.slice(k);` +
  `else{let e=k-1,a=Math.abs(e);t=d[0]+(n>1?"."+d.slice(1):"")+"e"+(e<0?"-":"+")+(a<10?"0"+a:a)}return g+t}`;

const NUMBERS_REPLACEMENTS = [
  ['t.stack[i].setivalue(r.value+a.value|0)', `${JNUM}(t.stack[i],r.value+a.value)`],
  ['t.stack[i].setivalue(r.value-a.value|0)', `${JNUM}(t.stack[i],r.value-a.value)`],
  ['t.stack[i].setivalue(zt(r.value,a.value))', `${JNUM}(t.stack[i],r.value*a.value)`],
  ['t.stack[i].setivalue(Jt(t,r.value,a.value))', `${JNUM}(t.stack[i],gt(t,r.value,a.value))`],
  ['t.stack[i].setivalue(Yt(t,r.value,a.value))', `${JNUM}(t.stack[i],Math.floor(r.value/a.value))`],
  ['t.stack[i].setivalue(0|-n.value)', `${JNUM}(t.stack[i],-n.value)`],
  // Constant folding (`luaO_arith`): the same, through its float arithmetic.
  ['return void _.setivalue(Ut(t,e,n.value,i.value))', `return void ${JNUM}(_,mt(t,e,n.value,i.value))`],
  // `luaO_tostring`.
  [
    'Ot=function(t,e){let n;if(e.ttisinteger())n=V(W(e.value));else{let t=Q(e.value);!Z&&/^[-0123456789]+$/.test(t)&&(t+=".0"),n=V(t)}e.setsvalue(z(t,n))}',
    'Ot=function(t,e){e.setsvalue(z(t,V(__jengine_num2str(e.value))))}',
  ],
  // `string.format`'s integer conversions.
  ['case"d":case"i":case"o":case"u":case"x":case"X":{let e=et(t,n);', 'case"d":case"i":case"o":case"u":case"x":case"X":{let e=Math.trunc(nt(t,n));'],
];

const GROUPS = [
  { mark: MATHS, prelude: '', replacements: MATHS_REPLACEMENTS, what: '% and ^' },
  { mark: NUMBERS, prelude: NUMBERS_PRELUDE, replacements: NUMBERS_REPLACEMENTS, what: 'numbers past 2^31, number text and %d' },
];

/** Patch the installed `fengari-web` with every group it does not have yet.
 *  Throws if it is missing, or if an upgrade has moved what a group
 *  replaces. Answers whether anything was patched. */
export function patchFengari(log = console.log) {
  if (!existsSync(file)) throw new Error(`[patch-fengari] ${file} not found - run npm install in editor/`);
  let src = readFileSync(file, 'utf8');
  let patched = false;
  for (const group of GROUPS) {
    if (src.includes(group.mark)) continue;
    for (const [from, to] of group.replacements) {
      const count = src.split(from).length - 1;
      if (count !== 1) {
        throw new Error(`[patch-fengari] expected one '${from}' in ${file}, found ${count} - has fengari-web changed?`);
      }
      src = src.replace(from, to);
    }
    // Marks and preludes go first: the bundle is one module, and a function
    // declared at its top is in scope everywhere in it.
    src = group.mark + group.prelude + src;
    log(`[patch-fengari] patched ${group.what} in fengari-web`);
    patched = true;
  }
  if (patched) writeFileSync(file, src);
  return patched;
}

// Run directly (`node scripts/patch-fengari.mjs`, the postinstall).
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    if (!patchFengari()) console.log('[patch-fengari] already patched');
  } catch (e) {
    console.error(e.message);
    process.exit(1);
  }
}
