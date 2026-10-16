// Fig: the Meet Assist observer as a transcript deck.
//   node docs/figures/ai-flow/build.mjs [--light]
// Needs the anatomy skill kit at ~/.claude/skills/anatomy.
import { readFileSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const SKILL = join(homedir(), ".claude/skills/anatomy");
const k = await import(join(SKILL, "kit/iso-kit.mjs"));
const G = await import(join(SKILL, "kit/lathe.mjs"));
const { kitScript } = await import(join(SKILL, "scripts/inline-kit.mjs"));
const HERE = dirname(fileURLToPath(import.meta.url));
import { M, LINES, KINDS, timeline, stateAt } from "./model.mjs";
import {
  C,
  packRadii,
  packsSvg,
  spokesSvg,
  marksSvg,
  drawersSvg,
  folderSvg,
  needlesSvg,
  cardSvg,
} from "./dyn.mjs";

const theme = process.argv.includes("--light") ? "light" : "dark";
const WIDTH = 640;
const HEIGHT = 392;

const PLATE = { x: -214, y: -110, w: 430, d: 174, r: 10 };
const PLATE_H = 12;
const FOOT_H = 5;

const P = k.fitProjection(
  [
    ...k.boxCorners(PLATE, -PLATE_H - FOOT_H, 0),
    [C.RACK.x, C.RACK.y, C.RACK.h + 4],
    [C.CAB.x, C.CAB.y, C.CAB.h + 2],
    [C.CARD.x, C.CARD.y, 46],
  ],
  WIDTH,
  HEIGHT,
  { pad: 18, azimuth: 56, elevation: 34 },
);

const S = (paths, style) => k.solidSvg(paths, style);
const L = (d, style) => k.lineSvg(d, style);
const D = (points, style) => k.dotsSvg(points, style);
const at = (p) => k.iso(p, P);
const byDepth = (pts) =>
  [...pts].sort(
    (a, b) => k.depthOf([a[0], a[1], 0], P) - k.depthOf([b[0], b[1], 0], P),
  );
const screws = (plan, inset, z, size = 0.45) =>
  D(
    k.corners(plan, inset).map(([x, y]) => at([x, y, z])),
    { size },
  );
const poly = (pts, cls, extra = "") =>
  `<path class="${cls}" ${extra} d="${k.pathOf(pts.map(at), true)}"/>`;

const T = timeline();
const S0 = stateAt(0, T);

// ---------- base ----------
function plate() {
  const out = [
    `<path class="iso-halo" d="${k.haloOf(PLATE, -PLATE_H - FOOT_H, PLATE_H, P)}"/>`,
  ];
  for (const [x, y] of byDepth(k.corners(PLATE, 26))) {
    out.push(
      S(k.cylinder(x, y, 8, -PLATE_H - FOOT_H, 1.6, P, 28), { tone: "lo" }),
    );
    out.push(
      S(k.cylinder(x, y, 6.2, -PLATE_H - FOOT_H + 1.6, FOOT_H - 1.6, P, 28), {
        tone: "mid",
      }),
    );
  }
  out.push(S(k.slabOf(PLATE, -PLATE_H, PLATE_H, P, 8, 1.6), { tone: "mid" }));
  out.push(L(k.planOutline(k.insetPlan(PLATE, 5), 0, P), { tone: "lo" }));
  const sc = k.corners(PLATE, 11);
  out.push(
    L(sc.map(([x, y]) => k.ring(x, y, 2.4, 0, P, 16)).join(""), { tone: "lo" }),
  );
  out.push(
    D(
      sc.map(([x, y]) => at([x, y, 0])),
      { size: 0.5 },
    ),
  );
  return out.join("");
}

// ---------- back row: project rack, unfiled tray, summary tray ----------
function rack() {
  const R = C.RACK;
  const cub = C.CUBBY;
  const out = [
    `<path class="iso-halo" d="${k.haloOf(R, 0, R.h, P)}" style="filter:blur(5px)"/>`,
  ];
  out.push(
    S(
      k.slabOf(
        { x: R.x - 2, y: R.y - 2, w: R.w + 4, d: R.d + 4, r: 2 },
        0,
        2.4,
        P,
        3,
        0.4,
      ),
      { tone: "lo" },
    ),
  );
  out.push(
    S(k.slabOf(R, 2.4, R.h - 2.4, P, 3, 0.8), {
      tone: "mid",
      inner: L(k.sideSeam(R, R.h - 3, P, 3), { tone: "faint" }),
    }),
  );
  out.push(screws(R, 3.5, R.h));
  for (const x of [R.x + 30, R.x + R.w - 34])
    out.push(
      S(k.slabOf({ x, y: R.y + 11, w: 4, d: 6, r: 1 }, R.h, 5, P, 3, 0.3), {
        tone: "mid",
      }),
    );
  out.push(
    S(
      k.slabOf(
        { x: R.x + 30, y: R.y + 12, w: R.w - 60, d: 4, r: 1.8 },
        R.h + 5,
        2.4,
        P,
        4,
        0.3,
      ),
      { tone: "mid" },
    ),
  );
  // Existing projects: folders already filed, one per meeting.
  const stock = { "0,0": 2, "1,0": 1, "2,0": 1, "0,1": 1, "2,1": 2 };
  const defs = [];
  for (let row = 0; row < 2; row++)
    for (let col = 0; col < 4; col++) {
      const x0 = cub.x0 + col * cub.gap;
      const x1 = x0 + cub.w;
      const z0 = cub.z0 + row * cub.row;
      const z1 = z0 + cub.h;
      const f = R.face;
      const b = R.y + 3;
      const id = `cubby-${col}-${row}`;
      const open = [
        [x0, f, z0],
        [x1, f, z0],
        [x1, f, z1],
        [x0, f, z1],
      ];
      defs.push(
        `<clipPath id="${id}"><path d="${k.pathOf(open.map(at), true)}"/></clipPath>`,
      );
      const inner = [];
      inner.push(
        poly(
          [
            [x0, b, z0],
            [x1, b, z0],
            [x1, b, z1],
            [x0, b, z1],
          ],
          "iso-shade",
          'data-shade="0"',
        ),
      );
      inner.push(
        poly(
          [
            [x0, f, z1],
            [x1, f, z1],
            [x1, b, z1],
            [x0, b, z1],
          ],
          "iso-shade",
          'data-shade="0"',
        ),
      );
      inner.push(
        poly(
          [
            [x0, f, z0],
            [x0, b, z0],
            [x0, b, z1],
            [x0, f, z1],
          ],
          "iso-shade",
          'data-shade="1"',
        ),
      );
      inner.push(
        poly(
          [
            [x0, f, z0],
            [x1, f, z0],
            [x1, b, z0],
            [x0, b, z0],
          ],
          "iso-top",
        ),
      );
      inner.push(
        L(
          k.segment([x0, b, z0], [x1, b, z0], P) +
            k.segment([x0, b, z0], [x0, b, z1], P) +
            k.segment([x0, b, z1], [x0, f, z1], P),
          { tone: "faint" },
        ),
      );
      const n = stock[`${col},${row}`] || 0;
      for (let j = 0; j < n; j++) {
        const fx = x0 + 2.5 + j * 0.8;
        const fz = z0 + j * 1.6;
        inner.push(
          S(
            k.slabOf({ x: fx, y: f - 22, w: 16, d: 20, r: 0.6 }, fz, 1.4, P, 3),
            { tone: "lo" },
          ),
        );
        inner.push(
          S(
            k.slabOf(
              { x: fx + 2 + j * 7, y: f - 3.6, w: 6, d: 1.6, r: 0.4 },
              fz,
              0.9,
              P,
              2,
            ),
            { tone: "lo", crease: "none" },
          ),
        );
      }
      out.push(`<g clip-path="url(#${id})">${inner.join("")}</g>`);
      out.push(L(k.pathOf(open.map(at), true), { tone: "mid" }));
      // A label holder under each opening.
      out.push(
        S(
          k.slabOf(
            { x: x0 + 6, y: f, w: 9, d: 0.8, r: 0.2 },
            z0 - 3.2,
            2.4,
            P,
            2,
          ),
          { tone: "lo", crease: "none" },
        ),
      );
    }
  // The cubby for this meeting's project gets its label line.
  const tx = cub.x0 + C.FOLDER.cubby[0] * cub.gap + 7.5;
  out.push(
    L(
      k.segment(
        [tx, R.face + 0.8, cub.z0 - 2],
        [tx + 6, R.face + 0.8, cub.z0 - 2],
        P,
      ),
      { tone: "hi", free: true },
    ),
  );
  return `<defs>${defs.join("")}</defs>${out.join("")}`;
}

function unfiledTray() {
  const plan = { x: 4, y: -100, w: 30, d: 28, r: 2 };
  const out = [S(k.slabOf(plan, 0, 1.2, P, 3, 0.4), { tone: "lo" })];
  for (const [x, y, w, d] of [
    [4, -100, 30, 1.6],
    [4, -100, 1.6, 28],
    [32.4, -100, 1.6, 28],
  ])
    out.push(
      S(k.slabOf({ x, y, w, d, r: 0.5 }, 1.2, 3.4, P, 2), { tone: "mid" }),
    );
  out.push(
    S(k.slabOf({ x: 4, y: -73.6, w: 30, d: 1.6, r: 0.5 }, 1.2, 1.8, P, 2), {
      tone: "mid",
    }),
  );
  return out.join("");
}

function summaryTray() {
  const t = C.TRAY;
  const out = [S(k.slabOf(t, 0, 2, P, 3, 0.5), { tone: "lo" })];
  for (const [x, y, w, d] of [
    [t.x, t.y, t.w, 2],
    [t.x, t.y, 2, t.d],
    [t.x + t.w - 2, t.y, 2, t.d],
  ])
    out.push(
      S(k.slabOf({ x, y, w, d, r: 0.6 }, 2, 6, P, 2, 0.3), { tone: "mid" }),
    );
  out.push(
    S(
      k.slabOf(
        { x: t.x, y: t.y + t.d - 2, w: t.w, d: 2, r: 0.6 },
        2,
        3,
        P,
        2,
        0.3,
      ),
      { tone: "mid" },
    ),
  );
  out.push(
    D([at([t.x + 1, t.y + 1, 8]), at([t.x + t.w - 1, t.y + 1, 8])], {
      size: 0.45,
    }),
  );
  return out.join("");
}

// Two summary sheets drop in at the end: the session summary, then the
// rolling summary it is merged into.
function sheet(i) {
  const t = C.TRAY;
  const plan = {
    x: t.x + 5 + i * 1.5,
    y: t.y + 4 + i * 1.5,
    w: 38,
    d: 28,
    r: 0.5,
  };
  const z = 2 + i * 1.1;
  const rows = [0.9, 0.7, 0.8, 0.5]
    .map((w, j) =>
      k.lineOnTop(
        [plan.x + 4, plan.y + 5 + j * 4.5],
        [plan.x + 4 + 28 * w, plan.y + 5 + j * 4.5],
        z + 1,
        P,
      ),
    )
    .join("");
  return (
    S(k.slabOf(plan, z, 1, P, 2), { tone: i ? "hi" : "mid" }) +
    L(rows, { tone: "lo", free: true })
  );
}

// ---------- context cabinet ----------
function cabinet() {
  const c = C.CAB;
  const plan = { x: c.x, y: c.y, w: c.w, d: c.d, r: 2.5 };
  const front = c.y + c.d;
  const out = [
    `<path class="iso-halo" d="${k.haloOf(plan, 0, c.h, P)}" style="filter:blur(5px)"/>`,
  ];
  out.push(
    S(k.slabOf(k.insetPlan(plan, -1.5), 0, 3, P, 4, 0.5), { tone: "lo" }),
  );
  out.push(S(k.slabOf(plan, 3, c.h - 3, P, 4, 1), { tone: "mid" }));
  const cap = k.insetPlan(plan, -1);
  out.push(S(k.slabOf(cap, c.h, 2, P, 4, 0.6), { tone: "mid" }));
  out.push(
    L(k.planOutline(k.insetPlan(cap, 4), c.h + 2, P), { tone: "faint" }),
  );
  out.push(screws(cap, 2.5, c.h + 2));
  // Drawer openings on the face, one per drawer.
  const holes = C.DRAWER_Z.map((z) =>
    k.pathOf(
      [
        [-109, front, z - 0.7],
        [-56, front, z - 0.7],
        [-56, front, z + C.DRAWER_H + 0.7],
        [-109, front, z + C.DRAWER_H + 0.7],
      ].map(at),
      true,
    ),
  ).join("");
  out.push(L(holes, { tone: "lo" }));
  return out.join("");
}

// ---------- tool rods ----------
function toolBlock() {
  const r = C.RODS;
  const plan = { x: r.x0 - 6, y: r.y - 6, w: 36, d: 12, r: 2 };
  const out = [
    S(k.slabOf(plan, 0, r.z, P, 3, 0.6), {
      tone: "mid",
      inner: L(k.sideSeam(plan, 4, P, 3), { tone: "faint" }),
    }),
  ];
  for (let i = 0; i < 5; i++)
    out.push(L(k.ring(r.x0 + i * r.gap, r.y, 2.2, r.z, P, 18), { tone: "lo" }));
  out.push(
    D(
      [
        at([plan.x + 2, plan.y + 2, r.z]),
        at([plan.x + plan.w - 2, plan.y + 2, r.z]),
      ],
      { size: 0.45 },
    ),
  );
  return out.join("");
}

function rod(i) {
  const r = C.RODS;
  const x = r.x0 + i * r.gap;
  return (
    S(k.cylinder(x, r.y, 1.3, r.z, 20, P, 16), {
      tone: "mid",
      crease: "none",
    }) +
    S(k.cylinder(x, r.y, 2.4, r.z + 20, 3, P, 20), { tone: "hi" }) +
    L(k.knurl(x, r.y, 2.4, r.z + 20, r.z + 23, 16, P), { tone: "lo" })
  );
}

// ---------- cooldown dials ----------
function dials() {
  const plan = { x: 20, y: -50, w: 70, d: 34, r: 3 };
  const out = [
    `<path class="iso-halo" d="${k.haloOf(plan, 0, 8, P)}" style="filter:blur(4px)"/>`,
  ];
  out.push(
    S(k.slabOf(plan, 0, 8, P, 4, 0.8), {
      tone: "mid",
      inner: L(k.sideSeam(plan, 3, P, 4), { tone: "faint" }),
    }),
  );
  out.push(screws(plan, 3, 8));
  for (const [x, y, r, top, count, every] of [
    [...C.DIAL_A, 18, 6],
    [...C.DIAL_B, 20, 2],
  ]) {
    out.push(
      S(k.cylinder(x, y, r, 8, top - 8, P, 64, 0.6), {
        tone: "hi",
        crease: "lo",
      }),
    );
    out.push(
      L(k.knurl(x, y, r, top - 2.4, top, Math.round(r * 5), P, 0.3), {
        tone: "lo",
      }),
    );
    out.push(L(k.sideArc(x, y, r, top - 2.4, P, 40), { tone: "faint" }));
    out.push(L(k.ring(x, y, r - 1.8, top, P, 64), { tone: "lo" }));
    const t = k.radialTicks(x, y, r - 2.4, count, every, top, [1, 2.2], P);
    out.push(L(t.minor, { tone: "lo" }), L(t.major, { tone: "mid" }));
    out.push(L(k.ring(x, y, 1.4, top, P, 16), { tone: "mid" }));
  }
  // Crown on dial A.
  out.push(
    S(
      k.slabOf(
        {
          x: C.DIAL_A[0] - 2,
          y: C.DIAL_A[1] + C.DIAL_A[2] - 0.6,
          w: 4,
          d: 3.4,
          r: 1,
        },
        9,
        3.4,
        P,
        3,
      ),
      { tone: "mid" },
    ),
  );
  return out.join("");
}

// ---------- desk bell ----------
const BELL = [118, -36];
function bell() {
  const [x, y] = BELL;
  const out = [S(k.cylinder(x, y, 10, 0, 3, P, 48, 0.5), { tone: "mid" })];
  out.push(L(k.sideArc(x, y, 10, 1.4, P, 40), { tone: "faint" }));
  const F = G.frameAlong([x, y, 3], [0, 0, 1]);
  const prof = [
    [0, 9.2],
    [0.6, 9.2, 1],
    [2.2, 8.8, 1],
    [4.2, 7.6, 1],
    [6.2, 5.6, 1],
    [7.6, 3.2, 1],
    [8.3, 1.2, 1],
    [8.4, 0],
  ];
  out.push(S(G.solidOf(prof, F, P), { tone: "hi" }));
  return out.join("");
}
function plunger() {
  const [x, y] = BELL;
  return (
    S(k.cylinder(x, y, 0.8, 11, 4, P, 14), { tone: "mid", crease: "none" }) +
    S(k.cylinder(x, y, 2, 15, 1.4, P, 20), { tone: "hi" })
  );
}

// ---------- card dispenser ----------
function cardBox() {
  const c = C.CARD;
  const plan = { x: 86, y: -6, w: 42, d: 26, r: 2.5 };
  const out = [
    `<path class="iso-halo" d="${k.haloOf(plan, 0, c.box, P)}" style="filter:blur(4px)"/>`,
  ];
  out.push(
    S(k.slabOf(plan, 0, c.box, P, 4, 0.8), {
      tone: "mid",
      inner: L(k.sideSeam(plan, 4, P, 4), { tone: "faint" }),
    }),
  );
  out.push(
    L(
      k.planOutline(
        { x: c.x - 1, y: c.y - 0.6, w: c.w + 2, d: c.d + 1.2, r: 0.8 },
        c.box,
        P,
        3,
      ),
      { tone: "mid" },
    ),
  );
  out.push(screws(plan, 3, c.box));
  return out.join("");
}
function cardClip() {
  const c = C.CARD;
  const f = c.y + c.d;
  return k.pathOf(
    [
      [c.x - 2, f, c.box],
      [c.x + c.w + 2, f, c.box],
      [c.x + c.w + 2, f, 90],
      [c.x - 2, f, 90],
    ].map(at),
    true,
  );
}

// ---------- tape deck ----------
function reelBase(i) {
  const [x, y] = C.REELS[i];
  const out = [
    `<path class="iso-halo" d="${k.haloOf({ x: x - 30, y: y - 30, w: 60, d: 60, r: 30 }, 0, 10, P)}" style="filter:blur(6px)"/>`,
  ];
  out.push(S(k.cylinder(x, y, 14, 0, 2, P, 48, 0.5), { tone: "lo" }));
  out.push(S(k.cylinder(x, y, 9, 2, 7.6, P, 40), { tone: "mid" }));
  out.push(L(k.sideArc(x, y, 9, 5, P, 32), { tone: "faint" }));
  out.push(
    S(k.cylinder(x, y, C.FLANGE_R, 9.6, 1.4, P, 96, 0.4), { tone: "mid" }),
  );
  out.push(
    D(
      k.circleRing(x, y, 12, 6).map(([a, b]) => at([a, b, 2])),
      { size: 0.4 },
    ),
  );
  return out.join("");
}
function reelTop(i) {
  const [x, y] = C.REELS[i];
  const out = [
    S(k.cylinder(x, y, C.FLANGE_R, 21.2, 1.4, P, 96, 0.4), { tone: "mid" }),
  ];
  out.push(L(k.ring(x, y, C.FLANGE_R - 2.4, 22.6, P, 96), { tone: "faint" }));
  out.push(S(k.cylinder(x, y, 7, 22.6, 3, P, 40, 0.4), { tone: "mid" }));
  out.push(S(k.cylinder(x, y, 3.2, 25.6, 2.4, P, 24), { tone: "hi" }));
  out.push(L(k.knurl(x, y, 7, 22.6, 25.6, 30, P), { tone: "lo" }));
  return out.join("");
}

function heads() {
  const out = [];
  const plan = { x: -131, y: 31, w: 26, d: 11, r: 1.5 };
  out.push(S(k.slabOf(plan, 0, 4, P, 3, 0.5), { tone: "lo" }));
  out.push(screws(plan, 2, 4));
  for (const who of ["me", "them"]) {
    const x = C.HEADS[who];
    const hp = { x: x - 4, y: 36, w: 8, d: C.TAPE.y0 - 36, r: 1 };
    out.push(
      S(k.slabOf(hp, 4, 20, P, 3, 0.4), {
        tone: "mid",
        inner: L(k.sideSeam(hp, 10, P, 3), { tone: "faint" }),
      }),
    );
    if (who === "me") {
      out.push(S(k.cylinder(x, 40, 3, 24, 6, P, 24), { tone: "hi" }));
      out.push(L(k.knurl(x, 40, 3, 24.6, 29.4, 14, P), { tone: "lo" }));
      out.push(L(k.ring(x, 40, 1.6, 30, P, 16), { tone: "lo" }));
    } else {
      out.push(S(k.cylinder(x, 40, 3.6, 24, 2, P, 28), { tone: "hi" }));
      out.push(
        L(k.ring(x, 40, 2.6, 26, P, 24) + k.ring(x, 40, 1.4, 26, P, 16), {
          tone: "lo",
        }),
      );
      out.push(D([at([x, 40, 26])], { size: 0.45 }));
    }
  }
  return out.join("");
}

function posts() {
  return [C.WINDOW[0] - 5, C.WINDOW[1]]
    .map((x) => {
      const plan = { x, y: 37, w: 5, d: 6.6, r: 1 };
      return (
        S(
          k.slabOf(
            { x: x - 1.5, y: 35.5, w: 8, d: 9, r: 1.5 },
            0,
            2.4,
            P,
            3,
            0.4,
          ),
          { tone: "lo" },
        ) + S(k.slabOf(plan, 2.4, 27.6, P, 3, 0.4), { tone: "mid" })
      );
    })
    .join("");
}

function tapeRun() {
  const t = C.TAPE;
  const plan = {
    x: C.RUN[0],
    y: t.y0,
    w: C.RUN[1] - C.RUN[0],
    d: t.y1 - t.y0,
    r: 0,
  };
  return S(k.slabOf(plan, t.z0, t.z1 - t.z0, P, 1), {
    tone: "hi",
    crease: "lo",
    inner: L(k.segment([C.RUN[0], t.y1, 16], [C.RUN[1], t.y1, 16], P), {
      tone: "faint",
    }),
  });
}

function roller(i) {
  const [x, y] = C.ROLLERS[i];
  return [
    S(k.cylinder(x, y, 6.4, 0, 2, P, 32, 0.4), { tone: "lo" }),
    S(k.cylinder(x, y, 3, 2, 7.6, P, 24), { tone: "mid" }),
    S(k.cylinder(x, y, 6, 9.6, 1.2, P, 32), { tone: "mid" }),
    S(k.cylinder(x, y, 4.6, 10.8, 10.4, P, 32), { tone: "hi", crease: "none" }),
    L(k.sideArc(x, y, 4.6, 16, P, 24), { tone: "faint" }),
    S(k.cylinder(x, y, 6, 21.2, 1.2, P, 32), { tone: "mid" }),
    S(k.cylinder(x, y, 2, 22.4, 1.6, P, 20), { tone: "hi" }),
  ].join("");
}

const PIN_X = Array.from(
  { length: C.PINS },
  (_, i) => C.WINDOW[0] + ((C.WINDOW[1] - C.WINDOW[0]) * (i + 0.5)) / C.PINS,
);
function pin(i) {
  const x = PIN_X[i];
  return (
    S(k.cylinder(x, 45, 1.1, C.PIN_REST, 33 - C.PIN_REST, P, 14), {
      tone: "hi",
      crease: "none",
    }) + S(k.cylinder(x, 45, 1.6, C.PIN_REST + 2.2, 1, P, 16), { tone: "mid" })
  );
}

function beam() {
  const plan = {
    x: C.WINDOW[0] - 5.5,
    y: 36.5,
    w: C.WINDOW[1] - C.WINDOW[0] + 11,
    d: 15,
    r: 2,
  };
  const out = [S(k.slabOf(plan, 30, 4.6, P, 3, 0.6), { tone: "mid" })];
  // Ruler strip on the beam's front face: one major tick per 20 s tick.
  const face = plan.y + plan.d;
  const strip = {
    x: C.WINDOW[0],
    y: face,
    w: C.WINDOW[1] - C.WINDOW[0],
    d: 0.5,
    r: 0,
  };
  out.push(S(k.slabOf(strip, 30.4, 3.8, P, 1), { tone: "lo", crease: "none" }));
  const minor = [];
  const major = [];
  const n = C.PINS * 2;
  for (let i = 0; i <= n; i++) {
    const x = C.WINDOW[0] + ((C.WINDOW[1] - C.WINDOW[0]) * i) / n;
    (i % 2 ? minor : major).push(
      k.segment(
        [x, face + 0.5, 34.2],
        [x, face + 0.5, 34.2 - (i % 2 ? 1.2 : 2.4)],
        P,
      ),
    );
  }
  out.push(
    L(minor.join(""), { tone: "lo" }),
    L(major.join(""), { tone: "mid" }),
  );
  out.push(
    D(
      PIN_X.map((x) => at([x, 45, 34.6])),
      { size: 0.4, tone: "lo" },
    ),
  );
  out.push(
    k.faceTextSvg(k.topMatrix([C.WINDOW[0] + 3, 39.4], 34.6, P, "x"), "3 MIN", {
      size: 2.4,
      tone: "lo",
    }),
  );
  return out.join("");
}

function labelPlate() {
  const plan = { x: 34, y: 51, w: 60, d: 9, r: 1 };
  const rows = [
    [0.8, 0],
    [0.55, 1],
    [0.66, 2],
  ]
    .map(([w, j]) =>
      k.lineOnTop(
        [plan.x + 4, plan.y + 2.4 + j * 2.2],
        [plan.x + 4 + 44 * w, plan.y + 2.4 + j * 2.2],
        0.8,
        P,
      ),
    )
    .join("");
  return (
    S(k.slabOf(plan, 0, 0.8, P, 3), { tone: "lo", crease: "none" }) +
    L(k.planOutline(k.insetPlan(plan, 1.4), 0.8, P, 3), { tone: "faint" }) +
    L(rows, { tone: "lo", free: true }) +
    D(
      [
        at([plan.x + 1.8, plan.y + 4.5, 0.8]),
        at([plan.x + plan.w - 1.8, plan.y + 4.5, 0.8]),
      ],
      { size: 0.45 },
    )
  );
}

// ---------- inputs and the meeting key ----------
const JACKS = [
  [-194, 40],
  [-178, 40],
];
// Two device knobs: microphone and output (system audio) selection.
function inputs() {
  const plan = { x: -204, y: 33, w: 38, d: 15, r: 2 };
  const out = [
    S(k.slabOf(plan, 0, 6, P, 3, 0.6), {
      tone: "mid",
      inner: L(k.sideSeam(plan, 2.5, P, 3), { tone: "faint" }),
    }),
  ];
  JACKS.forEach(([x, y], i) => {
    out.push(L(k.ring(x, y, 5.6, 6, P, 32), { tone: "lo" }));
    out.push(
      L(k.radialTicks(x, y, 5.6, 8, 4, 6, [0.8, 1.4], P).minor, { tone: "lo" }),
    );
    out.push(S(k.cylinder(x, y, 4, 6, 3.4, P, 32), { tone: "mid" }));
    out.push(L(k.knurl(x, y, 4, 6, 9.4, 26, P), { tone: "lo" }));
    const a = i ? 0.9 : -0.6;
    out.push(
      L(
        k.lineOnTop(
          [x, y],
          [x + Math.sin(a) * 3.2, y - Math.cos(a) * 3.2],
          9.4,
          P,
        ),
        { tone: "hi", free: true },
      ),
    );
  });
  out.push(
    D(
      [
        at([plan.x + 2, plan.y + 2, 6]),
        at([plan.x + 2, plan.y + plan.d - 2, 6]),
        at([plan.x + plan.w - 2, plan.y + 2, 6]),
      ],
      { size: 0.45 },
    ),
  );
  return out.join("");
}
const KEY = [192, 50];
function keyBase() {
  const [x, y] = KEY;
  return (
    S(k.cylinder(x, y, 9, 0, 2, P, 40, 0.4), { tone: "lo" }) +
    S(k.cylinder(x, y, 7, 2, 4, P, 40, 0.4), { tone: "mid" }) +
    L(k.knurl(x, y, 7, 2.4, 5.6, 34, P), { tone: "lo" })
  );
}
function keyCap() {
  const [x, y] = KEY;
  return (
    S(k.cylinder(x, y, 5, 6, 3, P, 36, 0.5), { tone: "hi" }) +
    L(k.ring(x, y, 3.2, 9, P, 28), { tone: "lo" })
  );
}

// ---------- assemble, back to front ----------
const R0 = packRadii(C, 0, M.V);
const f0 = folderSvg(k, P, C, 0);
const tr = (by) => k.translateAlong(P, by);
const body = [
  plate(),
  labelPlate(),
  rack(),
  `<g clip-path="url(#cubby-${C.FOLDER.cubby[0]}-${C.FOLDER.cubby[1]})"><g data-part="folder-in">${f0.inside}</g></g>`,
  summaryTray(),
  `<g data-part="sheet0" opacity="0">${sheet(0)}</g><g data-part="sheet1" opacity="0">${sheet(1)}</g>`,
  unfiledTray(),
  `<g data-part="folder-out">${f0.outside}</g>`,
  cabinet(),
  `<g data-part="drawers">${drawersSvg(k, P, C, S0)}</g>`,
  toolBlock(),
  ...byDepth(
    [0, 1, 2, 3, 4].map((i) => [C.RODS.x0 + i * C.RODS.gap, C.RODS.y, i]),
  ).map(([, , i]) => `<g data-part="rod" data-i="${i}">${rod(i)}</g>`),
  dials(),
  `<g data-part="needles">${needlesSvg(k, P, C, S0)}</g>`,
  `<g data-part="bell">${bell()}<g data-part="plunger">${plunger()}</g></g>`,
  reelBase(0),
  reelBase(1),
  `<g data-part="packs">${packsSvg(k, P, C, R0)}</g>`,
  reelTop(0),
  reelTop(1),
  `<g data-part="spokes">${spokesSvg(k, P, C, R0)}</g>`,
  cardBox(),
  `<defs><clipPath id="card-slot"><path d="${cardClip()}"/></clipPath></defs><g clip-path="url(#card-slot)"><g data-part="card" transform="${tr([0, 0, -14])}"></g></g>`,
  inputs(),
  heads(),
  posts(),
  tapeRun(),
  `<g data-part="marks"></g>`,
  roller(0),
  roller(1),
  ...PIN_X.map((_, i) => `<g data-part="pin" data-i="${i}">${pin(i)}</g>`),
  beam(),
  keyBase(),
  `<g data-part="key">${keyCap()}</g>`,
];

const svg = k.figureSvg({
  width: WIDTH,
  height: HEIGHT,
  label:
    "A reel-to-reel transcript deck. Two record heads write the user's speech and everyone else's onto two tracks of a tape. A bridge of nine pins spans the last three minutes of tape and drops every 20 seconds: that is one observer tick. Behind it a four-drawer cabinet holds the prompt context, five rods stand for the tools, two dials count the 3-minute and 10-minute cooldowns, and a desk bell and a card slot deliver a nudge. At the back a pigeonhole rack holds projects; the meeting's folder moves from the unfiled tray into a project once the assistant knows what the meeting is about. When the meeting ends the take-up reel stops and two summary sheets land in the tray.",
  body,
});

const lib = (src) => src.replace(/^export /gm, "");
const live = `
${lib(readFileSync(join(HERE, "model.mjs"), "utf8"))}
${lib(readFileSync(join(HERE, "dyn.mjs"), "utf8"))}
const P = ${JSON.stringify(P)};
const PIN_X = ${JSON.stringify(PIN_X)};
const K = { slabOf, cylinder, extrude, solidSvg, lineSvg, segment, onTop, lineOnTop, iso, pathOf };
const TL = timeline();
const stage = document.querySelector(".iso-stage");
const svg = stage.querySelector("svg");
const q = (s) => svg.querySelector('[data-part="' + s + '"]');
const parts = { key: q("key"), packs: q("packs"), spokes: q("spokes"), marks: q("marks"), drawers: q("drawers"), needles: q("needles"), card: q("card"), plunger: q("plunger"), bell: q("bell"), fin: q("folder-in"), fout: q("folder-out"), s0: q("sheet0"), s1: q("sheet1") };
const pins = [...svg.querySelectorAll('[data-part="pin"]')];
const rods = [...svg.querySelectorAll('[data-part="rod"]')];
const readout = document.querySelector("[data-readout]");
const still = matchMedia("(prefers-reduced-motion: reduce)");
const SPAN = M.END + M.HOLD;
const RATE = 15;
let t = 0, target = 0, touring = true, visible = false, frame = 0, last = 0, idle = 0;
let cardKey = null, folderKey = -1, drawerKey = "";
function draw() {
  const s = stateAt(t, TL);
  const R = packRadii(C, s.live, M.V);
  parts.packs.innerHTML = packsSvg(K, P, C, R);
  parts.spokes.innerHTML = spokesSvg(K, P, C, R);
  parts.marks.innerHTML = marksSvg(K, P, C, LINES, s.live, M.V);
  const dk = s.press.toFixed(3) + s.filed + (s.nudges > 0);
  if (dk !== drawerKey) { drawerKey = dk; parts.drawers.innerHTML = drawersSvg(K, P, C, s); }
  parts.needles.innerHTML = needlesSvg(K, P, C, s);
  pins.forEach((p) => p.setAttribute("transform", translateAlong(P, [0, 0, -C.PIN_DROP * s.press])));
  rods.forEach((r) => r.setAttribute("transform", translateAlong(P, [0, 0, s.tools.includes(+r.dataset.i) ? 7 * s.press : 0])));
  const fk = Math.round(s.folder * 400);
  if (fk !== folderKey) { folderKey = fk; const f = folderSvg(K, P, C, s.folder); parts.fin.innerHTML = f.inside; parts.fout.innerHTML = f.outside; }
  const key = s.card ? s.card.t : null;
  if (key !== cardKey) { cardKey = key; parts.card.innerHTML = s.card ? cardSvg(K, P, C, s.card, KINDS) : ""; }
  parts.card.setAttribute("transform", translateAlong(P, [0, 0, -14 + 14 * s.rise]));
  parts.key.setAttribute("transform", translateAlong(P, [0, 0, -1.8 * Math.max(0, 1 - Math.abs(t - M.END) / 3)]));
  parts.plunger.setAttribute("transform", translateAlong(P, [0, 0, -1.6 * s.strike]));
  parts.bell.setAttribute("transform", "translate(" + (0.35 * s.ring * Math.sin(t * 40)).toFixed(2) + " 0)");
  parts.s0.setAttribute("opacity", s.sheets[0] > 0 ? 1 : 0);
  parts.s1.setAttribute("opacity", s.sheets[1] > 0 ? 1 : 0);
  parts.s0.setAttribute("transform", translateAlong(P, [0, 0, 26 * (1 - s.sheets[0])]));
  parts.s1.setAttribute("transform", translateAlong(P, [0, 0, 26 * (1 - s.sheets[1])]));
  if (readout.textContent !== s.readout) readout.textContent = s.readout;
  stage.setAttribute("aria-valuenow", String(Math.round(t)));
  stage.setAttribute("aria-valuetext", s.readout);
}
function tick(now) {
  const dt = last ? Math.min((now - last) / 1000, 1 / 20) : 1 / 60;
  last = now;
  if (touring) { target = (target + dt * RATE) % SPAN; t = target; }
  else t = still.matches ? target : t + (target - t) * (1 - Math.exp(-dt / 0.25));
  if (!touring && Math.abs(target - t) < 0.02) t = target;
  draw();
  const settled = !touring && t === target;
  frame = visible && !settled ? requestAnimationFrame(tick) : 0;
}
const run = () => { if (!frame && visible) { last = 0; frame = requestAnimationFrame(tick); } };
const steer = (to) => { touring = false; target = Math.min(SPAN, Math.max(0, to)); clearTimeout(idle); idle = setTimeout(() => { touring = !still.matches; run(); }, 5000); run(); };
new IntersectionObserver((e) => { visible = e[e.length - 1].isIntersecting; if (visible) run(); }, { rootMargin: "120px 0px" }).observe(stage);
if (still.matches) { touring = false; target = t = 445; }
stage.addEventListener("pointermove", (ev) => {
  const box = svg.getBoundingClientRect();
  steer(Math.min(1, Math.max(0, (ev.clientX - box.left) / box.width)) * SPAN);
});
stage.addEventListener("keydown", (ev) => {
  let to = null;
  if (ev.key === "ArrowRight" || ev.key === "ArrowUp") to = (Math.floor(target / M.TICK) + 1) * M.TICK + 0.5;
  if (ev.key === "ArrowLeft" || ev.key === "ArrowDown") to = (Math.ceil(target / M.TICK) - 1) * M.TICK + 0.5;
  if (ev.key === "Home") to = 0;
  if (ev.key === "End") to = M.END + 80;
  if (to === null) return;
  ev.preventDefault();
  steer(to);
});
const hash = Number(new URLSearchParams(location.search).get("t"));
if (hash) { touring = false; target = t = hash; }
draw();
`;

const page = k.plateHtml({
  fig: "Fig 1",
  title: "How Meet Assist listens",
  hint: "Hover to scrub the meeting · ← → step one tick",
  readout: S0.readout,
  keys: [
    {
      mark: "lit",
      label: "Intervention card: the nudge, at most one per 3 min",
    },
    {
      mark: "raised",
      label: "Tape: transcript, me on the top track, them below",
    },
    {
      mark: "flat",
      label:
        "Drawers, top to bottom: project brief, previous meeting, notes, recent nudges",
    },
    {
      mark: "edge",
      label:
        "Rods: list_sources, search_context, read_source, read_transcript, list_meetings",
    },
  ],
  caption:
    "Every 20 s the nine pins read the last 3 minutes of tape, the four drawers are pasted into the prompt, and the model may lift up to five tool rods before it answers one question: speak now or stay silent. A card only comes out if the 3-minute dial and the 10-minute same-kind dial are both clear and the text is not a repeat. The folder leaves the unfiled tray after 20 lines, once the model can tell which project the meeting belongs to; then the brief and previous-meeting drawers fill. When the meeting ends, a session summary and a rolling summary land in the tray.",
  body: `<div class="iso-stage" tabindex="0" role="slider" aria-label="Meeting timeline: arrow keys step one observer tick" aria-valuemin="0" aria-valuemax="${M.END + M.HOLD}" aria-valuenow="0">${svg}</div>`,
});

const name = theme === "light" ? "ai-flow-light.html" : "ai-flow.html";
writeFileSync(
  join(HERE, name),
  k.pageHtml({
    title: "How Meet Assist listens",
    theme,
    body: page,
    script: kitScript() + live,
    width: 760,
  }),
);
console.log(
  "wrote",
  name,
  "ticks",
  T.ticks.length,
  "lines",
  LINES.length,
  "filed at",
  T.filedAt,
  "attempts",
  T.attempts,
  "emitted",
  T.emitted.map((e) => e.t + ":" + e.kind).join(","),
);
