import { useEffect, useRef, useState, type CSSProperties } from "react";
import { AnimatePresence, animate, motion, MotionValue, useMotionValue, useTransform } from "framer-motion";
import { invoke } from "@tauri-apps/api/core";

type Media = { title: string; artist: string; playing: boolean; position: number; duration: number; at: number } | null;
type Stt = { active: boolean; partial: string; text: string; err: string };
type Mode = "idle" | "media" | "listening" | "expanded";
type Edge = "top" | "bottom" | "left" | "right";
type Rect = { x: number; y: number; w: number; h: number };
type P = { x: number; y: number; down: boolean; inside: boolean };

const TAURI = "__TAURI_INTERNALS__" in window;
const dbg = (m: string) => TAURI && invoke("log", { msg: m });
const call = (c: string, a?: Record<string, unknown>) => (TAURI ? invoke(c, a) : Promise.resolve(null));
const spring = { type: "spring", stiffness: 380, damping: 28, mass: 0.9 } as const;
const move = { type: "spring", stiffness: 260, damping: 22, mass: 1 } as const;

function sizeOf(m: Mode, v: boolean, peek: boolean) {
  if (m === "idle") return peek ? (v ? { w: 44, h: 150, r: 22 } : { w: 150, h: 44, r: 22 }) : v ? { w: 37, h: 126, r: 18.5 } : { w: 126, h: 37, r: 18.5 };
  if (m === "media") return v ? { w: 37, h: 170, r: 18.5 } : { w: 250, h: 37, r: 18.5 };
  if (m === "listening") return { w: 340, h: 84, r: 42 };
  return { w: 370, h: 190, r: 44 };
}

function target(e: Edge, along: number, s: { w: number; h: number }, tucked: boolean, m: Rect) {
  const horiz = e === "top" || e === "bottom";
  const gap = tucked ? -(horiz ? s.h : s.w) * 0.6 : 0; // flush with the edge
  const half = (horiz ? s.w : s.h) / 2 + 12;
  const lo = (horiz ? m.x : m.y) + half, hi = (horiz ? m.x + m.w : m.y + m.h) - half;
  const a = Math.min(Math.max(along, lo), hi);
  if (e === "top") return { ex: a, ey: m.y + gap, fx: 0.5, fy: 0 };
  if (e === "bottom") return { ex: a, ey: m.y + m.h - gap, fx: 0.5, fy: 1 };
  if (e === "left") return { ex: m.x + gap, ey: a, fx: 0, fy: 0.5 };
  return { ex: m.x + m.w - gap, ey: a, fx: 1, fy: 0.5 };
}

const demoArt = "data:image/svg+xml;utf8," + encodeURIComponent(`<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 64 64'><defs><linearGradient id='g' x1='0' y1='0' x2='1' y2='1'><stop stop-color='#ff375f'/><stop offset='1' stop-color='#5e5ce6'/></linearGradient></defs><rect width='64' height='64' fill='url(#g)'/></svg>`);

function useMedia() {
  const [media, setMedia] = useState<Media>(TAURI ? null : { title: "Blinding Lights", artist: "The Weeknd", playing: true, position: 42000, duration: 215000, at: performance.now() });
  const [art, setArt] = useState<string | null>(TAURI ? null : demoArt);
  const last = useRef("");
  useEffect(() => {
    if (!TAURI) return;
    const tick = async () => {
      try {
        const m = await invoke<Omit<NonNullable<Media>, "at"> | null>("media_poll");
        setMedia(m ? { ...m, at: performance.now() } : null);
        const k = m ? `${m.title}|${m.artist}` : "";
        if (k !== last.current) {
          last.current = k;
          setArt(m ? await invoke<string | null>("media_art") : null);
        }
      } catch { setMedia(null); }
    };
    tick();
    const id = setInterval(tick, 1000);
    return () => clearInterval(id);
  }, []);
  return { media, art, setMedia };
}

function useTint(art: string | null) {
  const [tint, setTint] = useState("");
  useEffect(() => {
    if (!art) return setTint("");
    const i = new Image();
    i.onload = () => {
      const c = document.createElement("canvas");
      c.width = c.height = 8;
      const x = c.getContext("2d")!;
      x.drawImage(i, 0, 0, 8, 8);
      const d = x.getImageData(0, 0, 8, 8).data;
      let r = 0, g = 0, b = 0, n = 0;
      for (let k = 0; k < d.length; k += 4) {
        const sat = Math.max(d[k], d[k + 1], d[k + 2]) - Math.min(d[k], d[k + 1], d[k + 2]);
        const w = 1 + sat / 24;
        r += d[k] * w; g += d[k + 1] * w; b += d[k + 2] * w; n += w;
      }
      setTint(`rgb(${(r / n) | 0},${(g / n) | 0},${(b / n) | 0})`);
    };
    i.src = art;
  }, [art]);
  return tint;
}

function useStt() {
  const [stt, setStt] = useState<Stt>({ active: false, partial: "", text: "", err: "" });
  const last = useRef("");
  useEffect(() => {
    if (!TAURI) return;
    let busy = false;
    const id = setInterval(async () => {
      if (busy) return;
      busy = true;
      try {
        const s = await invoke<Stt>("stt_state");
        const k = JSON.stringify(s);
        if (k !== last.current) { last.current = k; setStt(s); }
      } catch {}
      busy = false;
    }, 100);
    return () => clearInterval(id);
  }, []);
  return { stt, setStt };
}

function useMic(active: boolean, level: MotionValue<number>) {
  useEffect(() => {
    if (!active) return level.set(0);
    let raf = 0, stream: MediaStream | undefined, ctx: AudioContext | undefined;
    navigator.mediaDevices.getUserMedia({ audio: true }).then((s) => {
      stream = s;
      ctx = new AudioContext();
      const an = ctx.createAnalyser();
      an.fftSize = 256;
      ctx.createMediaStreamSource(s).connect(an);
      const buf = new Uint8Array(an.fftSize);
      const loop = () => {
        an.getByteTimeDomainData(buf);
        let sum = 0;
        for (const v of buf) sum += ((v - 128) / 128) ** 2;
        level.set(Math.min(1, Math.sqrt(sum / buf.length) * 5));
        raf = requestAnimationFrame(loop);
      };
      loop();
    }).catch(() => {});
    return () => { cancelAnimationFrame(raf); stream?.getTracks().forEach((t) => t.stop()); ctx?.close(); };
  }, [active]);
}

// iOS "waveform" glyph: five rounded bars, tallest in the middle
const PROFILE = [0.42, 0.72, 1, 0.72, 0.42];
const GAIN = [0.9, 1.5, 2, 1.5, 0.9];
const Bar = ({ level, k }: { level: MotionValue<number>; k: number }) => {
  const s = useTransform(level, (v) => Math.min(1, 0.2 + v * k));
  return <motion.i style={{ scaleY: s }} />;
};
const Bars = ({ playing, level }: { playing: boolean; level?: MotionValue<number> }) =>
  level ? (
    <div className="bars">{GAIN.map((k, i) => <Bar key={i} level={level} k={k} />)}</div>
  ) : (
    <div className={`bars ${playing ? "live" : "paused"}`}>
      {PROFILE.map((h, i) => <i key={i} style={{ "--h": h } as CSSProperties} />)}
    </div>
  );

const fmt = (ms: number) => { const s = Math.max(0, Math.round(ms / 1000)); return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`; };
function Progress({ m }: { m: NonNullable<Media> }) {
  const [, f] = useState(0);
  useEffect(() => { const id = setInterval(() => f((n) => n + 1), 250); return () => clearInterval(id); }, []);
  const d = m.duration;
  const pos = Math.min(d || 1e12, m.position + (m.playing ? performance.now() - m.at : 0));
  return (
    <div className="prog">
      <span>{fmt(pos)}</span>
      <div className="track"><b style={{ width: d ? `${Math.min(100, (pos / d) * 100)}%` : "0%" }} /></div>
      <span>-{fmt(d ? d - pos : 0)}</span>
    </div>
  );
}

// SF Symbols, as drawn in the iOS Dynamic Island: backward.end.fill / pause.fill / play.fill / forward.end.fill
const Skip = ({ flip }: { flip?: boolean }) => (
  <svg width="16" height="15" viewBox="0 0 31 29" fill="currentColor">
    <g transform={flip ? "translate(31 0) scale(-1 1)" : undefined}>
      <rect x="0" y="0" width="5.8" height="29" rx="2.2" />
      <path d="M29.5 2.2V26.8L11.8 14.5z" stroke="currentColor" strokeWidth="3" strokeLinejoin="round" />
    </g>
  </svg>
);
const Icon = {
  prev: <Skip />,
  next: <Skip flip />,
  play: <svg width="15" height="17.5" viewBox="0 0 30 35" fill="currentColor"><path d="M5.5 5.2V29.8L25.8 17.5z" stroke="currentColor" strokeWidth="4.4" strokeLinejoin="round" /></svg>,
  pause: <svg width="15" height="17.5" viewBox="0 0 30 35" fill="currentColor"><rect x="0" y="0" width="10.8" height="35" rx="2.8" /><rect x="19.2" y="0" width="10.8" height="35" rx="2.8" /></svg>,
  mic: <svg viewBox="0 0 24 24" width="20" height="20" fill="#fff"><path d="M12 14a3 3 0 0 0 3-3V6a3 3 0 0 0-6 0v5a3 3 0 0 0 3 3zm5-3a5 5 0 0 1-10 0H5a7 7 0 0 0 6 6.9V21h2v-3.1A7 7 0 0 0 19 11z" /></svg>,
};

// play <-> pause morph, like SF Symbols' content transition
const glyphV = {
  initial: { scale: 0.55, opacity: 0, filter: "blur(4px)" },
  animate: { scale: 1, opacity: 1, filter: "blur(0px)", transition: { duration: 0.22, ease: [0.2, 0.8, 0.2, 1] } },
  exit: { scale: 0.55, opacity: 0, filter: "blur(4px)", transition: { duration: 0.14 } },
} as const;

// show the elapsed / remaining scrubber on the Media page (the iOS 17 layout). false = art, title and controls only
const SCRUBBER = true;

const pageV = {
  initial: (d: number) => ({ opacity: 0, filter: "blur(10px)", y: d * 28, scale: 0.9 }),
  animate: { opacity: 1, filter: "blur(0px)", y: 0, scale: 1, transition: { duration: 0.35, delay: 0.07 } },
  exit: (d: number) => ({ opacity: 0, filter: "blur(10px)", y: -d * 28, scale: 0.9, transition: { duration: 0.15 } }),
};

export default function App() {
  const { media, art, setMedia } = useMedia();
  const tint = useTint(art);
  const { stt, setStt } = useStt();
  const [page, setPage] = useState(0);
  const [dir, setDir] = useState(0);
  const [hover, setHover] = useState(false);
  const [edge, setEdge] = useState<Edge>("top");
  const [tucked, setTucked] = useState(false);
  const [dragging, setDragging] = useState(false);
  const [nonce, setNonce] = useState(0);
  const along = useRef(innerWidth / 2);
  const level = useMotionValue(0);
  const mons = useRef<Rect[]>([{ x: 0, y: 0, w: innerWidth, h: innerHeight }]);
  const monRef = useRef<Rect>(mons.current[0]);
  const edgeRef = useRef<Edge>("top");
  edgeRef.current = edge;

  const ex = useMotionValue(innerWidth / 2), ey = useMotionValue(0);
  const fx = useMotionValue(0.5), fy = useMotionValue(0);
  const w = useMotionValue(126), h = useMotionValue(37), r = useMotionValue(18.5), lift = useMotionValue(1), flat = useMotionValue(1);
  const rad = useTransform([r, flat], (v: number[]) => {
    const o = v[0] * (1 - v[1]), i = v[0], e = edgeRef.current;
    if (e === "top") return `${o}px ${o}px ${i}px ${i}px`;
    if (e === "bottom") return `${i}px ${i}px ${o}px ${o}px`;
    if (e === "left") return `${o}px ${i}px ${i}px ${o}px`;
    return `${i}px ${o}px ${o}px ${i}px`;
  });
  const ear = useTransform(r, (v) => `${(v * 0.36).toFixed(2)}px`);
  const x = useTransform([ex, w, fx], (v: number[]) => v[0] - v[1] * v[2]);
  const y = useTransform([ey, h, fy], (v: number[]) => v[0] - v[1] * v[2]);

  useMic(stt.active, level);

  const vertical = edge === "left" || edge === "right";
  const expanded = hover && !dragging && (!!media || page === 1);
  const mode: Mode = expanded ? "expanded" : stt.active ? "listening" : media ? "media" : "idle";
  const full = (stt.text + " " + stt.partial).trim();
  const done = stt.text.split(/\s+/).filter(Boolean), live = stt.partial.split(/\s+/).filter(Boolean);
  const toggleStt = () => (TAURI ? call("stt_toggle") : setStt((s) => ({ ...s, active: !s.active, text: s.active ? s.text : "", partial: s.active ? "" : "Dictation preview, words appear as you speak" })));

  useEffect(() => { if (stt.active) { setDir(0); setPage(1); } }, [stt.active]);

  // mouse wheel switches pages
  const wheel = useRef({ acc: 0, t: 0, last: 0 });
  const onWheel = (e: React.WheelEvent) => {
    const now = performance.now(), s = wheel.current;
    if (now - s.last > 250) s.acc = 0;
    s.last = now;
    if (now - s.t < 450) return;
    s.acc += e.deltaY;
    if (Math.abs(s.acc) < 30) return;
    const d = s.acc > 0 ? 1 : -1;
    s.acc = 0; s.t = now;
    setDir(d);
    setPage((p) => Math.max(0, Math.min(1, p + d)));
  };

  // spring everything to its target
  useEffect(() => {
    if (dragging) return;
    const s = sizeOf(mode, vertical, hover);
    const t = target(edge, along.current, s, tucked, monRef.current);
    animate(w, s.w, spring); animate(h, s.h, spring); animate(r, s.r, spring);
    animate(ex, t.ex, move); animate(ey, t.ey, move);
    animate(fx, t.fx, move); animate(fy, t.fy, move);
    animate(lift, 1, move); animate(flat, 1, spring);
  }, [edge, mode, hover, tucked, dragging, nonce]);

  // tuck into the edge when idle (not on top)
  useEffect(() => {
    setTucked(false);
    if (edge === "top" || hover || dragging || stt.active) return;
    const id = setTimeout(() => setTucked(true), 2500);
    return () => clearTimeout(id);
  }, [edge, hover, dragging, stt.active, nonce]);

  // report hit rect so Rust can toggle click-through
  useEffect(() => {
    if (!TAURI) return;
    let last = "";
    const id = setInterval(() => {
      const s = lift.get(), cw = w.get() * s, ch = h.get() * s;
      const px = x.get() - (cw - w.get()) / 2, py = y.get() - (ch - h.get()) / 2;
      const k = [px, py, cw, ch].map((n) => Math.round(n)).join();
      if (k !== last) { last = k; invoke("set_hit_rect", { x: px, y: py, w: cw, h: ch }); }
    }, 16);
    return () => clearInterval(id);
  }, []);

  // drag + snap
  const d = useRef<{ gx: number; gy: number; sx: number; sy: number; on: boolean; vx: number; vy: number; lx: number; ly: number; lt: number } | null>(null);
  const pdown = (cx: number, cy: number) => {
    d.current = { gx: cx - x.get(), gy: cy - y.get(), sx: cx, sy: cy, on: false, vx: 0, vy: 0, lx: cx, ly: cy, lt: performance.now() };
  };
  const pmove = (cx: number, cy: number) => {
    const s = d.current;
    if (!s) return;
    if (!s.on) {
      if (Math.hypot(cx - s.sx, cy - s.sy) < 4) return;
      s.on = true;
      const lx = x.get(), ly = y.get();
      fx.set(0); fy.set(0); ex.set(lx); ey.set(ly);
      animate(lift, 1.06, spring); animate(flat, 0, spring);
      setDragging(true);
      call("set_dragging", { on: true });
    }
    const now = performance.now(), dt = Math.max(1, now - s.lt);
    s.vx = s.vx * 0.6 + ((cx - s.lx) / dt) * 1000 * 0.4;
    s.vy = s.vy * 0.6 + ((cy - s.ly) / dt) * 1000 * 0.4;
    s.lx = cx; s.ly = cy; s.lt = now;
    ex.set(cx - s.gx); ey.set(cy - s.gy);
  };
  const pup = () => {
    const s = d.current;
    d.current = null;
    if (!s?.on) return;
    call("set_dragging", { on: false });
    const cx = ex.get() + w.get() / 2, cy = ey.get() + h.get() / 2;
    const px = cx + s.vx * 0.15, py = cy + s.vy * 0.15;
    const list = mons.current;
    const m = list.find((q) => cx >= q.x && cx < q.x + q.w && cy >= q.y && cy < q.y + q.h) ||
      list.reduce((a, q) => (Math.hypot(cx - (q.x + q.w / 2), cy - (q.y + q.h / 2)) < Math.hypot(cx - (a.x + a.w / 2), cy - (a.y + a.h / 2)) ? q : a));
    monRef.current = m;
    const dist: Record<Edge, number> = { top: py - m.y, bottom: m.y + m.h - py, left: px - m.x, right: m.x + m.w - px };
    const e = (Object.keys(dist) as Edge[]).reduce((a, b) => (dist[a] < dist[b] ? a : b));
    let a = e === "top" || e === "bottom" ? px : py;
    const mid = e === "top" || e === "bottom" ? m.x + m.w / 2 : m.y + m.h / 2;
    if (Math.abs(a - mid) < 48) a = mid; // magnetic center
    along.current = a;
    setEdge(e); setDragging(false); setNonce((n) => n + 1);
  };
  const fn = useRef({ pdown, pmove, pup });
  fn.current = { pdown, pmove, pup };

  // browser fallback (no Tauri): normal DOM pointer events
  const down = (e: React.PointerEvent) => {
    if (TAURI || (e.target as HTMLElement).closest("button")) return;
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    pdown(e.clientX, e.clientY);
  };
  const mv = (e: React.PointerEvent) => !TAURI && pmove(e.clientX, e.clientY);
  const up = () => !TAURI && pup();

  // Tauri: Rust reads the mouse (window stays click-through elsewhere); we poll it
  useEffect(() => {
    if (!TAURI) return;
    let active = false, btn: HTMLElement | null = null;
    const onPtr = (p: P) => {
      if (!d.current?.on) setHover(p.inside);
      const f = fn.current;
      if (p.down && !active) {
        if (!p.inside) return;
        active = true;
        btn = document.elementFromPoint(p.x, p.y)?.closest("button") as HTMLElement | null;
        if (btn) btn.classList.add("pressed"); else f.pdown(p.x, p.y);
      } else if (p.down && active) {
        if (!btn) f.pmove(p.x, p.y);
      } else if (!p.down && active) {
        active = false;
        if (btn) {
          btn.classList.remove("pressed");
          if (document.elementFromPoint(p.x, p.y)?.closest("button") === btn) btn.click();
          btn = null;
        } else f.pup();
      }
    };
    let busy = false;
    const id = setInterval(async () => {
      if (busy) return;
      busy = true;
      try { onPtr(await invoke<P>("get_ptr")); } catch (err) { dbg("get_ptr failed: " + String(err)); }
      busy = false;
    }, 16);
    return () => clearInterval(id);
  }, []);

  // all monitors (the overlay spans them), primary first
  useEffect(() => {
    if (!TAURI) return;
    invoke<number[][]>("get_monitors").then((l) => {
      if (!l.length) return;
      mons.current = l.map(([x, y, w, h]) => ({ x, y, w, h }));
      monRef.current = mons.current[0];
      along.current = mons.current[0].x + mons.current[0].w / 2;
      setNonce((n) => n + 1);
    });
  }, []);

  const control = (action: string) => {
    if (!TAURI && media) setMedia({ ...media, playing: action === "toggle" ? !media.playing : media.playing });
    call("media_control", { action });
  };
  const artEl = (big = false) => <div className={`art ${big ? "big" : ""}`} style={{ backgroundImage: art ? `url("${art}")` : undefined }} />;

  const siri = mode === "listening";
  const box = { x, y, width: w, height: h, scale: lift };
  const vp = { variants: pageV, initial: "initial", animate: "animate", exit: "exit" } as const;
  return (
    <div className={`stage ${dragging ? "dragging" : ""}`} style={{ ["--accent" as string]: tint ? `color-mix(in srgb, ${tint} 55%, white)` : "#fff" }}>
      <motion.div className="glow" style={{ ...box, borderRadius: rad }} animate={{ opacity: siri ? 0.35 : tint && media ? (mode === "expanded" ? 0.16 : 0.1) : 0 }} transition={{ duration: 0.6 }}>
        <div className={`halo ${siri ? "siri" : ""}`} style={{ background: siri ? undefined : tint }} />
      </motion.div>
      <motion.div
        className="shell"
        style={box}
        onPointerDown={down} onPointerMove={mv} onPointerUp={up} onPointerCancel={up} onWheel={onWheel}
        onPointerEnter={() => !TAURI && setHover(true)} onPointerLeave={() => !TAURI && setHover(false)}
      >
        {/* concave fillets where the island meets the screen edge */}
        <motion.div className={`ears e-${edge}`} style={{ opacity: flat, ["--ear" as string]: ear }}><i className="ear a" /><i className="ear b" /></motion.div>
        <motion.div className="island" style={{ borderRadius: rad }}>
          <AnimatePresence mode="popLayout" initial={false} custom={dir}>
            {mode === "media" && media && (
              <motion.div key={"media" + vertical} className={`row ${vertical ? "v" : ""}`} custom={0} {...vp}>
                {artEl()}
                {!vertical && <div className="col" />}
                <Bars playing={media.playing} />
              </motion.div>
            )}
            {mode === "listening" && (
              <motion.div key="listening" className="row" style={{ padding: "0 20px" }} custom={0} {...vp}>
                <motion.div className="mic" animate={{ scale: [1, 1.08, 1] }} transition={{ repeat: Infinity, duration: 1.6 }}>{Icon.mic}</motion.div>
                <div className="col"><div className="tx sm">{full ? <div className="tw">{done.map((t, i) => <span key={"d" + i} className="w">{t} </span>)}{live.map((t, i) => <span key={"l" + i + t} className="w hint">{t} </span>)}</div> : <span className="hint">Listening…</span>}</div></div>
                <Bars playing level={level} />
              </motion.div>
            )}
            {mode === "expanded" && page === 0 && (
              <motion.div key="ex0" className="row ex" custom={dir} {...vp}>
                {media ? (
                  <>
                    <div className="top">
                      {artEl(true)}
                      <div className="col"><div className="title">{media.title}</div><div className="artist">{media.artist}</div></div>
                      <Bars playing={media.playing} />
                    </div>
                    {SCRUBBER && <Progress m={media} />}
                    <div className="controls">
                      <button aria-label="Previous" onClick={() => control("prev")}>{Icon.prev}</button>
                      <button aria-label={media.playing ? "Pause" : "Play"} onClick={() => control("toggle")}>
                        <AnimatePresence mode="popLayout" initial={false}>
                          <motion.span key={media.playing ? "pause" : "play"} className="glyph" {...glyphV}>{media.playing ? Icon.pause : Icon.play}</motion.span>
                        </AnimatePresence>
                      </button>
                      <button aria-label="Next" onClick={() => control("next")}>{Icon.next}</button>
                    </div>
                  </>
                ) : <div className="hint empty">Nothing playing</div>}
              </motion.div>
            )}
            {mode === "expanded" && page === 1 && (
              <motion.div key="ex1" className="row ex" custom={dir} {...vp}>
                <div className="top">
                  <button className={`micbtn ${stt.active ? "on" : ""}`} onClick={toggleStt}>{Icon.mic}</button>
                  <div className="col">
                    <div className="title">Dictation</div>
                    <div className="artist">{stt.err || (stt.active ? "Listening…" : "Tap to dictate")}</div>
                  </div>
                  <Bars playing={stt.active} level={stt.active ? level : undefined} />
                </div>
                <div className="tx">
                  {full ? (
                    <div className="tw">
                      {done.map((t, i) => <span key={"d" + i} className="w">{t} </span>)}
                      {live.map((t, i) => <span key={"l" + i + t} className="w hint">{t} </span>)}
                    </div>
                  ) : <span className="hint">{stt.active ? "Listening… start speaking" : "Tap the mic or press Ctrl+Alt+Space, then speak. Words appear here live and are typed into the text field you have selected."}</span>}
                </div>
              </motion.div>
            )}
          </AnimatePresence>
          <motion.div className="dots" initial={false} animate={{ opacity: mode === "expanded" ? 1 : 0 }}>
            <i className={page === 0 ? "on" : ""} /><i className={page === 1 ? "on" : ""} />
          </motion.div>
        </motion.div>
      </motion.div>
    </div>
  );
}
