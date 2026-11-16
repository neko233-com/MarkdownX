var Je = Object.defineProperty;
var Ze = (t, e, r) => e in t ? Je(t, e, { enumerable: !0, configurable: !0, writable: !0, value: r }) : t[e] = r;
var A = (t, e, r) => Ze(t, typeof e != "symbol" ? e + "" : e, r);
var Oe = {}, Ke = (function(t, e, r, n, o) {
  var a = new Worker(Oe[e] || (Oe[e] = URL.createObjectURL(new Blob([
    t + ';addEventListener("error",function(e){e=e.error;postMessage({$e$:[e.message,e.code,e.stack]})})'
  ], { type: "text/javascript" }))));
  return a.onmessage = function(l) {
    var f = l.data, i = f.$e$;
    if (i) {
      var c = new Error(i[0]);
      c.code = i[1], c.stack = i[2], o(c, null);
    } else
      o(null, f);
  }, a.postMessage(r, n), a;
}), S = Uint8Array, H = Uint16Array, Se = Int32Array, ge = new S([
  0,
  0,
  0,
  0,
  0,
  0,
  0,
  0,
  1,
  1,
  1,
  1,
  2,
  2,
  2,
  2,
  3,
  3,
  3,
  3,
  4,
  4,
  4,
  4,
  5,
  5,
  5,
  5,
  0,
  /* unused */
  0,
  0,
  /* impossible */
  0
]), me = new S([
  0,
  0,
  0,
  0,
  1,
  1,
  2,
  2,
  3,
  3,
  4,
  4,
  5,
  5,
  6,
  6,
  7,
  7,
  8,
  8,
  9,
  9,
  10,
  10,
  11,
  11,
  12,
  12,
  13,
  13,
  /* unused */
  0,
  0
]), Ne = new S([16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15]), Be = function(t, e) {
  for (var r = new H(31), n = 0; n < 31; ++n)
    r[n] = e += 1 << t[n - 1];
  for (var o = new Se(r[30]), n = 1; n < 30; ++n)
    for (var a = r[n]; a < r[n + 1]; ++a)
      o[a] = a - r[n] << 5 | n;
  return { b: r, r: o };
}, ke = Be(ge, 2), pe = ke.b, Qe = ke.r;
pe[28] = 258, Qe[258] = 28;
var et = Be(me, 0), Re = et.b, ae = new H(32768);
for (var y = 0; y < 32768; ++y) {
  var X = (y & 43690) >> 1 | (y & 21845) << 1;
  X = (X & 52428) >> 2 | (X & 13107) << 2, X = (X & 61680) >> 4 | (X & 3855) << 4, ae[y] = ((X & 65280) >> 8 | (X & 255) << 8) >> 1;
}
var Q = (function(t, e, r) {
  for (var n = t.length, o = 0, a = new H(e); o < n; ++o)
    t[o] && ++a[t[o] - 1];
  var l = new H(e);
  for (o = 1; o < e; ++o)
    l[o] = l[o - 1] + a[o - 1] << 1;
  var f;
  if (r) {
    f = new H(1 << e);
    var i = 15 - e;
    for (o = 0; o < n; ++o)
      if (t[o])
        for (var c = o << 4 | t[o], h = e - t[o], s = l[t[o] - 1]++ << h, u = s | (1 << h) - 1; s <= u; ++s)
          f[ae[s] >> i] = c;
  } else
    for (f = new H(n), o = 0; o < n; ++o)
      t[o] && (f[o] = ae[l[t[o] - 1]++] >> 15 - t[o]);
  return f;
}), te = new S(288);
for (var y = 0; y < 144; ++y)
  te[y] = 8;
for (var y = 144; y < 256; ++y)
  te[y] = 9;
for (var y = 256; y < 280; ++y)
  te[y] = 7;
for (var y = 280; y < 288; ++y)
  te[y] = 8;
var $e = new S(32);
for (var y = 0; y < 32; ++y)
  $e[y] = 5;
var Ue = /* @__PURE__ */ Q(te, 9, 1), ze = /* @__PURE__ */ Q($e, 5, 1), ne = function(t) {
  for (var e = t[0], r = 1; r < t.length; ++r)
    t[r] > e && (e = t[r]);
  return e;
}, B = function(t, e, r) {
  var n = e / 8 | 0;
  return (t[n] | t[n + 1] << 8) >> (e & 7) & r;
}, oe = function(t, e) {
  var r = e / 8 | 0;
  return (t[r] | t[r + 1] << 8 | t[r + 2] << 16) >> (e & 7);
}, Le = function(t) {
  return (t + 7) / 8 | 0;
}, se = function(t, e, r) {
  return (e == null || e < 0) && (e = 0), (r == null || r > t.length) && (r = t.length), new S(t.subarray(e, r));
}, _e = [
  "unexpected EOF",
  "invalid block type",
  "invalid length/literal",
  "invalid distance",
  "stream finished",
  "no stream handler",
  ,
  // determined by compression function
  "no callback",
  "invalid UTF-8 data",
  "extra field too long",
  "date not in range 1980-2099",
  "filename too long",
  "stream finishing",
  "invalid zip data"
  // determined by unknown compression method
], M = function(t, e, r) {
  var n = new Error(e || _e[t]);
  if (n.code = t, Error.captureStackTrace && Error.captureStackTrace(n, M), !r)
    throw n;
  return n;
}, je = function(t, e, r, n) {
  var o = t.length, a = n ? n.length : 0;
  if (!o || e.f && !e.l)
    return r || new S(0);
  var l = !r, f = l || e.i != 2, i = e.i;
  l && (r = new S(o * 3));
  var c = function(Ee) {
    var De = r.length;
    if (Ee > De) {
      var xe = new S(Math.max(De * 2, Ee));
      xe.set(r), r = xe;
    }
  }, h = e.f || 0, s = e.p || 0, u = e.b || 0, g = e.l, p = e.d, m = e.m, w = e.n, v = o * 8;
  do {
    if (!g) {
      h = B(t, s, 1);
      var O = B(t, s + 1, 3);
      if (s += 3, O)
        if (O == 1)
          g = Ue, p = ze, m = 9, w = 5;
        else if (O == 2) {
          var C = B(t, s, 31) + 257, Z = B(t, s + 10, 15) + 4, P = C + B(t, s + 5, 31) + 1;
          s += 14;
          for (var T = new S(P), F = new S(19), b = 0; b < Z; ++b)
            F[Ne[b]] = B(t, s + b * 3, 7);
          s += Z * 3;
          for (var x = ne(F), N = (1 << x) - 1, _ = Q(F, x, 1), b = 0; b < P; ) {
            var V = _[B(t, s, N)];
            s += V & 15;
            var E = V >> 4;
            if (E < 16)
              T[b++] = E;
            else {
              var R = 0, q = 0;
              for (E == 16 ? (q = 3 + B(t, s, 3), s += 2, R = T[b - 1]) : E == 17 ? (q = 3 + B(t, s, 7), s += 3) : E == 18 && (q = 11 + B(t, s, 127), s += 7); q--; )
                T[b++] = R;
            }
          }
          var j = T.subarray(0, C), W = T.subarray(C);
          m = ne(j), w = ne(W), g = Q(j, m, 1), p = Q(W, w, 1);
        } else
          M(1);
      else {
        var E = Le(s) + 4, D = t[E - 4] | t[E - 3] << 8, d = E + D;
        if (d > o) {
          i && M(0);
          break;
        }
        f && c(u + D), r.set(t.subarray(E, d), u), e.b = u += D, e.p = s = d * 8, e.f = h;
        continue;
      }
      if (s > v) {
        i && M(0);
        break;
      }
    }
    f && c(u + 131072);
    for (var Ve = (1 << m) - 1, Ye = (1 << w) - 1, fe = s; ; fe = s) {
      var R = g[oe(t, s) & Ve], K = R >> 4;
      if (s += R & 15, s > v) {
        i && M(0);
        break;
      }
      if (R || M(2), K < 256)
        r[u++] = K;
      else if (K == 256) {
        fe = s, g = null;
        break;
      } else {
        var be = K - 254;
        if (K > 264) {
          var b = K - 257, ee = ge[b];
          be = B(t, s, (1 << ee) - 1) + pe[b], s += ee;
        }
        var ce = p[oe(t, s) & Ye], le = ce >> 4;
        ce || M(3), s += ce & 15;
        var W = Re[le];
        if (le > 3) {
          var ee = me[le];
          W += oe(t, s) & (1 << ee) - 1, s += ee;
        }
        if (s > v) {
          i && M(0);
          break;
        }
        f && c(u + 131072);
        var Ae = u + be;
        if (u < W) {
          var ye = a - W, He = Math.min(W, Ae);
          for (ye + u < 0 && M(3); u < He; ++u)
            r[u] = n[ye + u];
        }
        for (; u < Ae; ++u)
          r[u] = r[u - W];
      }
    }
    e.l = g, e.p = fe, e.b = u, e.f = h, g && (h = 1, e.m = m, e.d = p, e.n = w);
  } while (!h);
  return u != r.length && l ? se(r, 0, u) : r.subarray(0, u);
}, tt = /* @__PURE__ */ new S(0), rt = function(t, e) {
  var r = {};
  for (var n in t)
    r[n] = t[n];
  for (var n in e)
    r[n] = e[n];
  return r;
}, Ce = function(t, e, r) {
  for (var n = t(), o = t.toString(), a = o.slice(o.indexOf("[") + 1, o.lastIndexOf("]")).replace(/\s+/g, "").split(","), l = 0; l < n.length; ++l) {
    var f = n[l], i = a[l];
    if (typeof f == "function") {
      e += ";" + i + "=";
      var c = f.toString();
      if (f.prototype)
        if (c.indexOf("[native code]") != -1) {
          var h = c.indexOf(" ", 8) + 1;
          e += c.slice(h, c.indexOf("(", h));
        } else {
          e += c;
          for (var s in f.prototype)
            e += ";" + i + ".prototype." + s + "=" + f.prototype[s].toString();
        }
      else
        e += c;
    } else
      r[i] = f;
  }
  return e;
}, re = [], nt = function(t) {
  var e = [];
  for (var r in t)
    t[r].buffer && e.push((t[r] = new t[r].constructor(t[r])).buffer);
  return e;
}, ot = function(t, e, r, n) {
  if (!re[r]) {
    for (var o = "", a = {}, l = t.length - 1, f = 0; f < l; ++f)
      o = Ce(t[f], o, a);
    re[r] = { c: Ce(t[l], o, a), e: a };
  }
  var i = rt({}, re[r].e);
  return Ke(re[r].c + ";onmessage=function(e){for(var k in e.data)self[k]=e.data[k];onmessage=" + e.toString() + "}", r, i, nt(i), n);
}, it = function() {
  return [S, H, Se, ge, me, Ne, pe, Re, Ue, ze, ae, _e, Q, ne, B, oe, Le, se, M, je, ve, Ge, qe];
}, Ge = function(t) {
  return postMessage(t, [t.buffer]);
}, qe = function(t) {
  return t && {
    out: t.size && new S(t.size),
    dictionary: t.dictionary
  };
}, at = function(t, e, r, n, o, a) {
  var l = ot(r, n, o, function(f, i) {
    l.terminate(), a(f, i);
  });
  return l.postMessage([t, e], e.consume ? [t.buffer] : []), function() {
    l.terminate();
  };
}, G = function(t, e) {
  return t[e] | t[e + 1] << 8;
}, k = function(t, e) {
  return (t[e] | t[e + 1] << 8 | t[e + 2] << 16 | t[e + 3] << 24) >>> 0;
}, ue = function(t, e) {
  return k(t, e) + k(t, e + 4) * 4294967296;
};
function st(t, e, r) {
  return r || (r = e, e = {}), typeof r != "function" && M(7), at(t, e, [
    it
  ], function(n) {
    return Ge(ve(n.data[0], qe(n.data[1])));
  }, 1, r);
}
function ve(t, e) {
  return je(t, { i: 2 }, e && e.out, e && e.dictionary);
}
var de = typeof TextDecoder < "u" && /* @__PURE__ */ new TextDecoder(), ft = 0;
try {
  de.decode(tt, { stream: !0 }), ft = 1;
} catch {
}
var ct = function(t) {
  for (var e = "", r = 0; ; ) {
    var n = t[r++], o = (n > 127) + (n > 223) + (n > 239);
    if (r + o > t.length)
      return { s: e, r: se(t, r - 1) };
    o ? o == 3 ? (n = ((n & 15) << 18 | (t[r++] & 63) << 12 | (t[r++] & 63) << 6 | t[r++] & 63) - 65536, e += String.fromCharCode(55296 | n >> 10, 56320 | n & 1023)) : o & 1 ? e += String.fromCharCode((n & 31) << 6 | t[r++] & 63) : e += String.fromCharCode((n & 15) << 12 | (t[r++] & 63) << 6 | t[r++] & 63) : e += String.fromCharCode(n);
  }
};
function lt(t, e) {
  if (e) {
    for (var r = "", n = 0; n < t.length; n += 16384)
      r += String.fromCharCode.apply(null, t.subarray(n, n + 16384));
    return r;
  } else {
    if (de)
      return de.decode(t);
    var o = ct(t), a = o.s, r = o.r;
    return r.length && M(8), a;
  }
}
var ut = function(t, e) {
  return e + 30 + G(t, e + 26) + G(t, e + 28);
}, ht = function(t, e, r) {
  var n = G(t, e + 28), o = G(t, e + 30), a = lt(t.subarray(e + 46, e + 46 + n), !(G(t, e + 8) & 2048)), l = e + 46 + n, f = dt(t, l, o, r, k(t, e + 20), k(t, e + 24), k(t, e + 42)), i = f[0], c = f[1], h = f[2];
  return [G(t, e + 10), i, c, a, l + o + G(t, e + 32), h];
}, dt = function(t, e, r, n, o, a, l) {
  var f = o == 4294967295, i = a == 4294967295, c = l == 4294967295, h = e + r, s = f + i + c;
  if (n && s) {
    for (; e + 4 < h; e += 4 + G(t, e + 2))
      if (G(t, e) == 1)
        return [
          f ? ue(t, e + 4 + 8 * i) : o,
          i ? ue(t, e + 4) : a,
          c ? ue(t, e + 4 + 8 * (i + f)) : l,
          1
        ];
    n < 2 && M(13);
  }
  return [o, a, l, 0];
}, Pe = typeof queueMicrotask == "function" ? queueMicrotask : typeof setTimeout == "function" ? setTimeout : function(t) {
  t();
};
function gt(t, e, r) {
  r || (r = e, e = {}), typeof r != "function" && M(7);
  var n = [], o = function() {
    for (var w = 0; w < n.length; ++w)
      n[w]();
  }, a = {}, l = function(w, v) {
    Pe(function() {
      r(w, v);
    });
  };
  Pe(function() {
    l = r;
  });
  for (var f = t.length - 22; k(t, f) != 101010256; --f)
    if (!f || t.length - f > 65558)
      return l(M(13, 0, 1), null), o;
  var i = G(t, f + 8);
  if (i) {
    var c = i, h = k(t, f + 16), s = k(t, f - 20) == 117853008;
    if (s) {
      var u = k(t, f - 12);
      s = k(t, u) == 101075792, s && (c = i = k(t, u + 32), h = k(t, u + 48));
    }
    for (var g = e && e.filter, p = function(w) {
      var v = ht(t, h, s), O = v[0], E = v[1], D = v[2], d = v[3], C = v[4], Z = v[5], P = ut(t, Z);
      h = C;
      var T = function(b, x) {
        b ? (o(), l(b, null)) : (x && (a[d] = x), --i || l(null, a));
      };
      if (!g || g({
        name: d,
        size: E,
        originalSize: D,
        compression: O
      }))
        if (!O)
          T(null, se(t, P, P + E));
        else if (O == 8) {
          var F = t.subarray(P, P + E);
          if (D < 524288 || E > 0.8 * D)
            try {
              T(null, ve(F, { out: new S(D) }));
            } catch (b) {
              T(b, null);
            }
          else
            n.push(st(F, { size: D }, T));
        } else
          T(M(14, "unknown compression type " + O, 1), null);
      else
        T(null, null);
    }, m = 0; m < c; ++m)
      p(m);
  } else
    l(null, {});
  return o;
}
const I = (t, e) => Array.from(t.children).filter((r) => !e || r.localName === e), z = (t, e) => I(t, e)[0], Y = (t, e) => Array.from(t.getElementsByTagNameNS("*", e)), L = (t, e) => {
  var r, n;
  return ((n = (r = z(t, e)) == null ? void 0 : r.textContent) == null ? void 0 : n.trim()) ?? "";
};
function J(t) {
  if (!(t != null && t.trim())) return [];
  const e = t.trim().split(/[\s,]+/).map(Number);
  if (e.some((r) => !Number.isFinite(r))) throw new Error(`Invalid OFD number: ${t}`);
  return e;
}
function $(t, e, r = 0) {
  const n = t.getAttribute(e);
  if (n === null) return r;
  const o = Number(n);
  if (!Number.isFinite(o)) throw new Error(`Invalid ${e}: ${n}`);
  return o;
}
function we(t) {
  const e = J(t);
  if (e.length !== 4 || e[2] < 0 || e[3] < 0) throw new Error(`Invalid OFD box: ${t}`);
  return e;
}
function Fe(t, e) {
  const r = J(t);
  if (!r.length && e) return [...e];
  if (r.length !== 4 || r[2] <= 0 || r[3] <= 0) throw new Error(`Invalid OFD box: ${t}`);
  return r;
}
function U(t, e) {
  const r = e.replace(/\\/g, "/").startsWith("/") ? [] : t.split("/").slice(0, -1);
  for (const n of e.replace(/\\/g, "/").split("/"))
    if (!(!n || n === "."))
      if (n === "..") {
        if (!r.length) throw new Error("OFD path escapes archive");
        r.pop();
      } else r.push(n);
  return r.join("/");
}
function mt(t, e) {
  if (/<!DOCTYPE|<!ENTITY/i.test(t)) throw new Error(`Unsafe XML in ${e}`);
  const r = new DOMParser().parseFromString(t, "application/xml");
  if (r.getElementsByTagName("parsererror").length) throw new Error(`Invalid XML: ${e}`);
  return r.documentElement;
}
function Ie(t) {
  const e = (t == null ? void 0 : t.trim().split(/\s+/).filter(Boolean)) ?? [], r = [];
  for (let n = 0; n < e.length; n++) {
    if (e[n] === "g") {
      const o = Number(e[++n]), a = Number(e[++n]);
      if (!Number.isInteger(o) || o < 0 || o > 1e5 || !Number.isFinite(a)) throw new Error("Invalid OFD delta sequence");
      for (let l = 0; l < o; l++) r.push(a);
    } else {
      const o = Number(e[n]);
      if (!Number.isFinite(o)) throw new Error("Invalid OFD delta");
      r.push(o);
    }
    if (r.length > 1e5) throw new Error("OFD delta limit exceeded");
  }
  return r;
}
function pt(t) {
  return J(t == null ? void 0 : t.replace(/#([0-9a-f]+)/gi, (e, r) => String(parseInt(r, 16))));
}
function ie(t) {
  t == null || t.throwIfAborted();
}
class vt {
  constructor(e) {
    A(this, "xmlCache", /* @__PURE__ */ new Map());
    this.files = e;
  }
  has(e) {
    return Object.prototype.hasOwnProperty.call(this.files, e);
  }
  bytes(e) {
    const r = this.files[e];
    if (!r) throw new Error(`OFD resource not found: ${e}`);
    return r;
  }
  xml(e) {
    let r = this.xmlCache.get(e);
    return r || (r = mt(new TextDecoder().decode(this.bytes(e)), e), this.xmlCache.set(e, r)), r;
  }
  clear() {
    this.files = {}, this.xmlCache.clear();
  }
}
async function wt(t, e) {
  var f;
  const { signal: r, maxFileSize: n = 64 * 1024 * 1024, maxUncompressedSize: o = 256 * 1024 * 1024, maxEntries: a = 1e4 } = e;
  ie(r);
  let l;
  if (typeof t == "string" || t instanceof URL) {
    const i = await fetch(t, { signal: r });
    if (!i.ok) throw new Error(`OFD download failed: HTTP ${i.status}`);
    if (Number(i.headers.get("content-length")) > n)
      throw await ((f = i.body) == null ? void 0 : f.cancel()), new Error("OFD file size limit exceeded");
    if (!i.body) throw new Error("OFD response has no body");
    const c = i.body.getReader(), h = [];
    let s = 0;
    try {
      for (; ; ) {
        ie(r);
        const { done: g, value: p } = await c.read();
        if (g) break;
        if (s += p.length, s > n) throw new Error("OFD file size limit exceeded");
        h.push(p);
      }
    } finally {
      await c.cancel(), c.releaseLock();
    }
    l = new Uint8Array(s);
    let u = 0;
    for (const g of h)
      l.set(g, u), u += g.length;
  } else if (t instanceof Blob) {
    if (t.size > n) throw new Error("OFD file size limit exceeded");
    l = new Uint8Array(await t.arrayBuffer());
  } else l = t instanceof Uint8Array ? t : new Uint8Array(t);
  if (l.length > n) throw new Error("OFD file size limit exceeded");
  return ie(r), new Promise((i, c) => {
    let h = 0, s = 0, u;
    const g = () => {
      p(), c((r == null ? void 0 : r.reason) ?? new DOMException("Aborted", "AbortError"));
    }, p = gt(l, { filter(m) {
      return h += m.originalSize, s++, (h > o || s > a) && (u = new Error("OFD archive expansion limit exceeded")), !u;
    } }, (m, w) => {
      if (r == null || r.removeEventListener("abort", g), r != null && r.aborted) return c(r.reason);
      if (m || u) return c(u ?? m);
      try {
        const v = /* @__PURE__ */ Object.create(null);
        for (const [O, E] of Object.entries(w)) {
          if (O.endsWith("/")) continue;
          const D = U("", O);
          if (v[D]) throw new Error(`Duplicate OFD entry: ${D}`);
          v[D] = E;
        }
        i(new vt(v));
      } catch (v) {
        c(v);
      }
    });
    r == null || r.addEventListener("abort", g, { once: !0 });
  });
}
function We(t, e) {
  var c;
  const r = e.match(/[A-Za-z]|[-+]?(?:\d*\.\d+|\d+\.?\d*)(?:[eE][-+]?\d+)?/g) ?? [];
  let n = 0, o = 0, a = 0, l = 0, f = 0;
  const i = () => {
    const h = Number(r[n++]);
    if (!Number.isFinite(h)) throw new Error("Invalid OFD path operand");
    return h;
  };
  for ((c = t.beginPath) == null || c.call(t); n < r.length; ) {
    const h = r[n++];
    switch (h) {
      case "S":
      case "M":
        o = i(), a = i(), l = o, f = a, t.moveTo(o, a);
        break;
      case "L":
        o = i(), a = i(), t.lineTo(o, a);
        break;
      case "Q": {
        const s = i(), u = i();
        o = i(), a = i(), t.quadraticCurveTo(s, u, o, a);
        break;
      }
      case "B": {
        const s = i(), u = i(), g = i(), p = i();
        o = i(), a = i(), t.bezierCurveTo(s, u, g, p, o, a);
        break;
      }
      case "A": {
        const s = i(), u = i(), g = i(), p = i(), m = i(), w = i(), v = i();
        bt(t, o, a, s, u, g, !!p, !!m, w, v), o = w, a = v;
        break;
      }
      case "C":
        t.closePath(), o = l, a = f;
        break;
      default:
        throw new Error(`Unsupported OFD path command: ${h}`);
    }
  }
}
function bt(t, e, r, n, o, a, l, f, i, c) {
  if (n = Math.abs(n), o = Math.abs(o), e === i && r === c) return;
  if (!n || !o) {
    t.lineTo(i, c);
    return;
  }
  const h = a * Math.PI / 180, s = Math.cos(h), u = Math.sin(h), g = s * (e - i) / 2 + u * (r - c) / 2, p = -u * (e - i) / 2 + s * (r - c) / 2, m = g * g / (n * n) + p * p / (o * o);
  m > 1 && (n *= Math.sqrt(m), o *= Math.sqrt(m));
  const w = (l === f ? -1 : 1) * Math.sqrt(Math.max(0, (n * n * o * o - n * n * p * p - o * o * g * g) / (n * n * p * p + o * o * g * g))), v = w * n * p / o, O = -w * o * g / n, E = s * v - u * O + (e + i) / 2, D = u * v + s * O + (r + c) / 2, d = Math.atan2((p - O) / o, (g - v) / n);
  let C = Math.atan2((-p - O) / o, (-g - v) / n) - d;
  !f && C > 0 && (C -= 2 * Math.PI), f && C < 0 && (C += 2 * Math.PI), t.ellipse(E, D, n, o, h, d, d + C, !f);
}
const he = /* @__PURE__ */ new WeakSet();
function At(t, e, r) {
  var l, f;
  const n = new AbortController(), o = () => {
    var i;
    return n.abort((i = e.signal) == null ? void 0 : i.reason);
  };
  return (l = e.signal) == null || l.addEventListener("abort", o, { once: !0 }), (f = e.signal) != null && f.aborted && o(), { promise: (async () => {
    const { canvasContext: i, viewport: c, pixelRatio: h = globalThis.devicePixelRatio || 1, background: s = "#fff", maxCanvasPixels: u = 32e6 } = e;
    if (![c.width, c.height, c.unit, h].every((v) => Number.isFinite(v) && v > 0)) throw new Error("Invalid viewport or pixel ratio");
    const g = Math.ceil(c.width * h), p = Math.ceil(c.height * h);
    if (g * p > u) throw new Error("Canvas pixel limit exceeded; reduce scale or pixelRatio");
    const m = n.signal, w = i.canvas;
    if (m.throwIfAborted(), he.has(w)) throw new Error("Canvas is already rendering; await the previous render task");
    he.add(w);
    try {
      const v = /* @__PURE__ */ new Map(), O = /* @__PURE__ */ new Map();
      for (const { root: E, resources: D } of t.contents)
        for (const d of [E, ...Array.from(E.getElementsByTagName("*"))])
          if (m.throwIfAborted(), d.localName === "TextObject" && (v.set(d, await D.font(d.getAttribute("Font") || "")), m.throwIfAborted()), d.localName === "ImageObject") {
            try {
              O.set(d, await D.image(d.getAttribute("ResourceID") || ""));
            } catch (C) {
              m.throwIfAborted(), r("IMAGE_DECODE", `Image ${d.getAttribute("ResourceID")} could not be decoded (PNG/JPEG/WebP supported): ${String(C)}`);
            }
            m.throwIfAborted();
          }
      m.throwIfAborted(), w.width = g, w.height = p, w.style.width = `${c.width}px`, w.style.height = `${c.height}px`, i.save();
      try {
        i.fillStyle = s, i.fillRect(0, 0, g, p), i.scale(h, h), c.rotation === 90 ? (i.translate(c.width, 0), i.rotate(Math.PI / 2)) : c.rotation === 180 ? (i.translate(c.width, c.height), i.rotate(Math.PI)) : c.rotation === 270 && (i.translate(0, c.height), i.rotate(-Math.PI / 2)), i.scale(c.unit, c.unit), i.translate(-t.physicalBox[0], -t.physicalBox[1]);
        let E = 0;
        const D = async (d, C, Z = []) => {
          if (m.throwIfAborted(), ++E % 100 === 0 && (await new Promise((F) => setTimeout(F, 0)), m.throwIfAborted()), d.getAttribute("Visible") === "false") return;
          const P = d.localName;
          if (["Area", "Template", "PageRes", "Actions"].includes(P)) return;
          if (["CompositeObject", "VideoObject"].includes(P)) {
            r(`UNSUPPORTED_${P}`, `${P} is not supported.`);
            return;
          }
          const T = [...Z, ...Xe(C, d.getAttribute("DrawParam")), d];
          i.save();
          try {
            if (d.hasAttribute("Boundary") && (P.endsWith("Object") || P === "Appearance")) {
              const [b, x, N, _] = we(d.getAttribute("Boundary"));
              i.translate(b, x), P === "TextObject" && (N === 0 || _ === 0) ? r("ZERO_TEXT_BOUNDARY", "Text with a zero-size Boundary is rendered without boundary clipping; its extent may differ from the source.") : (i.beginPath(), i.rect(0, 0, N, _), i.clip());
            }
            const F = J(d.getAttribute("CTM"));
            if (F.length) {
              if (F.length !== 6) throw new Error("Invalid CTM");
              i.transform(F[0], F[1], F[2], F[3], F[4], F[5]);
            }
            if (d.hasAttribute("Alpha") && (i.globalAlpha *= $(d, "Alpha", 255) / 255), yt(i, T, C, r), Et(i, d, r), P === "TextObject") {
              z(d, "CGTransform") && r("GLYPH_TRANSFORM", "Explicit glyph substitution is not supported."), ($(d, "ReadDirection") || $(d, "CharDirection")) && r("TEXT_DIRECTION", "Non-default text directions are not supported.");
              const b = $(d, "Size", 3.5);
              i.font = `${d.getAttribute("Italic") === "true" ? "italic " : ""}${d.getAttribute("Weight") || (d.getAttribute("Bold") === "true" ? "700" : "400")} ${b}px ${v.get(d) || "sans-serif"}`, i.textBaseline = "alphabetic";
              let x = 0, N = 0;
              for (const _ of I(d, "TextCode")) {
                x = $(_, "X", x), N = $(_, "Y", N);
                const V = Ie(_.getAttribute("DeltaX")), R = Ie(_.getAttribute("DeltaY")), q = Array.from(_.textContent || "");
                for (let j = 0; j < q.length; j++)
                  i.save(), i.translate(x, N), i.scale($(d, "HScale", 1), 1), d.getAttribute("Fill") !== "false" && i.fillText(q[j], 0, 0), d.getAttribute("Stroke") === "true" && i.strokeText(q[j], 0, 0), i.restore(), x += V[j] ?? (V.length ? V[V.length - 1] : i.measureText(q[j]).width * $(d, "HScale", 1)), N += R[j] ?? (R.length ? R[R.length - 1] : 0);
              }
            } else if (P === "PathObject")
              We(i, L(d, "AbbreviatedData")), d.getAttribute("Fill") === "true" && i.fill(d.getAttribute("Rule") === "Even-Odd" ? "evenodd" : "nonzero"), d.getAttribute("Stroke") !== "false" && i.stroke();
            else if (P === "ImageObject") {
              const b = O.get(d);
              b && i.drawImage(b, 0, 0, 1, 1), (d.hasAttribute("Mask") || d.hasAttribute("Substitution")) && r("IMAGE_MASK", "Image masks and substitutions are not supported.");
            } else {
              let b = I(d);
              P === "Content" && (b = b.map((x, N) => ({ node: x, index: N })).sort((x, N) => Me(x.node) - Me(N.node) || x.index - N.index).map((x) => x.node));
              for (const x of b) await D(x, C, T);
            }
          } finally {
            i.restore();
          }
        };
        for (const { root: d, resources: C } of t.contents) await D(d, C);
      } finally {
        i.restore();
      }
    } finally {
      he.delete(w);
    }
  })().finally(() => {
    var i;
    return (i = e.signal) == null ? void 0 : i.removeEventListener("abort", o);
  }), cancel: () => n.abort(new DOMException("Rendering cancelled", "AbortError")) };
}
function Me(t) {
  return t.getAttribute("Type") === "Background" ? 0 : t.getAttribute("Type") === "Foreground" ? 2 : 1;
}
function Xe(t, e, r = /* @__PURE__ */ new Set()) {
  if (!e) return [];
  if (r.has(e)) throw new Error("Circular DrawParam reference");
  r.add(e);
  const n = t.getParam(e);
  return n ? [...Xe(t, n.getAttribute("Relative"), r), n] : [];
}
function yt(t, e, r, n) {
  const o = (f) => {
    var i;
    return (i = [...e].reverse().find((c) => c.hasAttribute(f))) == null ? void 0 : i.getAttribute(f);
  }, a = (f) => {
    var p;
    const i = [...e].reverse().map((m) => z(m, f)).find(Boolean);
    if (!i) return "#000";
    I(i).length && n("COLOR_EFFECT", "Gradient and pattern colors are not supported.");
    const c = r.getColorSpace(i.getAttribute("ColorSpace") || "");
    c != null && c.hasAttribute("Profile") && n("ICC_PROFILE", "ICC color profiles are not applied.");
    let h = i.getAttribute("Value");
    if (i.hasAttribute("Index")) {
      const m = c && z(c, "Palette");
      h = m ? ((p = I(m, "CV")[$(i, "Index")]) == null ? void 0 : p.textContent) ?? null : null, h || n("COLOR_INDEX", "A palette color could not be resolved.");
    }
    const s = c ? $(c, "BitsPerComponent", 8) : 8, u = pt(h).map((m) => m * 255 / (2 ** s - 1));
    let g;
    return u.length === 1 ? g = [u[0], u[0], u[0]] : u.length === 4 ? g = u.slice(0, 3).map((m) => 255 * (1 - m / 255) * (1 - u[3] / 255)) : g = u.length === 3 ? u : [0, 0, 0], `rgba(${g.map((m) => Math.min(255, Math.max(0, m))).join(",")},${$(i, "Alpha", 255) / 255})`;
  };
  t.fillStyle = a("FillColor"), t.strokeStyle = a("StrokeColor");
  const l = Number(o("LineWidth") ?? 0.353);
  t.lineWidth = Number.isFinite(l) && l > 0 ? l : 0.01, t.lineCap = { Butt: "butt", Round: "round", Square: "square" }[o("Cap")] || "butt", t.lineJoin = { Miter: "miter", Round: "round", Bevel: "bevel" }[o("Join")] || "miter", t.miterLimit = Number(o("MiterLimit") || 10), t.setLineDash(J(o("DashPattern"))), t.lineDashOffset = Number(o("DashOffset") || 0);
}
function Et(t, e, r) {
  const n = z(e, "Clips");
  if (n)
    for (const o of I(n, "Clip")) {
      const a = new Path2D();
      let l = !1, f = "nonzero";
      for (const i of I(o, "Area")) {
        const c = new DOMMatrix(), h = J(i.getAttribute("CTM"));
        h.length === 6 && c.multiplySelf(new DOMMatrix(h));
        for (const s of I(i)) {
          if (s.localName !== "Path") {
            r("TEXT_CLIP", "Text clipping shapes are not supported.");
            continue;
          }
          const u = new DOMMatrix(Array.from(c.toFloat64Array()));
          if (s.hasAttribute("Boundary")) {
            const [m, w] = we(s.getAttribute("Boundary"));
            u.translateSelf(m, w);
          }
          const g = J(s.getAttribute("CTM"));
          g.length === 6 && u.multiplySelf(new DOMMatrix(g));
          const p = new Path2D();
          We(p, L(s, "AbbreviatedData")), a.addPath(p, u), l = !0, s.getAttribute("Rule") === "Even-Odd" && (f = "evenodd");
        }
      }
      l && t.clip(a, f);
    }
}
let Dt = 0;
class Te {
  constructor(e, r, n) {
    A(this, "colorSpaces", /* @__PURE__ */ new Map());
    A(this, "fonts", /* @__PURE__ */ new Map());
    A(this, "images", /* @__PURE__ */ new Map());
    A(this, "drawParams", /* @__PURE__ */ new Map());
    A(this, "fontPaths", /* @__PURE__ */ new Map());
    A(this, "loadedFonts", /* @__PURE__ */ new Map());
    A(this, "loadedImages", /* @__PURE__ */ new Map());
    A(this, "faces", []);
    A(this, "disposed", !1);
    A(this, "namespace", `ofd_${++Dt}`);
    this.archive = e, this.warn = r, this.parent = n;
  }
  load(e) {
    if (!this.archive.has(e)) {
      this.warn("MISSING_RESOURCE", `Referenced resource file is absent: ${e}`);
      return;
    }
    const r = this.archive.xml(e), n = U(e, `${r.getAttribute("BaseLoc") || "."}/__resource__`);
    for (const o of Y(r, "Font")) {
      const a = o.getAttribute("ID");
      this.fonts.set(a, o), L(o, "FontFile") && this.fontPaths.set(a, U(n, L(o, "FontFile")));
    }
    for (const o of Y(r, "MultiMedia"))
      o.getAttribute("Type") === "Image" && this.images.set(o.getAttribute("ID"), U(n, L(o, "MediaFile")));
    for (const o of Y(r, "DrawParam")) this.drawParams.set(o.getAttribute("ID"), o);
    for (const o of Y(r, "ColorSpace")) this.colorSpaces.set(o.getAttribute("ID"), o);
    for (const o of ["CompositeGraphicUnit"]) Y(r, o).length && this.warn(`RESOURCE_${o.toUpperCase()}`, `${o} resources may require unsupported rendering features.`);
  }
  getColorSpace(e) {
    var r;
    return this.colorSpaces.get(e) ?? ((r = this.parent) == null ? void 0 : r.getColorSpace(e));
  }
  getParam(e) {
    var r;
    return this.drawParams.get(e) ?? ((r = this.parent) == null ? void 0 : r.getParam(e));
  }
  font(e) {
    if (!this.fonts.has(e) && this.parent) return this.parent.font(e);
    let r = this.loadedFonts.get(e);
    return r || (r = (async () => {
      const n = this.fonts.get(e), o = this.fontPaths.get(e), a = JSON.stringify((n == null ? void 0 : n.getAttribute("FamilyName")) || (n == null ? void 0 : n.getAttribute("FontName")) || "sans-serif") + ", sans-serif";
      if (!o)
        return this.warn("FONT_SUBSTITUTION", "Some fonts are not embedded; text uses locally available fonts."), a;
      try {
        const l = `${this.namespace}_${e.replace(/[^a-z0-9]/gi, "_")}`, f = await new FontFace(l, this.archive.bytes(o).slice().buffer).load();
        return this.disposed ? a : (document.fonts.add(f), this.faces.push(f), l);
      } catch {
        return this.warn("FONT_DECODE", `Unable to load embedded font ${e}; using system font.`), a;
      }
    })(), this.loadedFonts.set(e, r), r);
  }
  image(e) {
    if (!this.images.has(e) && this.parent) return this.parent.image(e);
    let r = this.loadedImages.get(e);
    return r || (r = (async () => {
      const n = this.images.get(e);
      if (!n) throw new Error(`Missing image resource: ${e}`);
      const o = this.archive.bytes(n).slice(), a = await createImageBitmap(new Blob([o.buffer]));
      if (this.disposed)
        throw a.close(), new Error("OFD document destroyed");
      return a;
    })(), this.loadedImages.set(e, r), r);
  }
  destroy() {
    this.disposed = !0;
    for (const e of this.faces) document.fonts.delete(e);
    for (const e of this.loadedImages.values()) e.then((r) => r.close(), () => {
    });
    this.loadedImages.clear(), this.loadedFonts.clear();
  }
}
class xt {
  constructor(e, r, n, o, a) {
    this.owner = e, this.pageNumber = r, this.id = n, this.physicalBox = o, this.contents = a;
  }
  getViewport({ scale: e = 1, rotation: r = 0 } = {}) {
    if (this.owner.assertAlive(), !Number.isFinite(e) || e <= 0) throw new Error("Scale must be positive");
    if (!Number.isFinite(r) || r % 90) throw new Error("Rotation must be a multiple of 90");
    r = (r % 360 + 360) % 360;
    const n = 96 / 25.4 * e, o = this.physicalBox[2] * n, a = this.physicalBox[3] * n;
    return { width: r % 180 ? a : o, height: r % 180 ? o : a, scale: e, rotation: r, unit: n };
  }
  render(e) {
    return this.owner.assertAlive(), this.owner.track(At(this, e, this.owner.warn));
  }
  async getTextContent() {
    return this.owner.assertAlive(), { items: this.contents.flatMap(({ root: e }) => Y(e, "TextObject").map((r) => ({ text: Y(r, "TextCode").map((n) => n.textContent || "").join(""), boundary: we(r.getAttribute("Boundary")), font: r.getAttribute("Font") || "", size: Number(r.getAttribute("Size")) }))) };
  }
}
class Ot {
  constructor(e, r, n) {
    A(this, "diagnostics", []);
    A(this, "metadata", {});
    A(this, "numPages");
    A(this, "pages");
    A(this, "pageCache", /* @__PURE__ */ new Map());
    A(this, "resources");
    A(this, "scopes", []);
    A(this, "templates", /* @__PURE__ */ new Map());
    A(this, "tasks", /* @__PURE__ */ new Set());
    A(this, "destroyed", !1);
    A(this, "physicalBox");
    A(this, "annotationPath");
    A(this, "warn", (e, r) => {
      this.diagnostics.some((n) => n.code === e && n.message === r) || this.diagnostics.push({ code: e, message: r });
    });
    this.archive = e, this.path = r;
    const o = e.xml(r), a = z(o, "CommonData");
    if (!a) throw new Error("Missing OFD CommonData");
    const l = z(a, "PageArea");
    l ? this.physicalBox = Fe(L(l, "PhysicalBox")) : this.warn("MISSING_DEFAULT_PAGE_AREA", "Document has no default PageArea; each page must supply its own PhysicalBox."), this.resources = new Te(e, this.warn), this.scopes.push(this.resources);
    for (const f of ["PublicRes", "DocumentRes"]) for (const i of I(a, f)) this.resources.load(U(r, i.textContent.trim()));
    for (const f of I(a, "TemplatePage")) this.templates.set(f.getAttribute("ID"), f);
    if (this.pages = I(z(o, "Pages") ?? o, "Page"), this.numPages = this.pages.length, !this.numPages) throw new Error("OFD document contains no pages");
    if (this.annotationPath = L(o, "Annotations"), n) for (const f of I(n)) this.metadata[f.localName] = f.textContent ?? "";
  }
  assertAlive() {
    if (this.destroyed) throw new Error("OFD document destroyed");
  }
  track(e) {
    return this.tasks.add(e), e.promise.then(() => this.tasks.delete(e), () => this.tasks.delete(e)), e;
  }
  async getPage(e) {
    if (this.assertAlive(), !Number.isInteger(e) || e < 1 || e > this.numPages) throw new RangeError(`Page must be between 1 and ${this.numPages}`);
    let r = this.pageCache.get(e);
    return r || (r = this.parsePage(e), this.pageCache.set(e, r)), r;
  }
  scope(e, r) {
    const n = new Te(this.archive, this.warn, this.resources);
    this.scopes.push(n);
    for (const o of I(e, "PageRes")) n.load(U(r, o.textContent.trim()));
    return n;
  }
  async parsePage(e) {
    const r = this.pages[e - 1], n = U(this.path, r.getAttribute("BaseLoc") || ""), o = this.archive.xml(n), a = [], l = [];
    for (const c of I(o, "Template")) {
      const h = this.templates.get(c.getAttribute("TemplateID"));
      if (!h) {
        this.warn("MISSING_TEMPLATE", "A referenced template page is missing.");
        continue;
      }
      const s = U(this.path, h.getAttribute("BaseLoc")), u = this.archive.xml(s), g = { root: u, resources: this.scope(u, s) };
      ((c.getAttribute("ZOrder") || h.getAttribute("ZOrder")) === "Foreground" ? l : a).push(g);
    }
    if (a.push({ root: o, resources: this.scope(o, n) }, ...l), this.annotationPath) {
      const c = U(this.path, this.annotationPath), h = this.archive.xml(c);
      for (const s of I(h, "Page")) if (s.getAttribute("PageID") === r.getAttribute("ID")) {
        const u = U(c, L(s, "FileLoc")), g = this.archive.xml(u);
        for (const p of I(g, "Annot")) if (p.getAttribute("Visible") !== "false") {
          const m = z(p, "Appearance");
          m && a.push({ root: m, resources: this.resources });
        }
      }
    }
    const f = z(o, "Area"), i = f && L(f, "PhysicalBox");
    if (!i && !this.physicalBox) throw new Error(`Page ${e} has no PhysicalBox and the document has no default PageArea`);
    return new xt(this, e, r.getAttribute("ID") || "", Fe(i, this.physicalBox), a);
  }
  destroy() {
    if (!this.destroyed) {
      this.destroyed = !0;
      for (const e of this.tasks) e.cancel();
      for (const e of this.scopes) e.destroy();
      this.pageCache.clear(), this.archive.clear();
    }
  }
}
async function Pt(t, e = {}) {
  const r = await wt(t, e);
  try {
    ie(e.signal);
    const n = r.xml("OFD.xml");
    if (n.localName !== "OFD") throw new Error("Invalid OFD root");
    const o = I(n, "DocBody"), a = e.documentIndex ?? 0;
    if (!Number.isInteger(a) || a < 0 || !o[a]) throw new Error("OFD document index out of range");
    const l = o[a], f = new Ot(r, U("OFD.xml", L(l, "DocRoot")), z(l, "DocInfo"));
    return L(l, "Signatures") && f.warn("SIGNATURE_UNSUPPORTED", "Digital signatures and embedded seals are not rendered or verified."), o.length > 1 && f.warn("MULTI_DOCUMENT", `Archive has ${o.length} documents; selected index ${a}.`), f;
  } catch (n) {
    throw r.clear(), n;
  }
}
export {
  Ot as OFDDocument,
  xt as OFDPage,
  Pt as getDocument
};
//# sourceMappingURL=ofdjs.js.map
