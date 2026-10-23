// A smoke test of the page in Node: stand-ins for three.js and the DOM, enough to run its
// logic through every case, rule, view, colour and the time scale, without a browser.
//
//   node viz/harness.js viz/tidy3d.html
//
// It checks that every position is finite and every box gets a colour; it does not draw.
const fs = require('fs');
const html = fs.readFileSync(process.argv[2], 'utf8');
const src = html.slice(html.lastIndexOf('<script>') + 8, html.lastIndexOf('</script>'));
const any = () => new Proxy(function () {}, { get: (t, k) => (k === Symbol.toPrimitive ? () => 0 : any()), apply: () => any(), construct: () => any() });
class Attr { constructor(a, n) { this.array = a; this.itemSize = n; } setUsage() {} }
class Geo { constructor() { this.attributes = {}; } setAttribute(k, a) { this.attributes[k] = a; } computeBoundingSphere() {} dispose() {} }
class Obj { constructor(g, m) { this.geometry = g; this.material = m; this.children = []; this.position = { set() {}, x: 0, y: 0, z: 0 }; this.scale = { set() {} }; }
  add(c) { this.children.push(c); } remove() {} traverse(f) { f(this); this.children.forEach(c => c.traverse ? c.traverse(f) : f(c)); } computeLineDistances() {} }
class Color { constructor(s) { this.s = s; } set(s) { this.s = s; return this; } clone() { return new Color(this.s); } lerp() { return this; } }
let colorsSet = 0;
class InstancedMesh extends Obj { constructor(g, m, n) { super(g, m); this.count = n; this.instanceMatrix = new Attr(new Float32Array(16 * n), 16); this.instanceColor = null; }
  setColorAt(i, c) { if (!(c instanceof Color)) throw new Error('setColorAt without a colour at ' + i); this.instanceColor = this.instanceColor || {}; colorsSet++; } dispose() {} }
class Vector3 { constructor(x, y, z) { this.x = x; this.y = y; this.z = z; } project() { this.z = 0.5; this.x = 0; this.y = 0; return this; } }
const THREE = {
  WebGLRenderer: class { constructor() { this.domElement = el(); } setPixelRatio() {} setSize() {} render() {} },
  Scene: class extends Obj {}, Group: class extends Obj {},
  PerspectiveCamera: class extends Obj { updateProjectionMatrix() {} },
  OrbitControls: class { constructor() { this.target = { set() {} }; } update() {} },
  AmbientLight: class extends Obj {}, DirectionalLight: class extends Obj {},
  MeshLambertMaterial: class { constructor() { this.color = new Color(); } },
  LineBasicMaterial: class { constructor(o) { Object.assign(this, o); this.color = new Color(); } },
  LineDashedMaterial: class { constructor(o) { Object.assign(this, o); this.color = new Color(); } },
  BoxGeometry: class extends Geo {},
  EdgesGeometry: class extends Geo { constructor() { super(); this.attributes.position = new Attr(new Float32Array(72), 3); } },
  BufferGeometry: Geo, BufferAttribute: Attr, LineSegments: Obj, Line: Obj, InstancedMesh, Color, Vector3,
  Vector2: class { set() {} }, Raycaster: class { setFromCamera() {} intersectObject() { return [{ instanceId: 0 }]; } },
  DynamicDrawUsage: 0,
};
const els = {};
function el(id) {
  const e = { id, hidden: false, style: {}, children: [], textContent: '', innerHTML: '', value: '0', attrs: {},
    setAttribute(k, v) { this.attrs[k] = v; }, addEventListener(k, f) { (this.on = this.on || {})[k] = f; },
    appendChild(c) { this.children.push(c); }, prepend() {}, getBoundingClientRect: () => ({ left: 0, top: 0, width: 1000, height: 600 }),
    offsetWidth: 200, offsetHeight: 60, className: '' };
  return e;
}
const document = {
  getElementById: id => (els[id] = els[id] || el(id)),
  createElement: () => el(), querySelectorAll: () => els.cases.children,
  documentElement: {},
};
const window = { matchMedia: () => ({ matches: false, addEventListener() {} }), devicePixelRatio: 1 };
const getComputedStyle = () => ({ getPropertyValue: () => '#123456' });
const ResizeObserver = class { observe() {} }, MutationObserver = class { observe() {} };
let raf = null; const requestAnimationFrame = f => (raf = f);
const location = { hash: '' };
const performance = { now: () => clock };
let clock = 0;
const run = new Function('THREE', 'document', 'window', 'getComputedStyle', 'ResizeObserver', 'MutationObserver', 'requestAnimationFrame', 'location', 'performance',
  src + '\n;return { CASES, setCase, setRule, setView, setColour, state, place, figures, legend, drawTicks, describe, extentOf, P: () => P, real: () => real };');
const page = run(THREE, document, window, getComputedStyle, ResizeObserver, MutationObserver, requestAnimationFrame, location, performance);
for (let i = 0; i < page.CASES.length; i++) {
  page.setCase(i);
  for (const rule of ['compact', 'allx', 'alternate']) {
    clock += 10; page.setRule(rule);
    for (const t of [100, 500, 1000, 2000]) { clock += t; raf(clock); }
    const P = page.P();
    if ([...P].some(v => !Number.isFinite(v))) throw new Error('non-finite position in ' + page.CASES[i].key + ' ' + rule);
  }
  for (const v of ['top', 'front', 'orbit']) page.setView(v);
  page.setColour('axis'); page.setColour('kind');
  els.scale.value = '-1'; els.scale.on.input(); raf(clock += 16);
  const c = page.CASES[i], r = page.real();
  const tipText = page.describe(r[r.length - 1]);
  console.log(c.key.padEnd(7), 'nodes', c.nodes.length, 'drawn', r.length, 'figures', (els.figs.innerHTML.match(/<dt>/g) || []).length,
    'legend', (els.legend.innerHTML.match(/class="key"/g) || []).length, 'ticks', els.ticks.children.length, '| tip:', tipText.replace(/<[^>]+>/g, ' ').replace(/\s+/g, ' ').slice(0, 110));
}
console.log('colours set', colorsSet, 'ok');
