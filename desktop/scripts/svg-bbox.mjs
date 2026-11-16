// Bounding box of SVG path data (absolute/relative M L H V C S Q T A Z), by sampling each segment.
export function pathBBox(d) {
  const toks = d.match(/[MmLlHhVvCcSsQqTtAaZz]|-?(?:\d*\.\d+|\d+\.?)(?:e[-+]?\d+)?/gi) ?? [];
  let i = 0, cmd = "", x = 0, y = 0, sx = 0, sy = 0, cx = 0, cy = 0, qx = 0, qy = 0, prev = "";
  const b = { x0: Infinity, y0: Infinity, x1: -Infinity, y1: -Infinity };
  const add = (px, py) => { b.x0 = Math.min(b.x0, px); b.y0 = Math.min(b.y0, py); b.x1 = Math.max(b.x1, px); b.y1 = Math.max(b.y1, py); };
  const num = () => Number(toks[i++]);
  const N = 48;
  const cubic = (x0, y0, x1, y1, x2, y2, x3, y3) => { for (let k = 1; k <= N; k++) { const t = k / N, u = 1 - t; add(u*u*u*x0 + 3*u*u*t*x1 + 3*u*t*t*x2 + t*t*t*x3, u*u*u*y0 + 3*u*u*t*y1 + 3*u*t*t*y2 + t*t*t*y3); } };
  const quad = (x0, y0, x1, y1, x2, y2) => { for (let k = 1; k <= N; k++) { const t = k / N, u = 1 - t; add(u*u*x0 + 2*u*t*x1 + t*t*x2, u*u*y0 + 2*u*t*y1 + t*t*y2); } };
  const arc = (x1, y1, rx, ry, phi, fa, fs, x2, y2) => {
    // Endpoint to centre parameterisation (SVG spec F.6.5).
    rx = Math.abs(rx); ry = Math.abs(ry);
    if (!rx || !ry) { add(x2, y2); return; }
    const p = (phi * Math.PI) / 180, cp = Math.cos(p), sp = Math.sin(p);
    const dx = (x1 - x2) / 2, dy = (y1 - y2) / 2;
    const x1p = cp * dx + sp * dy, y1p = -sp * dx + cp * dy;
    const l = (x1p * x1p) / (rx * rx) + (y1p * y1p) / (ry * ry);
    if (l > 1) { rx *= Math.sqrt(l); ry *= Math.sqrt(l); }
    const sign = fa === fs ? -1 : 1;
    const num_ = rx*rx*ry*ry - rx*rx*y1p*y1p - ry*ry*x1p*x1p;
    const co = sign * Math.sqrt(Math.max(0, num_ / (rx*rx*y1p*y1p + ry*ry*x1p*x1p)));
    const cxp = (co * rx * y1p) / ry, cyp = (-co * ry * x1p) / rx;
    const ccx = cp * cxp - sp * cyp + (x1 + x2) / 2, ccy = sp * cxp + cp * cyp + (y1 + y2) / 2;
    const ang = (ux, uy, vx, vy) => { const a = Math.atan2(ux * vy - uy * vx, ux * vx + uy * vy); return a; };
    const t1 = ang(1, 0, (x1p - cxp) / rx, (y1p - cyp) / ry);
    let dt = ang((x1p - cxp) / rx, (y1p - cyp) / ry, (-x1p - cxp) / rx, (-y1p - cyp) / ry);
    if (!fs && dt > 0) dt -= 2 * Math.PI; else if (fs && dt < 0) dt += 2 * Math.PI;
    for (let k = 1; k <= N; k++) { const t = t1 + (dt * k) / N; add(ccx + rx * Math.cos(t) * cp - ry * Math.sin(t) * sp, ccy + rx * Math.cos(t) * sp + ry * Math.sin(t) * cp); }
  };
  while (i < toks.length) {
    if (/[a-z]/i.test(toks[i])) cmd = toks[i++];
    const rel = cmd === cmd.toLowerCase() && cmd !== "z" ? true : false;
    const C = cmd.toUpperCase();
    const ox = rel ? x : 0, oy = rel ? y : 0;
    if (C === "Z") { x = sx; y = sy; prev = "Z"; continue; }
    if (C === "M") { x = ox + num(); y = oy + num(); sx = x; sy = y; add(x, y); cmd = rel ? "l" : "L"; prev = "M"; continue; }
    if (C === "L") { x = ox + num(); y = oy + num(); add(x, y); }
    else if (C === "H") { x = ox + num(); add(x, y); }
    else if (C === "V") { y = oy + num(); add(x, y); }
    else if (C === "C") { const x1 = ox + num(), y1 = oy + num(), x2 = ox + num(), y2 = oy + num(), x3 = ox + num(), y3 = oy + num(); cubic(x, y, x1, y1, x2, y2, x3, y3); cx = x2; cy = y2; x = x3; y = y3; }
    else if (C === "S") { const x1 = /[CS]/.test(prev) ? 2 * x - cx : x, y1 = /[CS]/.test(prev) ? 2 * y - cy : y; const x2 = ox + num(), y2 = oy + num(), x3 = ox + num(), y3 = oy + num(); cubic(x, y, x1, y1, x2, y2, x3, y3); cx = x2; cy = y2; x = x3; y = y3; }
    else if (C === "Q") { const x1 = ox + num(), y1 = oy + num(), x2 = ox + num(), y2 = oy + num(); quad(x, y, x1, y1, x2, y2); qx = x1; qy = y1; x = x2; y = y2; }
    else if (C === "T") { const x1 = /[QT]/.test(prev) ? 2 * x - qx : x, y1 = /[QT]/.test(prev) ? 2 * y - qy : y; const x2 = ox + num(), y2 = oy + num(); quad(x, y, x1, y1, x2, y2); qx = x1; qy = y1; x = x2; y = y2; }
    else if (C === "A") { const rx = num(), ry = num(), phi = num(), fa = num(), fs = num(), x2 = ox + num(), y2 = oy + num(); arc(x, y, rx, ry, phi, fa, fs, x2, y2); x = x2; y = y2; }
    else { i++; continue; }
    prev = C;
  }
  return b;
}
