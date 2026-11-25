import { useEffect, useRef, useState } from "react";
import { AnimatePresence, animate, motion, MotionValue, useMotionValue, useTransform } from "framer-motion";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

type Media = { title: string; artist: string; playing: boolean } | null;
type Mode = "idle" | "media" | "listening" | "expanded";
type Edge = "top" | "bottom" | "left" | "right";

const TAURI = "__TAURI_INTERNALS__" in window;
const dbg = (m: string) => TAURI && invoke("log", { msg: m });
if (TAURI) {
  addEventListener("error", (e) => dbg("error: " + e.message));
  addEventListener("unhandledrejection", (e) => dbg("reject: " + String(e.reason)));
  dbg(`boot ${innerWidth}x${innerHeight} dpr=${devicePixelRatio}`);
}
const call = (c: string, a?: Record<string, unknown>) => (TAURI ? invoke(c, a) : Promise.resolve(null));
const GAP = 10;
const spring = { type: "spring", stiffness: 380, damping: 28, mass: 0.9 } as const;
const move = { type: "spring", stiffness: 260, damping: 22, mass: 1 } as const;

// iOS Dynamic Island metrics (pt): pill 126x37, expanded 370x162
function sizeOf(m: Mode, v: boolean, peek: boolean) {
  if (m === "idle") return peek ? (v ? { w: 44, h: 150, r: 22 } : { w: 150, h: 44, r: 22 }) : v ? { w: 37, h: 126, r: 18.5 } : { w: 126, h: 37, r: 18.5 };
  if (m === "media") return v ? { w: 37, h: 170, r: 18.5 } : { w: 250, h: 37, r: 18.5 };
  if (m === "listening") return { w: 340, h: 84, r: 42 };
  return { w: 370, h: 162, r: 44 };
}

function target(e: Edge, along: number, s: { w: number; h: number }, tucked: boolean) {
  const W = innerWidth, H = innerHeight, horiz = e === "top" || e === "bottom";
  const gap = tucked ? -(horiz ? s.h : s.w) * 0.6 : GAP;
  const half = (horiz ? s.w : s.h) / 2 + 12;
  const a = Math.min(Math.max(along, half), (horiz ? W : H) - half);
  if (e === "top") return { ex: a, ey: gap, fx: 0.5, fy: 0 };
  if (e === "bottom") return { ex: a, ey: H - gap, fx: 0.5, fy: 1 };
  if (e === "left") return { ex: gap, ey: a, fx: 0, fy: 0.5 };
  return { ex: W - gap, ey: a, fx: 1, fy: 0.5 };
}

const demoArt = "data:image/svg+xml;utf8," + encodeURIComponent(`<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 64 64'><defs><linearGradient id='g' x1='0' y1='0' x2='1' y2='1'><stop stop-color='#ff375f'/><stop offset='1' stop-color='#5e5ce6'/></linearGradient></defs><rect width='64' height='64' fill='url(#g)'/></svg>`);

function useMedia() {
  const [media, setMedia] = useState<Media>(TAURI ? null : { title: "Blinding Lights", artist: "The Weeknd", playing: true });
  const [art, setArt] = useState<string | null>(TAURI ? null : demoArt);
  const last = useRef("");
  useEffect(() => {
    if (!TAURI) return;
    const tick = async () => {
      try {
        const m = (await invoke<Media>("media_poll")) as Media;
        setMedia(m);
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

const Bar = ({ level, k }: { level: MotionValue<number>; k: number }) => {
  const s = useTransform(level, (v) => Math.min(1, 0.2 + v * k));
  return <motion.i style={{ scaleY: s }} />;
};
const Bars = ({ playing, level }: { playing: boolean; level?: MotionValue<number> }) =>
  level ? (
    <div className="bars">{[1.2, 2, 1.6, 0.9].map((k, i) => <Bar key={i} level={level} k={k} />)}</div>
  ) : (
    <div className={`bars ${playing ? "live" : "paused"}`}><i /><i /><i /><i /></div>
  );

const Icon = {
  prev: <svg viewBox="0 0 24 24"><path d="M6 6h2v12H6zM9.5 12 18 6v12z" /></svg>,
  next: <svg viewBox="0 0 24 24"><path d="M16 6h2v12h-2zM6 18V6l8.5 6z" /></svg>,
  play: <svg viewBox="0 0 24 24"><path d="M8 5v14l11-7z" /></svg>,
  pause: <svg viewBox="0 0 24 24"><path d="M6 5h4v14H6zM14 5h4v14h-4z" /></svg>,
  mic: <svg viewBox="0 0 24 24" width="18" height="18" fill="#fff"><path d="M12 14a3 3 0 0 0 3-3V6a3 3 0 0 0-6 0v5a3 3 0 0 0 3 3zm5-3a5 5 0 0 1-10 0H5a7 7 0 0 0 6 6.9V21h2v-3.1A7 7 0 0 0 19 11z" /></svg>,
};

const fade = {
  initial: { opacity: 0, filter: "blur(10px)", scale: 0.85 },
  animate: { opacity: 1, filter: "blur(0px)", scale: 1, transition: { duration: 0.32, delay: 0.08 } },
  exit: { opacity: 0, filter: "blur(10px)", scale: 0.85, transition: { duration: 0.14 } },
};

export default function App() {
  const { media, art, setMedia } = useMedia();
  const tint = useTint(art);
  const [listening, setListening] = useState(false);
  const [hover, setHover] = useState(false);
  const [text, setText] = useState("");
  const [edge, setEdge] = useState<Edge>("top");
  const [tucked, setTucked] = useState(false);
  const [dragging, setDragging] = useState(false);
  const [nonce, setNonce] = useState(0);
  const along = useRef(innerWidth / 2);
  const level = useMotionValue(0);

  const ex = useMotionValue(innerWidth / 2), ey = useMotionValue(GAP);
  const fx = useMotionValue(0.5), fy = useMotionValue(0);
  const w = useMotionValue(126), h = useMotionValue(37), r = useMotionValue(18.5), lift = useMotionValue(1);
  const x = useTransform([ex, w, fx], (v: number[]) => v[0] - v[1] * v[2]);
  const y = useTransform([ey, h, fy], (v: number[]) => v[0] - v[1] * v[2]);

  useMic(listening, level);

  const vertical = edge === "left" || edge === "right";
  const mode: Mode = listening ? "listening" : media && hover && !dragging ? "expanded" : media ? "media" : "idle";

  // events: hover from Rust (window is click-through), toggle-listen, stt
  useEffect(() => {
    const toggle = () => { setListening((v) => !v); setText(""); };
    const subs = [
      listen("toggle-listen", toggle),
      listen<string>("stt-partial", (e) => setText(e.payload)),
    ];
    const key = (e: KeyboardEvent) => e.ctrlKey && e.altKey && e.code === "Space" && toggle();
    addEventListener("keydown", key);
    return () => { subs.forEach((s) => s.then((f) => f())); removeEventListener("keydown", key); };
  }, []);

  // spring everything to its target
  useEffect(() => {
    if (dragging) return;
    const s = sizeOf(mode, vertical, hover);
    const t = target(edge, along.current, s, tucked);
    animate(w, s.w, spring); animate(h, s.h, spring); animate(r, s.r, spring);
    animate(ex, t.ex, move); animate(ey, t.ey, move);
    animate(fx, t.fx, move); animate(fy, t.fy, move);
    animate(lift, 1, move);
  }, [edge, mode, hover, tucked, dragging, nonce]);

  // tuck into the edge when idle (not on top)
  useEffect(() => {
    setTucked(false);
    if (edge === "top" || hover || dragging || listening) return;
    const id = setTimeout(() => setTucked(true), 2500);
    return () => clearTimeout(id);
  }, [edge, hover, dragging, listening, nonce]);

  // report hit rect so Rust can toggle click-through
  useEffect(() => {
    if (!TAURI) return;
    let last = "";
    dbg("hit-rect loop started");
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
      animate(lift, 1.06, spring);
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
    const W = innerWidth, H = innerHeight;
    const px = ex.get() + w.get() / 2 + s.vx * 0.15, py = ey.get() + h.get() / 2 + s.vy * 0.15;
    const dist: Record<Edge, number> = { top: py, bottom: H - py, left: px, right: W - px };
    const e = (Object.keys(dist) as Edge[]).reduce((a, b) => (dist[a] < dist[b] ? a : b));
    let a = e === "top" || e === "bottom" ? px : py;
    const mid = e === "top" || e === "bottom" ? W / 2 : H / 2;
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

  // Tauri: Rust reads the mouse (window stays click-through) and sends it here
  useEffect(() => {
    if (!TAURI) return;
    let active = false, btn: HTMLElement | null = null;
    type P = { x: number; y: number; down: boolean; inside: boolean };
    const onPtr = (e: { payload: P }) => {
      const { x: px, y: py, down: dn, inside } = e.payload;
      const f = fn.current;
      if (!d.current?.on) setHover(inside);
      if (dn && !active) {
        if (!inside) return;
        active = true;
        dbg(`press ${px | 0},${py | 0}`);
        btn = document.elementFromPoint(px, py)?.closest("button") as HTMLElement | null;
        if (!btn) f.pdown(px, py);
      } else if (dn && active) {
        if (!btn) f.pmove(px, py);
      } else if (!dn && active) {
        active = false;
        if (btn) {
          if (document.elementFromPoint(px, py)?.closest("button") === btn) btn.click();
          btn = null;
        } else f.pup();
      }
    };
    let busy = false, n = 0;
    dbg("ptr poll started, visibility=" + document.visibilityState);
    const id = setInterval(async () => {
      if (busy) return;
      busy = true;
      try {
        const p = await invoke<P>("get_ptr");
        if (++n % 150 === 1) dbg(`poll #${n} ${JSON.stringify(p)}`);
        onPtr({ payload: p });
      } catch (err) { dbg("get_ptr failed: " + String(err)); }
      busy = false;
    }, 16);
    return () => clearInterval(id);
  }, []);

  // keep position proportional when the overlay window gets resized to the monitor
  useEffect(() => {
    let lw = innerWidth, lh = innerHeight;
    const on = () => {
      const horiz = edge === "top" || edge === "bottom";
      along.current *= horiz ? innerWidth / lw : innerHeight / lh;
      lw = innerWidth; lh = innerHeight;
      setNonce((n) => n + 1);
    };
    addEventListener("resize", on);
    return () => removeEventListener("resize", on);
  }, [edge]);

  const control = (action: string) => {
    if (!TAURI && media) setMedia({ ...media, playing: action === "toggle" ? !media.playing : media.playing });
    call("media_control", { action });
  };
  const artEl = (big = false) => <div className={`art ${big ? "big" : ""}`} style={{ backgroundImage: art ? `url(${art})` : undefined, boxShadow: big && tint ? `0 6px 18px ${tint}66` : undefined }} />;

  const box = { x, y, width: w, height: h, scale: lift };
  const siri = mode === "listening";
  return (
    <div className="stage">
      <motion.div className="glow" style={{ ...box, borderRadius: r }} animate={{ opacity: siri ? 0.9 : tint && media ? (mode === "expanded" ? 0.5 : 0.3) : 0 }} transition={{ duration: 0.5 }}>
        <div className={`halo ${siri ? "siri" : ""}`} style={{ background: siri ? undefined : tint }} />
      </motion.div>
      <motion.div
        className={`island ${dragging ? "dragging" : ""}`}
        style={{ ...box, borderRadius: r }}
        onPointerDown={down} onPointerMove={mv} onPointerUp={up} onPointerCancel={up}
        onPointerEnter={() => !TAURI && setHover(true)} onPointerLeave={() => !TAURI && setHover(false)}
      >
        <AnimatePresence mode="popLayout" initial={false}>
          {mode === "media" && media && (
            <motion.div key={"media" + vertical} className={`row ${vertical ? "v" : ""}`} {...fade}>
              {artEl()}
              {!vertical && <div className="col" />}
              <Bars playing={media.playing} />
            </motion.div>
          )}
          {mode === "listening" && (
            <motion.div key="listening" className="row" style={{ padding: "0 20px" }} {...fade}>
              <motion.div className="mic" animate={{ scale: [1, 1.08, 1] }} transition={{ repeat: Infinity, duration: 1.6 }}>{Icon.mic}</motion.div>
              <div className="col"><div className="transcript">{text || <span className="hint">Listening…</span>}</div></div>
              <Bars playing level={level} />
            </motion.div>
          )}
          {mode === "expanded" && media && (
            <motion.div key="expanded" className="row ex" {...fade}>
              <div className="top">
                {artEl(true)}
                <div className="col"><div className="title">{media.title}</div><div className="artist">{media.artist}</div></div>
                <Bars playing={media.playing} />
              </div>
              <div className="controls">
                <motion.button whileTap={{ scale: 0.8 }} onClick={() => control("prev")}>{Icon.prev}</motion.button>
                <motion.button whileTap={{ scale: 0.8 }} onClick={() => control("toggle")}>{media.playing ? Icon.pause : Icon.play}</motion.button>
                <motion.button whileTap={{ scale: 0.8 }} onClick={() => control("next")}>{Icon.next}</motion.button>
              </div>
            </motion.div>
          )}
        </AnimatePresence>
      </motion.div>
    </div>
  );
}
