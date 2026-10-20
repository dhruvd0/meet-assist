// CASE landing page motion. One engine for smooth scroll (Lenis), one for
// animation (GSAP). Under reduced motion nothing here runs except icons:
// the markup and CSS already show every final state.

import gsap from "gsap";
import { ScrollTrigger } from "gsap/ScrollTrigger";
import Lenis from "lenis";
import { addCollection } from "iconify-icon";
import solar from "./solar-subset.json";

addCollection(solar);

const reduced = window.matchMedia("(prefers-reduced-motion: reduce)");

// ---------------------------------------------------------------------------
// Word splitting. The heading keeps its full text as the accessible name; the
// visible words are decorative copies hidden from assistive technology.
function splitWords(el) {
  const text = el.textContent.replace(/\s+/g, " ").trim();
  el.setAttribute("aria-label", text);
  const frag = document.createDocumentFragment();
  for (const node of Array.from(el.childNodes)) {
    if (node.nodeType === Node.TEXT_NODE) {
      for (const part of node.textContent.split(/(\s+)/)) {
        if (!part) continue;
        if (/^\s+$/.test(part)) {
          frag.append(" ");
        } else {
          const span = document.createElement("span");
          span.className = "w";
          span.setAttribute("aria-hidden", "true");
          span.textContent = part;
          frag.append(span);
        }
      }
    } else if (node.nodeType === Node.ELEMENT_NODE && node.tagName === "EM") {
      // Keep the italic wrapper; split its words inside.
      const em = document.createElement("em");
      em.setAttribute("aria-hidden", "true");
      for (const part of node.textContent.split(/(\s+)/)) {
        if (!part) continue;
        if (/^\s+$/.test(part)) {
          em.append(" ");
        } else {
          const span = document.createElement("span");
          span.className = "w";
          span.textContent = part;
          em.append(span);
        }
      }
      frag.append(em);
    } else {
      frag.append(node);
    }
  }
  el.replaceChildren(frag);
  return el.querySelectorAll(".w");
}

// ---------------------------------------------------------------------------
if (reduced.matches) {
  // Final states only. Strip the pre-animation opacity so nothing is hidden.
  document.documentElement.classList.remove("js");
  document.getElementById("nudge-row")?.classList.add("is-on");
  document.getElementById("nudge-empty")?.classList.add("is-off");
} else {
  run();
}

function run() {
  gsap.registerPlugin(ScrollTrigger);

  // Smooth scroll: Lenis drives ScrollTrigger from GSAP's ticker.
  const lenis = new Lenis({ autoRaf: false, lerp: 0.12 });
  lenis.on("scroll", ScrollTrigger.update);
  const tick = (t) => lenis.raf(t * 1000);
  gsap.ticker.add(tick);
  gsap.ticker.lagSmoothing(0);

  // In-page anchors go through Lenis so the nav links feel the same as scrolling.
  document.querySelectorAll('a[href^="#"]').forEach((a) => {
    a.addEventListener("click", (e) => {
      const target = document.querySelector(a.getAttribute("href"));
      if (!target) return;
      e.preventDefault();
      lenis.scrollTo(target, { offset: -56 });
      target.closest("details")?.removeAttribute("open");
    });
  });

  const cleanups = [() => gsap.ticker.remove(tick), () => lenis.destroy()];

  // --- Hero intro -----------------------------------------------------------
  const hero = buildHero();
  cleanups.push(hero.destroy);

  // --- Section headings: word by word, once, on enter -------------------------
  document.querySelectorAll("[data-words]").forEach((h) => {
    if (h.id === "hero-title") return;
    const words = splitWords(h);
    gsap.set(words, { opacity: 0, y: 12 });
    ScrollTrigger.create({
      trigger: h,
      start: "top 85%",
      once: true,
      onEnter: () =>
        gsap.to(words, {
          opacity: 1,
          y: 0,
          duration: 0.7,
          ease: "power3.out",
          stagger: 0.045,
        }),
    });
  });

  // Supporting copy and media follow their heading.
  document.querySelectorAll("[data-reveal]").forEach((el) => {
    if (el.closest(".hero")) return;
    gsap.set(el, { opacity: 0, y: 16 });
    ScrollTrigger.create({
      trigger: el,
      start: "top 88%",
      once: true,
      onEnter: () => gsap.to(el, { opacity: 1, y: 0, duration: 0.7, ease: "power3.out" }),
    });
  });

  // The four nudges land one after another, like beeps.
  const nudges = gsap.utils.toArray("[data-nudge]");
  gsap.set(nudges, { opacity: 0, y: 20 });
  ScrollTrigger.create({
    trigger: "#speaks-up .nudges",
    start: "top 80%",
    once: true,
    onEnter: () =>
      gsap.to(nudges, { opacity: 1, y: 0, duration: 0.6, ease: "power3.out", stagger: 0.18 }),
  });

  // Measurements change when fonts and the film's poster arrive.
  document.fonts?.ready.then(() => ScrollTrigger.refresh());
  const video = document.getElementById("film-video");
  video?.addEventListener("loadedmetadata", () => ScrollTrigger.refresh(), { once: true });

  // Pause everything while the tab is hidden; resume on return.
  const onVisibility = () => {
    if (document.hidden) {
      gsap.globalTimeline.pause();
    } else {
      gsap.globalTimeline.resume();
    }
  };
  document.addEventListener("visibilitychange", onVisibility);
  cleanups.push(() => document.removeEventListener("visibilitychange", onVisibility));

  // If the user flips reduced motion mid-session, finish immediately.
  const onReduce = (e) => {
    if (!e.matches) return;
    cleanups.forEach((fn) => fn());
    ScrollTrigger.killAll();
    gsap.globalTimeline.progress(1);
    document.documentElement.classList.remove("js");
    gsap.set("[data-reveal], .w, [data-nudge], #app, #card", { clearProps: "all" });
  };
  reduced.addEventListener("change", onReduce);
  cleanups.push(() => reduced.removeEventListener("change", onReduce));

  window.addEventListener("pagehide", () => cleanups.forEach((fn) => fn()), { once: true });
}

// ---------------------------------------------------------------------------
// Hero: headline, then the window, then a meeting that goes in circles, then
// the beep and the card. The nav and CTA are never hidden.
function buildHero() {
  const title = document.getElementById("hero-title");
  const words = splitWords(title);
  const heroReveals = document.querySelectorAll(".hero [data-reveal]");
  const app = document.getElementById("app");
  const card = document.getElementById("card");
  const ring = document.getElementById("ring");
  const lines = gsap.utils.toArray("#lines .line");
  const nudgeRow = document.getElementById("nudge-row");
  const nudgeEmpty = document.getElementById("nudge-empty");
  const meter = gsap.utils.toArray("#dock .app-meter i");
  const clock = document.getElementById("clock");

  gsap.set(words, { opacity: 0, y: 16 });
  gsap.set(heroReveals, { opacity: 0, y: 12 });
  gsap.set(app, { opacity: 0, y: 28 });
  gsap.set(lines, { opacity: 0, y: 8 });
  gsap.set(card, { opacity: 0, y: -14, scale: 0.97, transformOrigin: "top right" });

  // The level meter breathes for as long as the "meeting" is live.
  const breathe = gsap.to(meter, {
    scaleY: () => gsap.utils.random(0.25, 1),
    duration: 0.22,
    repeat: -1,
    repeatRefresh: true,
    ease: "sine.inOut",
    stagger: { each: 0.05, repeat: -1, repeatRefresh: true },
    paused: true,
  });

  const stamps = ["12:04", "12:11", "12:19", "12:26", "12:40", "12:47"];

  const tl = gsap.timeline({ defaults: { ease: "power3.out" } });
  tl.to(words, { opacity: 1, y: 0, duration: 0.8, stagger: 0.06 })
    .to(heroReveals, { opacity: 1, y: 0, duration: 0.7, stagger: 0.1 }, "-=0.5")
    .to(app, { opacity: 1, y: 0, duration: 0.9 }, "-=0.7")
    .add(() => breathe.play(), "-=0.2");

  // Transcript: the question comes back three times.
  lines.forEach((line, i) => {
    tl.to(line, { opacity: 1, y: 0, duration: 0.45 }, i === 0 ? "-=0.1" : "+=0.55");
    tl.add(() => {
      clock.textContent = stamps[i];
    }, "<");
  });

  // Beep and card.
  tl.add(() => {
    clock.textContent = "12:54";
  }, "+=0.5")
    .fromTo(
      ring,
      { opacity: 0.9, scale: 1 },
      { opacity: 0, scale: 1.08, duration: 0.9, ease: "power2.out" },
      "<"
    )
    .to(card, { opacity: 1, y: 0, scale: 1, duration: 0.55, ease: "back.out(1.6)" }, "<")
    .add(() => {
      nudgeRow.classList.add("is-on");
      nudgeEmpty.classList.add("is-off");
    }, "-=0.2")
    .from(nudgeRow, { opacity: 0, y: 6, duration: 0.4 }, "<");

  // Pointer: the window leans very slightly toward a fine pointer. Additive,
  // disabled for touch and coarse pointers, and reset on leave or blur.
  const fine = window.matchMedia("(pointer: fine)");
  const stage = app.parentElement;
  let raf = 0;
  let pending = null;
  const lean = (x, y) => {
    const r = stage.getBoundingClientRect();
    const dx = (x - (r.left + r.width / 2)) / r.width;
    const dy = (y - (r.top + r.height / 2)) / r.height;
    gsap.to(app, { rotationY: dx * 4, rotationX: -dy * 3, duration: 0.8, ease: "power2.out" });
  };
  const onMove = (e) => {
    if (!fine.matches || tl.progress() < 0.3) return;
    pending = [e.clientX, e.clientY];
    if (!raf) {
      raf = requestAnimationFrame(() => {
        raf = 0;
        if (pending) lean(...pending);
      });
    }
  };
  const reset = () => gsap.to(app, { rotationX: 0, rotationY: 0, duration: 0.8, ease: "power2.out" });
  stage.addEventListener("pointermove", onMove);
  stage.addEventListener("pointerleave", reset);
  window.addEventListener("blur", reset);

  // Save work when the hero is offscreen.
  const vis = ScrollTrigger.create({
    trigger: stage,
    start: "top bottom",
    end: "bottom top",
    onLeave: () => breathe.pause(),
    onLeaveBack: () => breathe.pause(),
    onEnter: () => tl.progress() > 0.3 && breathe.play(),
    onEnterBack: () => breathe.play(),
  });

  return {
    destroy() {
      tl.kill();
      breathe.kill();
      vis.kill();
      cancelAnimationFrame(raf);
      stage.removeEventListener("pointermove", onMove);
      stage.removeEventListener("pointerleave", reset);
      window.removeEventListener("blur", reset);
    },
  };
}
