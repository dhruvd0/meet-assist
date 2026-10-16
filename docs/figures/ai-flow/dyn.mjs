// Parts rebuilt per frame. Each takes the kit as K so the same source runs in
// the build (Node) and, inlined, in the page.
export const C = {
  REELS: [
    [-170, 2],
    [170, 2],
  ],
  FLANGE_R: 32,
  PACK: [28, 12],
  PACK_K: 0.733,
  ROLLERS: [
    [-138, 50],
    [138, 50],
  ],
  TAPE: { y0: 44.6, y1: 45.4, z0: 11, z1: 21 },
  RUN: [-138, 138],
  HEADS: { me: -124, them: -112 },
  TRACK: { me: 18.4, them: 13.6 },
  WINDOW: [-100, 20],
  PINS: 9,
  PIN_REST: 24.4,
  PIN_DROP: 3.2,
  CAB: { x: -112, y: -52, w: 58, d: 62, h: 54 },
  DRAWER_Z: [40, 28.6, 17.2, 5.8],
  DRAWER_H: 10,
  RODS: { x0: -38, gap: 6, y: -36, z: 12 },
  DIAL_A: [38, -33, 13, 14],
  DIAL_B: [72, -33, 10, 13],
  CARD: { x: 93.5, y: 6.6, w: 27, d: 1.2, z: 2, h: 28, box: 16 },
  RACK: { x: 40, y: -100, w: 104, d: 28, r: 2, h: 46, face: -72 },
  CUBBY: { x0: 44, w: 21, gap: 25, z0: 4, h: 17, row: 21 },
  FOLDER: {
    w: 16,
    d: 20,
    h: 1.4,
    from: [19, -86, 2.2],
    cubby: [1, 0],
    over: -62,
  },
  TRAY: { x: 152, y: -100, w: 50, d: 38, r: 2 },
};

const lerp = (a, b, f) => a + (b - a) * f;

export function tangentPoint(c, r, q, side) {
  const dx = q[0] - c[0];
  const dy = q[1] - c[1];
  const d = Math.hypot(dx, dy);
  const base = Math.atan2(dy, dx);
  const a = Math.acos(Math.min(1, r / d));
  const p1 = [c[0] + r * Math.cos(base + a), c[1] + r * Math.sin(base + a)];
  const p2 = [c[0] + r * Math.cos(base - a), c[1] + r * Math.sin(base - a)];
  return side > 0 ? (p1[0] > p2[0] ? p1 : p2) : p1[0] < p2[0] ? p1 : p2;
}

export function packRadii(C, live, V) {
  const len = V * live;
  return {
    len,
    r: [
      Math.sqrt(C.PACK[0] ** 2 - C.PACK_K * len),
      Math.sqrt(C.PACK[1] ** 2 + C.PACK_K * len),
    ],
  };
}

// Wound tape on both reels, and the two runs from pack to roller.
export function packsSvg(K, P, C, R) {
  const { z0, z1 } = C.TAPE;
  const out = [];
  C.REELS.forEach(([x, y], i) =>
    out.push(
      K.solidSvg(K.cylinder(x, y, R.r[i], z0, z1 - z0, P, 64), {
        tone: "hi",
        crease: "lo",
      }),
    ),
  );
  C.REELS.forEach((c, i) => {
    const roller = C.ROLLERS[i];
    const q = [roller[0], C.TAPE.y0 + 0.4];
    const p = tangentPoint(c, R.r[i] - 0.2, q, i === 0 ? 1 : -1);
    const dx = q[0] - p[0];
    const dy = q[1] - p[1];
    const l = Math.hypot(dx, dy);
    const n = [(-dy / l) * 0.4, (dx / l) * 0.4];
    const ringPts = [
      [p[0] - n[0], p[1] - n[1]],
      [q[0] - n[0], q[1] - n[1]],
      [q[0] + n[0], q[1] + n[1]],
      [p[0] + n[0], p[1] + n[1]],
    ];
    out.push(
      K.solidSvg(K.extrude(ringPts, z0, z1 - z0, P), {
        tone: "hi",
        crease: "none",
      }),
    );
  });
  return out.join("");
}

// The three windows in each top flange, turning with the reel.
export function spokesSvg(K, P, C, R) {
  const out = [];
  C.REELS.forEach(([x, y], i) => {
    const turn = (i === 0 ? -1 : 1) * (R.len / ((C.PACK[0] + C.PACK[1]) / 2));
    for (let w = 0; w < 3; w++) {
      const a = turn + (w * Math.PI * 2) / 3;
      const pts = [];
      for (let s = 0; s <= 10; s++) {
        const t = a - 0.55 + (1.1 * s) / 10;
        pts.push([x + Math.cos(t) * 25, y + Math.sin(t) * 25]);
      }
      for (let s = 10; s >= 0; s--) {
        const t = a - 0.42 + (0.84 * s) / 10;
        pts.push([x + Math.cos(t) * 13.5, y + Math.sin(t) * 13.5]);
      }
      out.push(K.lineSvg(K.onTop(pts, 22.6, P, true), { tone: "mid" }));
    }
  });
  return out.join("");
}

// Transcript lines on the tape face: written at a head, carried right at V.
export function marksSvg(K, P, C, lines, live, V) {
  const inWin = [];
  const old = [];
  const face = C.TAPE.y1;
  for (const l of lines) {
    if (l.t > live) break;
    const x0 = C.HEADS[l.who] + 4 + V * (live - l.t);
    if (x0 > C.RUN[1] - 6) continue;
    const x1 = Math.min(x0 + l.len, C.RUN[1] - 6);
    const z = C.TRACK[l.who];
    const d = K.segment([x0, face, z], [x1, face, z], P);
    (x0 <= C.WINDOW[1] && x0 >= C.WINDOW[0] - 30 ? inWin : old).push(d);
  }
  return (
    K.lineSvg(old.join(""), { tone: "lo", free: true }) +
    K.lineSvg(inWin.join(""), { tone: "hi", free: true })
  );
}

// Four drawers pulled together on every tick: the prompt pastes all four.
export function drawersSvg(K, P, C, s) {
  const filled = [s.filed, s.filed, true, s.nudges > 0];
  const out = [];
  for (let i = 3; i >= 0; i--) {
    const z0 = C.DRAWER_Z[i];
    const pull = 6 * s.press;
    const front = C.CAB.y + C.CAB.d;
    out.push(
      K.solidSvg(
        K.slabOf(
          { x: -108, y: front, w: 50, d: 2.4 + pull, r: 1 },
          z0,
          C.DRAWER_H,
          P,
          3,
          0.4,
        ),
        { tone: "mid" },
      ),
    );
    if (filled[i] && pull > 2) {
      const h = 2.4 * Math.min(1, (pull - 2) / 3);
      out.push(
        K.solidSvg(
          K.slabOf(
            { x: -100 + i * 4, y: front + 0.6, w: 16, d: 0.8, r: 0.2 },
            z0 + C.DRAWER_H,
            h,
            P,
            2,
          ),
          { tone: "hi", crease: "none" },
        ),
      );
    }
    const hy = front + 2.4 + pull;
    out.push(
      K.solidSvg(
        K.slabOf({ x: -87, y: hy, w: 8, d: 1.6, r: 0.8 }, z0 + 4, 2, P, 3),
        { tone: "mid", crease: "none" },
      ),
    );
  }
  return out.join("");
}

export function folderAt(C, f) {
  const F = C.FOLDER;
  const cub = C.CUBBY;
  const tx = cub.x0 + F.cubby[0] * cub.gap + cub.w / 2;
  const tz = cub.z0 + F.cubby[1] * cub.row + 1.6;
  const ty = C.RACK.face - 2 - F.d / 2;
  const [sx, sy, sz] = F.from;
  if (f < 0.3) {
    const g = f / 0.3;
    return [sx, lerp(sy, F.over, g), sz + 10 * Math.sin((g * Math.PI) / 2)];
  }
  if (f < 0.65) {
    const g = (f - 0.3) / 0.35;
    return [lerp(sx, tx, g), F.over, lerp(sz + 10, tz + 3, g)];
  }
  const g = (f - 0.65) / 0.35;
  return [tx, lerp(F.over, ty, g), lerp(tz + 3, tz, Math.min(1, g * 2))];
}

// The meeting's folder: the part in front of the rack face plain, the part
// inside clipped by the cubby opening.
export function folderSvg(K, P, C, f) {
  const F = C.FOLDER;
  const [cx, cy, z] = folderAt(C, f);
  const x = cx - F.w / 2;
  const y0 = cy - F.d / 2;
  const y1 = cy + F.d / 2;
  const face = C.RACK.face;
  const paint = (a, b) =>
    K.solidSvg(K.slabOf({ x, y: a, w: F.w, d: b - a, r: 0.6 }, z, F.h, P, 3), {
      tone: "hi",
    });
  const tab = (a) =>
    K.solidSvg(
      K.slabOf(
        { x: x + 2, y: a - 1.6, w: 6, d: 1.6, r: 0.4 },
        z,
        F.h * 0.6,
        P,
        2,
      ),
      { tone: "hi", crease: "none" },
    );
  if (y0 >= face) return { inside: "", outside: tab(y0) + paint(y0, y1) };
  if (y1 <= face) return { inside: tab(y0) + paint(y0, y1), outside: "" };
  return { inside: tab(y0) + paint(y0, face), outside: paint(face, y1) };
}

const needle = (K, P, cx, cy, r, z, a) =>
  K.lineOnTop(
    [cx - Math.sin(a) * 2.6, cy + Math.cos(a) * 2.6],
    [cx + Math.sin(a) * r, cy - Math.cos(a) * r],
    z,
    P,
  );

export function needlesSvg(K, P, C, s) {
  const [ax, ay, ar, az] = C.DIAL_A;
  const [bx, by, br, bz] = C.DIAL_B;
  return (
    K.lineSvg(
      needle(K, P, ax, ay, ar - 4, az + 0.2, (s.cooldown / 180) * Math.PI * 2),
      { tone: "hi", free: "start" },
    ) +
    K.lineSvg(
      needle(
        K,
        P,
        bx,
        by,
        br - 3.4,
        bz + 0.2,
        (s.sameKind / 600) * Math.PI * 2,
      ),
      { tone: "hi", free: "start" },
    )
  );
}

// One intervention card, built at full height; the page slides it up.
export function cardSvg(K, P, C, card, KINDS) {
  const c = C.CARD;
  const plan = { x: c.x, y: c.y, w: c.w, d: c.d, r: 0.3 };
  let out = K.solidSvg(K.slabOf(plan, c.z, c.h, P, 2), {
    tone: "hi",
    lit: true,
  });
  const k = Math.max(0, KINDS.indexOf(card.kind));
  out += K.solidSvg(
    K.slabOf(
      { x: c.x + 1.5 + k * 6.4, y: c.y, w: 5, d: c.d, r: 0.4 },
      c.z + c.h,
      2.2,
      P,
      2,
    ),
    { tone: "hi", lit: true, crease: "none" },
  );
  const face = c.y + c.d;
  const rows = card.rows
    .map((w, j) =>
      K.segment(
        [c.x + 2.5, face, c.z + c.h - 3 - j * 2.8],
        [c.x + 2.5 + (c.w - 6) * w, face, c.z + c.h - 3 - j * 2.8],
        P,
      ),
    )
    .join("");
  return out + K.lineSvg(rows, { tone: "lit", free: true });
}
