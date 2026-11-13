function run() {
  const out = {};
  const html = document.documentElement || document.appendChild(document.createElement('html'));
  const n = document.body || html.appendChild(document.createElement('body'));
  const c = document.createElement('p'); n.appendChild(c); c.appendChild(document.createTextNode('abc'));
  const t = document.createElement('i'), t2 = document.createElement('i');
  const f = () => {}; t.addEventListener('x', f);
  const ev = new Event('x');
  const tw = document.createTreeWalker(n);
  const canvas = document.createElement('canvas'); const ctx = canvas.getContext('2d');
  const st = c.style; st.color = 'red';
  const mi = { clientX: 1 };
  let x, s;
  function time(name, N, body) {
    let best = Infinity;
    for (let r = 0; r < 5; r++) {
      const a = performance.now(); body(N); const b = performance.now();
      best = Math.min(best, (b - a) * 1e6 / N);
    }
    out[name] = Math.round(best * 10) / 10;
  }
  const N = 1000000;
  time('empty', N, (N) => { for (let i = 0; i < N; i++) { x = n; } });
  time('nodeType', N, (N) => { for (let i = 0; i < N; i++) { x = n.nodeType; } });
  time('parentNode', N, (N) => { for (let i = 0; i < N; i++) { x = c.parentNode; } });
  time('g document', N, (N) => { for (let i = 0; i < N; i++) { x = document; } });
  time('g navigator', N, (N) => { for (let i = 0; i < N; i++) { x = navigator; } });
  time('g location', N, (N) => { for (let i = 0; i < N; i++) { x = location; } });
  time('g performance', N, (N) => { for (let i = 0; i < N; i++) { x = performance; } });
  time('g innerWidth', N, (N) => { for (let i = 0; i < N; i++) { x = innerWidth; } });
  time('performance.now()', N, (N) => { for (let i = 0; i < N; i++) { x = performance.now(); } });
  time('dispatch none', N, (N) => { for (let i = 0; i < N; i++) { x = t2.dispatchEvent(ev); } });
  time('dispatch one', N, (N) => { for (let i = 0; i < N; i++) { x = t.dispatchEvent(ev); } });
  time('tw.parentNode()', N, (N) => { for (let i = 0; i < N; i++) { x = tw.parentNode(); } });
  time('tw set+nextNode', N, (N) => { for (let i = 0; i < N; i++) { tw.currentNode = n; x = tw.nextNode(); } });
  time('tw set', N, (N) => { for (let i = 0; i < N; i++) { tw.currentNode = n; } });
  time('setAttribute flat', N, (N) => { for (let i = 0; i < N; i++) { n.setAttribute('a', 'value-number-long-1'); } });
  time('concat only', N, (N) => { for (let i = 0; i < N; i++) { s = 'value-number-long-' + (i & 7); } });
  time('setAttribute cons', N, (N) => { for (let i = 0; i < N; i++) { s = 'value-number-long-' + (i & 7); n.setAttribute('a', s); } });
  time('getAttribute flat', N, (N) => { for (let i = 0; i < N; i++) { x = n.getAttribute('id'); } });
  time('concat2 only', N, (N) => { for (let i = 0; i < N; i++) { s = 'data-attribute-name-' + (i & 7); } });
  time('getAttribute cons', N, (N) => { for (let i = 0; i < N; i++) { s = 'data-attribute-name-' + (i & 7); x = n.getAttribute(s); } });
  time('setAttribute non-ascii', N, (N) => { for (let i = 0; i < N; i++) { n.setAttribute('a', 'valué-number-long-1'); } });
  ctx.lineWidth = 1; time('lineWidth 1', N, (N) => { for (let i = 0; i < N; i++) { x = ctx.lineWidth; } });
  ctx.lineWidth = 1.5; time('lineWidth 1.5', N, (N) => { for (let i = 0; i < N; i++) { x = ctx.lineWidth; } });
  time('style.color red', N, (N) => { for (let i = 0; i < N; i++) { x = st.color; } });
  time('c.innerHTML', N, (N) => { for (let i = 0; i < N; i++) { x = c.innerHTML; } });
  time('c.textContent', N, (N) => { for (let i = 0; i < N; i++) { x = c.textContent; } });
  time('ctx.lineCap', N, (N) => { for (let i = 0; i < N; i++) { x = ctx.lineCap; } });
  time('st.cssText', N, (N) => { for (let i = 0; i < N; i++) { x = st.cssText; } });
  time('n.tagName', N, (N) => { for (let i = 0; i < N; i++) { x = n.tagName; } });
  const opt = document.createElement('option'); opt.setAttribute('value', 'v'); opt.textContent = 'some text';
  const link = document.createElement('a'); link.setAttribute('href', '/path?q#f');
  time('document.readyState', N, (N) => { for (let i = 0; i < N; i++) { x = document.readyState; } });
  time('document.compatMode', N, (N) => { for (let i = 0; i < N; i++) { x = document.compatMode; } });
  time('opt.value', N, (N) => { for (let i = 0; i < N; i++) { x = opt.value; } });
  time('opt.text', N, (N) => { for (let i = 0; i < N; i++) { x = opt.text; } });
  time('link.href', N, (N) => { for (let i = 0; i < N; i++) { x = link.href; } });
  time('new Event', N, (N) => { for (let i = 0; i < N; i++) { x = new Event('x'); } });
  time('new MouseEvent init', 200000, (N) => { for (let i = 0; i < N; i++) { x = new MouseEvent('x', mi); } });
  const M = 200000;
  const holder = document.createElement('div');
  let best1 = Infinity, best2 = Infinity;
  for (let r = 0; r < 3; r++) {
    holder.innerHTML = '<i></i>'.repeat(M);
    let a = performance.now(); let cur = holder.firstChild; while (cur) { cur = cur.nextSibling; } let b = performance.now();
    best1 = Math.min(best1, (b - a) * 1e6 / M);
    a = performance.now(); cur = holder.firstChild; while (cur) { cur = cur.nextSibling; } b = performance.now();
    best2 = Math.min(best2, (b - a) * 1e6 / M);
  }
  out['walk new wrappers'] = Math.round(best1 * 10) / 10; out['walk again'] = Math.round(best2 * 10) / 10;
  return JSON.stringify(out);
}
