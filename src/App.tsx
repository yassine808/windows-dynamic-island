import { useEffect, useRef, useState, type CSSProperties } from "react";
import { AnimatePresence, animate, motion, MotionValue, useMotionValue, useTransform } from "framer-motion";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { createSpeech, speechAvailable } from "./speech";

type Media = { title: string; artist: string; playing: boolean; position: number; duration: number; at: number } | null;
type Stt = { active: boolean; partial: string; text: string; err: string };
type Mode = "idle" | "media" | "listening" | "expanded" | "timer";
type Tm = { total: number; left: number; end: number; running: boolean; done: boolean };
type Mix = { vol: number; mute: boolean; mic: number; mic_mute: boolean; ok: boolean };
type St = { cpu: number; ram: number; gpu: number; down: number; up: number };
type Px = { hex: string; r: number; g: number; b: number };
const PAGES = 7; // 0 media, 1 dictation, 2 timer, 3 mixer, 4 stats, 5 tools, 6 settings
const PAGE_H = [190, 138, 160, 140, 150, 230, 246];
const pad2 = (n: number) => String(n).padStart(2, "0");

// one wheel of the timer picker: scroll over it (or click the dim numbers) to change the value; wraps around
function WheelCol({ v, max, label, on }: { v: number; max: number; label: string; on: (n: number) => void }) {
  const acc = useRef(0);
  const step = (d: number) => on((v + d + max + 1) % (max + 1));
  return (
    <div
      className="wcol"
      onWheel={(e) => {
        e.stopPropagation(); // don't switch pages while scrolling the picker
        acc.current += e.deltaY;
        if (Math.abs(acc.current) < 40) return;
        const d = acc.current > 0 ? 1 : -1;
        acc.current = 0;
        step(d);
      }}
    >
      <button className="wv dim" aria-label={`${label} down`} onClick={() => step(-1)}>{pad2((v + max) % (max + 1))}</button>
      <div className="wv cur">{pad2(v)}<small>{label}</small></div>
      <button className="wv dim" aria-label={`${label} up`} onClick={() => step(1)}>{pad2((v + 1) % (max + 1))}</button>
    </div>
  );
}
const clamp01 = (n: number) => Math.min(1, Math.max(0, n));
const fmtB = (b: number) => (b >= 1048576 ? `${(b / 1048576).toFixed(1)} MB/s` : `${Math.round(b / 1024)} KB/s`);
const load = (v: number) => (v >= 85 ? "#ff453a" : v >= 60 ? "#ff9f0a" : "#fff");
type Edge = "top" | "bottom" | "left" | "right";
type Rect = { x: number; y: number; w: number; h: number };
type P = { x: number; y: number; down: boolean; inside: boolean };

const TAURI = "__TAURI_INTERNALS__" in window;
const dbg = (m: string) => TAURI && invoke("log", { msg: m });
const call = (c: string, a?: Record<string, unknown>) => (TAURI ? invoke(c, a) : Promise.resolve(null));
const spring = { type: "spring", stiffness: 380, damping: 28, mass: 0.9 } as const;
const move = { type: "spring", stiffness: 260, damping: 22, mass: 1 } as const;
// no overshoot: used when the island only slides along its own edge (un-tuck), so it can never pop off the edge
const settle = { type: "spring", stiffness: 300, damping: 38, mass: 1 } as const;

function sizeOf(m: Mode, v: boolean, peek: boolean, page = 0) {
  if (m === "idle") return peek ? (v ? { w: 44, h: 150, r: 22 } : { w: 150, h: 44, r: 22 }) : v ? { w: 37, h: 126, r: 18.5 } : { w: 126, h: 37, r: 18.5 };
  if (m === "media") return v ? { w: 37, h: 170, r: 18.5 } : { w: 250, h: 37, r: 18.5 };
  if (m === "timer") return v ? { w: 37, h: 126, r: 18.5 } : { w: 150, h: 37, r: 18.5 };
  if (m === "listening") return { w: 340, h: 84, r: 42 };
  return { w: 370, h: PAGE_H[page] ?? 190, r: 44 };
}

function target(e: Edge, along: number, s: { w: number; h: number }, tucked: boolean, m: Rect) {
  const horiz = e === "top" || e === "bottom";
  const gap = tucked ? -(horiz ? s.h : s.w) * 0.6 : -1; // flush with the edge (1px past it)
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
  const freeze = useRef(0); // after a seek, ignore the (stale) polled position for a moment
  useEffect(() => {
    if (!TAURI) return;
    const tick = async () => {
      try {
        const m = await invoke<Omit<NonNullable<Media>, "at"> | null>("media_poll");
        const now = performance.now();
        setMedia((prev) => (!m ? null : now < freeze.current && prev ? { ...m, position: prev.position, at: prev.at } : { ...m, at: now }));
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
  return { media, art, setMedia, freeze };
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
  const lastPartialLog = useRef(0);
  const local = useRef(false); // the browser engine is running the show (no WinRT polling then)
  useEffect(() => {
    if (!TAURI) return;
    const ok = speechAvailable();
    local.current = ok;
    dbg(`stt: browser engine ${ok ? "available" : "missing"}`);
    // no audio involved: just report what WebView2 thinks of the microphone
    navigator.permissions?.query({ name: "microphone" as PermissionName })
      .then((p) => dbg(`stt: mic permission ${p.state}`))
      .catch(() => dbg("stt: mic permission unknown"));
    if (!ok) return;
    let alive = true;
    let reported = false; // the end callback can fire several times per session
    call("stt_engine", { browser: true });
    let rec = createSpeech({
      partial: (t) => {
        setStt((s) => (s.partial === t ? s : { ...s, partial: t }));
        // visible proof in the log that words arrive while you speak
        const now = performance.now();
        if (now - lastPartialLog.current > 700) { lastPartialLog.current = now; dbg(`stt: interim "${t.slice(-60)}"`); }
      },
      final: (t) => {
        if (!alive) return;
        setStt((s) => ({ ...s, text: `${s.text}${t} `, partial: "" }));
        dbg(`stt: final "${t}"`);
        call("stt_type", { text: t });
      },
      error: (msg) => {
        // engine is unusable here: hand dictation back to the Windows recognizer for the next press
        local.current = false;
        if (!alive) return;
        setStt((s) => ({ ...s, err: msg, active: false, partial: "" }));
        call("stt_engine", { browser: false });
        dbg(`stt: browser engine fell back (${msg})`);
      },
      end: () => {
        if (!alive || reported) return;
        reported = true;
        setStt((s) => (s.active || s.partial ? { ...s, active: false, partial: "" } : s));
        call("stt_ended"); // keep the Rust state machine in sync when the engine stops by itself
      },
      debug: (m) => dbg(`stt: ${m}`),
    });
    let gone = false;
    const offs: Array<() => void> = [];
    const sub = (p: Promise<() => void>) => p.then((f) => (gone ? f() : offs.push(f))).catch(() => {});
    sub(listen("stt://start", () => { if (!alive) return; reported = false; setStt((s) => ({ ...s, active: true, err: "", text: "", partial: "" })); rec.start(); }));
    sub(listen("stt://stop", () => rec.stop()));
    return () => { alive = false; gone = true; rec.abort(); offs.forEach((f) => f()); };
  }, []);
  // Windows recognizer fallback: polled only while the browser engine is not in charge.
  useEffect(() => {
    if (!TAURI) return;
    let busy = false;
    const id = setInterval(async () => {
      if (local.current || busy) return;
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

const fmtT = (s: number) => { const h = Math.floor(s / 3600), m = Math.floor((s % 3600) / 60), x = s % 60; return (h ? `${h}:${String(m).padStart(2, "0")}` : `${m}`) + `:${String(x).padStart(2, "0")}`; };
const beep = () => {
  try {
    const c = new AudioContext();
    [0, 0.25, 0.5].forEach((t) => {
      const o = c.createOscillator(), g = c.createGain(), at = c.currentTime + t;
      o.frequency.value = 880;
      g.gain.setValueAtTime(0.15, at); g.gain.exponentialRampToValueAtTime(0.001, at + 0.18);
      o.connect(g).connect(c.destination); o.start(at); o.stop(at + 0.2);
    });
    setTimeout(() => c.close(), 1200);
  } catch {}
};
const Ring = ({ frac, size, done }: { frac: number; size: number; done?: boolean }) => {
  const sw = size > 30 ? 4 : 3, rr = (size - sw) / 2, c = 2 * Math.PI * rr;
  return (
    <svg className="ring" width={size} height={size} viewBox={`0 0 ${size} ${size}`}>
      <circle cx={size / 2} cy={size / 2} r={rr} fill="none" stroke="rgba(255,255,255,0.22)" strokeWidth={sw} />
      <circle cx={size / 2} cy={size / 2} r={rr} fill="none" stroke={done ? "#30d158" : "var(--orange, #ff9f0a)"} strokeWidth={sw} strokeLinecap="round" strokeDasharray={c} strokeDashoffset={c * (1 - Math.max(0, Math.min(1, frac)))} transform={`rotate(-90 ${size / 2} ${size / 2})`} />
    </svg>
  );
};

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

// elapsed rounds down, remaining rounds up, so they always add up to the length (like iOS)
const fmt = (ms: number, up = false) => { const s = Math.max(0, Math[up ? "ceil" : "floor"](ms / 1000)); return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`; };
function Progress({ m, scrub }: { m: NonNullable<Media>; scrub: number | null }) {
  const [, f] = useState(0);
  useEffect(() => { const id = setInterval(() => f((n) => n + 1), 250); return () => clearInterval(id); }, []);
  const d = m.duration > 0 ? m.duration : 0; // unknown length (live stream, some browsers) -> 0
  const run = Math.min(d || 1e12, Math.max(0, m.position + (m.playing ? performance.now() - m.at : 0)));
  const pos = scrub != null && d ? scrub * d : run;
  return (
    <div className="prog">
      <span>{fmt(pos)}</span>
      <div className={`seek ${d ? "" : "off"} ${scrub != null ? "on" : ""}`}>
        <div className="track"><b style={{ width: d ? `${Math.min(100, (pos / d) * 100)}%` : "0%" }} /></div>
      </div>
      {d > 0 && <span>-{fmt(d - pos, true)}</span>}
    </div>
  );
}

// Transport glyphs like the iOS Dynamic Island: backward.fill / pause.fill / play.fill / forward.fill (rounded, filled)
const Skip = ({ flip }: { flip?: boolean }) => (
  <svg width="30" height="19" viewBox="0 0 35 22" fill="currentColor" stroke="currentColor" strokeWidth="3.2" strokeLinejoin="round">
    <g transform={flip ? "translate(35 0) scale(-1 1)" : undefined}>
      <path d="M15.4 3.6V18.4L3.6 11z" />
      <path d="M31.4 3.6V18.4L19.6 11z" />
    </g>
  </svg>
);
const Icon = {
  prev: <Skip />,
  next: <Skip flip />,
  play: <svg width="19" height="22" viewBox="0 0 30 35" fill="currentColor"><path d="M5.5 5.2V29.8L25.8 17.5z" stroke="currentColor" strokeWidth="4.4" strokeLinejoin="round" /></svg>,
  pause: <svg width="17" height="22" viewBox="0 0 30 35" fill="currentColor"><rect x="0" y="0" width="10.8" height="35" rx="3.2" /><rect x="19.2" y="0" width="10.8" height="35" rx="3.2" /></svg>,
  spk: <svg viewBox="0 0 24 24" width="20" height="20" fill="currentColor"><path d="M3 9v6h4l5 4V5L7 9H3zm13.5 3a4.5 4.5 0 0 0-2.5-4v8a4.5 4.5 0 0 0 2.5-4zM14 3.2v2.1a7 7 0 0 1 0 13.4v2.1a9 9 0 0 0 0-17.6z" /></svg>,
  spkOff: <svg viewBox="0 0 24 24" width="20" height="20" fill="currentColor"><path d="M3 9v6h4l5 4V5L7 9H3z" /><path d="M16 9l5 6M21 9l-5 6" stroke="currentColor" strokeWidth="2" strokeLinecap="round" fill="none" /></svg>,
  micOff: <svg viewBox="0 0 24 24" width="20" height="20" fill="currentColor"><path d="M12 14a3 3 0 0 0 3-3V6a3 3 0 0 0-6 0v5a3 3 0 0 0 3 3zm5-3a5 5 0 0 1-10 0H5a7 7 0 0 0 6 6.9V21h2v-3.1A7 7 0 0 0 19 11z" /><path d="M4 4l16 16" stroke="#000" strokeWidth="4.5" strokeLinecap="round" /><path d="M4 4l16 16" stroke="currentColor" strokeWidth="2" strokeLinecap="round" /></svg>,
  cam: <svg viewBox="0 0 24 24" width="20" height="20" fill="currentColor"><path d="M9 4l-1.8 2H4a2 2 0 0 0-2 2v10a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-3.2L15 4H9zm3 4.5A4.5 4.5 0 1 1 7.5 13 4.5 4.5 0 0 1 12 8.5zm0 2A2.5 2.5 0 1 0 14.5 13 2.5 2.5 0 0 0 12 10.5z" /></svg>,
  drop: <svg viewBox="0 0 24 24" width="20" height="20" fill="currentColor"><path d="M12 2.5S5 10 5 14.5a7 7 0 0 0 14 0C19 10 12 2.5 12 2.5z" /></svg>,
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
  const { media, art, setMedia, freeze } = useMedia();
  const tint = useTint(art);
  const { stt, setStt } = useStt();
  const [page, setPage] = useState(0);
  const [dir, setDir] = useState(0);
  const [hover, setHover] = useState(false);
  const [edge, setEdge] = useState<Edge>("top");
  const [tucked, setTucked] = useState(false);
  const [dragging, setDragging] = useState(false);
  const [nonce, setNonce] = useState(0);
  const [scrub, setScrub] = useState<number | null>(null);
  const [cfg, setCfg] = useState({ shortcut: "Ctrl+Alt+Space", hold: false });
  const [rec, setRec] = useState(false);
  const [recErr, setRecErr] = useState("");
  const recCancel = useRef<(() => void) | null>(null);
  const [tm, setTm] = useState<Tm>({ total: 25 * 60, left: 25 * 60, end: 0, running: false, done: false });
  const timerActive = tm.running || tm.done;
  const leftNow = (t: Tm) => Math.max(0, Math.ceil((t.end - Date.now()) / 1000));
  const tmStart = (sec: number) => setTm({ total: sec, left: sec, end: Date.now() + sec * 1000, running: true, done: false });
  const tmToggle = () => setTm((t) => {
    if (t.running) return { ...t, running: false, left: leftNow(t) || t.total };
    const left = t.done || t.left <= 0 ? t.total : t.left;
    if (left <= 0) return t;
    return { ...t, left, end: Date.now() + left * 1000, running: true, done: false };
  });
  const tmSet = (total: number) => setTm((t) => ({ ...t, total, left: total, running: false, done: false }));
  const tmIdle = !tm.running && !tm.done && tm.left === tm.total;
  const tmH = Math.floor(tm.total / 3600), tmM = Math.floor((tm.total % 3600) / 60), tmS = tm.total % 60;
  const tmReset = () => setTm((t) => ({ ...t, left: t.total, running: false, done: false }));
  useEffect(() => {
    if (!tm.running) return;
    const id = setInterval(() => setTm((t) => {
      if (!t.running) return t;
      const left = leftNow(t);
      if (left <= 0) { beep(); return { ...t, left: 0, running: false, done: true }; }
      return left === t.left ? t : { ...t, left };
    }), 250);
    return () => clearInterval(id);
  }, [tm.running]);
  useEffect(() => {
    if (!tm.done) return;
    const id = setTimeout(() => setTm((t) => ({ ...t, left: t.total, done: false })), 8000);
    return () => clearTimeout(id);
  }, [tm.done]);

  // Auto-hide per axis: top/bottom and left/right are toggled separately. Reads the old single
  // "hide" key (off / sides / all) once so an existing preference is not lost.
  const [hideUD, setHideUD] = useState<boolean>(() => {
    try { const v = localStorage.getItem("hideUD"); return v ? v === "on" : localStorage.getItem("hide") === "all"; } catch { return false; }
  });
  const [hideLR, setHideLR] = useState<boolean>(() => {
    try { const v = localStorage.getItem("hideLR"); return v ? v === "on" : localStorage.getItem("hide") !== "off"; } catch { return true; }
  });
  const setHidePref = (k: "ud" | "lr", v: boolean) => {
    (k === "ud" ? setHideUD : setHideLR)(v);
    try { localStorage.setItem(k === "ud" ? "hideUD" : "hideLR", v ? "on" : "off"); } catch {}
  };
  // step aside while a windowed (borderless) fullscreen app owns the screen; the game-style exclusive
  // fullscreen case is already handled in Rust, this is the one that leaves the cursor on the desktop
  const [hideFs, setHideFs] = useState<boolean>(() => {
    try { return localStorage.getItem("hideFS") === "on"; } catch { return false; }
  });
  const [fsApp, setFsApp] = useState<"none" | "windowed" | "full">("none");
  const setHideFsPref = (v: boolean) => {
    setHideFs(v);
    try { localStorage.setItem("hideFS", v ? "on" : "off"); } catch {}
  };
  // Start with Windows: the setup turns it on while installing, this switch flips the same Run value.
  // (The browser demo has no registry, so it shows the shipping default.)
  const [autoOn, setAutoOn] = useState<boolean>(() => !TAURI);
  useEffect(() => { if (TAURI) invoke<boolean>("autostart_get").then(setAutoOn).catch(() => {}); }, []);
  const setAuto = (on: boolean) => {
    setAutoOn(on);
    call("autostart_set", { on }).catch((e) => { setAutoOn(!on); setRecErr(`Autostart: ${String(e)}`); });
  };
  useEffect(() => {
    if (!TAURI) return;
    let gone = false;
    let un: (() => void) | undefined;
    listen<{ full: boolean; windowed: boolean }>("fs", (e) => setFsApp(e.payload.windowed ? "windowed" : e.payload.full ? "full" : "none"))
      .then((f) => { if (gone) f(); else un = f; })
      .catch(() => {});
    return () => { gone = true; un?.(); };
  }, []);

  // ---- mixer / stats / tools state ----
  const [mix, setMix] = useState<Mix>({ vol: 0.5, mute: false, mic: 0.8, mic_mute: false, ok: true });
  const [slOn, setSlOn] = useState<string | null>(null);
  const [st, setSt] = useState<St>({ cpu: 0, ram: 0, gpu: -1, down: 0, up: 0 });
  const [clips, setClips] = useState<string[]>(TAURI ? [] : ["Hello world", "https://example.com/some/long/link"]);
  const [pick, setPick] = useState(false);
  const [px, setPx] = useState<Px | null>(null);
  const [note, setNote] = useState("");
  const mixFreeze = useRef(0);
  const sendT = useRef<Record<string, number>>({});
  const pickRef = useRef(false), pxRef = useRef<Px | null>(null), noteT = useRef(0);
  pickRef.current = pick; pxRef.current = px;
  const flash = (t: string) => { setNote(t); clearTimeout(noteT.current); noteT.current = window.setTimeout(() => setNote(""), 3000); };
  const slider = (id: "vol" | "mic", f: number, commit: boolean) => {
    f = clamp01(f);
    const now = performance.now();
    mixFreeze.current = now + 800;
    setMix((m) => ({ ...m, [id]: f, ...(f > 0 ? { [id === "vol" ? "mute" : "mic_mute"]: false } : {}) }));
    setSlOn(commit ? null : id);
    if (commit || now - (sendT.current[id] ?? 0) > 40) { sendT.current[id] = now; call("mixer_set_vol", { mic: id === "mic", v: f }); }
  };
  const muteSet = (mic: boolean, on: boolean) => {
    mixFreeze.current = performance.now() + 800;
    setMix((m) => ({ ...m, [mic ? "mic_mute" : "mute"]: on }));
    call("mixer_set_mute", { mic, on });
  };
  const shot = async () => {
    if (!TAURI) return flash("Needs the desktop app");
    flash("Capturing…");
    try { await invoke<string>("screenshot"); flash("Saved to Pictures/IslandWin"); } catch (e) { flash(String(e)); }
  };
  const clipCopy = (i: number) => { call("clip_copy", { i }); flash("Copied"); };
  const clipClear = () => { setClips([]); call("clip_clear"); };
  const togglePick = () => { setPx(null); setPick((v) => !v); };
  const commitPick = () => {
    const c = pxRef.current;
    pickRef.current = false; setPick(false);
    if (c) { call("set_clipboard", { text: c.hex }); flash(`Copied ${c.hex}`); }
  };
  const commitPickRef = useRef(commitPick);
  commitPickRef.current = commitPick;
  const mediaRef = useRef(media);
  mediaRef.current = media;
  const flyKey = useRef("");
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
    const o = Math.max(0, v[0] * (1 - v[1])), i = v[0], e = edgeRef.current;
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
  const expanded = (hover || pick) && !dragging && (!!media || page > 0);
  const mode: Mode = stt.active ? "listening" : expanded ? "expanded" : media ? "media" : timerActive ? "timer" : "idle";
  const full = (stt.text + " " + stt.partial).trim();
  const done = stt.text.split(/\s+/).filter(Boolean), live = stt.partial.split(/\s+/).filter(Boolean);
  // a toggle can be triggered twice by one physical click; ignore the echo
  const gate = useRef(0);
  const once = () => { const n = performance.now(); if (n - gate.current < 350) return false; gate.current = n; return true; };
  const toggleStt = () => !once() ? undefined : (TAURI ? call("stt_toggle") : setStt((s) => ({ ...s, active: !s.active, text: s.active ? s.text : "", partial: s.active ? "" : "Dictation preview, words appear as you speak" })));

  useEffect(() => { if (stt.active) { setDir(0); setPage(1); } }, [stt.active]);
  // refs read from the pointer handler and the timers below (they run outside React's render)
  const sttRef = useRef(stt); sttRef.current = stt;
  const holdRef = useRef(cfg.hold); holdRef.current = cfg.hold;
  useEffect(() => { if (TAURI) invoke<{ shortcut: string; hold: boolean }>("stt_settings").then(setCfg).catch(() => {}); }, []);
  const pretty = (k: string) => k.split("+").map((p) => p.replace(/^Key|^Digit/, "").replace(/^Arrow/, "")).join(" + ");
  const setHold = (hold: boolean) => { setCfg((c) => ({ ...c, hold })); call("set_stt_hold", { hold }); };
  const recordGate = useRef(0);
  const record = async () => {
    // Cancel is immediate (a second press on the chip must never be swallowed); only re-arming is debounced.
    if (rec) {
      dbg("shortcut record: cancelled from the UI");
      if (TAURI) call("cancel_record_shortcut"); else recCancel.current?.();
      return;
    }
    const now = performance.now();
    if (now - recordGate.current < 400) return;
    recordGate.current = now;
    setRecErr("");
    setRec(true);
    dbg("shortcut record: armed, waiting for keys");
    if (TAURI) {
      // A combo that is already taken or a bare letter sends you straight back to "press keys"
      // instead of dropping out of recording, so the chip can always be reassigned.
      for (let i = 0; i < 10; i++) {
        try {
          const k = await invoke<string>("record_shortcut");
          setCfg((c) => ({ ...c, shortcut: k }));
          setRecErr("");
          dbg(`shortcut record: saved ${k}`);
          break;
        } catch (e) {
          const msg = String(e);
          if (msg === "cancelled") break;
          setRecErr(msg);
          dbg(`shortcut record: rejected (${msg})`);
          if (!/add ctrl|not a shortcut|cannot be a shortcut|already|taken|register/i.test(msg)) break;
          await new Promise((r) => setTimeout(r, 260));
        }
      }
    } else {
      try {
        const k = await new Promise<string>((res, rej) => {
          const h = (e: KeyboardEvent) => {
            if (/^(Control|Shift|Alt|Meta)/.test(e.code)) return;
            e.preventDefault(); removeEventListener("keydown", h); recCancel.current = null;
            if (e.code === "Escape") return rej("cancelled");
            res([e.ctrlKey && "Ctrl", e.altKey && "Alt", e.shiftKey && "Shift", e.metaKey && "Super", e.code].filter(Boolean).join("+"));
          };
          addEventListener("keydown", h);
          recCancel.current = () => { removeEventListener("keydown", h); rej("cancelled"); };
        });
        setCfg((c) => ({ ...c, shortcut: k }));
        dbg(`shortcut record: saved ${k}`);
      } catch (e) { if (String(e) !== "cancelled") setRecErr(String(e)); }
    }
    setRec(false);
  };

  // While recording, the island takes the keyboard (Rust lifts its no-activate style), so the
  // webview sees the keys itself. That is the path that still works when the foreground app is
  // elevated and a low-level hook gets nothing; Rust takes whichever source reports first.
  useEffect(() => {
    if (!TAURI || !rec) return;
    const h = (e: KeyboardEvent) => {
      if (e.repeat || /^(Control|Shift|Alt|Meta|OS)/.test(e.key)) return;
      if (e.code === "Escape") { e.preventDefault(); call("cancel_record_shortcut"); return; }
      e.preventDefault(); e.stopPropagation();
      const parts = [e.ctrlKey && "Ctrl", e.altKey && "Alt", e.shiftKey && "Shift", e.metaKey && "Super", e.code].filter(Boolean);
      call("record_submit", { key: parts.join("+") }).catch((err) => setRecErr(String(err)));
    };
    addEventListener("keydown", h, true);
    return () => removeEventListener("keydown", h, true);
  }, [rec]);

  // live data only while the matching page is open
  const open = mode === "expanded";
  useEffect(() => {
    if (!open || page !== 3) return;
    let on = true;
    const f = async () => {
      if (!TAURI || performance.now() < mixFreeze.current) return;
      try { const m = await invoke<Mix>("mixer_get"); if (on && m.ok) setMix(m); } catch {}
    };
    f();
    const id = setInterval(f, 500);
    return () => { on = false; clearInterval(id); };
  }, [open, page]);
  useEffect(() => {
    if (!open || page !== 4) return;
    let on = true;
    const f = async () => {
      if (!TAURI) return setSt({ cpu: 20 + Math.random() * 40, ram: 55, gpu: 35 + Math.random() * 30, down: 1.2e6 * Math.random(), up: 2e5 * Math.random() });
      try { const v = await invoke<St>("stats"); if (on) setSt(v); } catch {}
    };
    f();
    const id = setInterval(f, 1000);
    return () => { on = false; clearInterval(id); };
  }, [open, page]);
  useEffect(() => {
    if (!open || page !== 5 || !TAURI) return;
    let on = true;
    const f = async () => { try { const l = await invoke<string[]>("clip_list"); if (on) setClips(l); } catch {} };
    f();
    const id = setInterval(f, 600);
    return () => { on = false; clearInterval(id); };
  }, [open, page]);
  useEffect(() => {
    if (!pick || !TAURI) return;
    let on = true, busy = false;
    const id = setInterval(async () => {
      if (busy) return;
      busy = true;
      try { const c = await invoke<Px | null>("pixel_at_cursor"); if (on && c) setPx(c); } catch {}
      busy = false;
    }, 50);
    return () => { on = false; clearInterval(id); };
  }, [pick]);

  // Tauri: Rust already turns every click into a synthetic one (see get_ptr loop). Drop the webview's own real click,
  // otherwise toggle buttons (mic, record shortcut) fire twice and flip straight back.
  useEffect(() => {
    if (!TAURI) return;
    const h = (e: MouseEvent) => { if (e.isTrusted) { e.stopImmediatePropagation(); e.preventDefault(); } };
    addEventListener("click", h, true);
    return () => removeEventListener("click", h, true);
  }, []);

  // mouse wheel switches pages; scroll is never thrown away, so a long scroll walks to the last page
  const wheel = useRef({ acc: 0, t: 0, last: 0, timer: 0 });
  useEffect(() => () => clearTimeout(wheel.current.timer), []);
  const onWheel = (e: React.WheelEvent) => {
    const s = wheel.current, now = performance.now();
    if (now - s.last > 300) s.acc = 0; // a new gesture starts from rest
    s.last = now;
    s.acc = Math.max(-400, Math.min(400, s.acc + e.deltaY));
    const tick = () => {
      s.timer = 0;
      if (Math.abs(s.acc) < 100) return;
      const d = s.acc > 0 ? 1 : -1;
      s.acc -= d * 100;
      setDir(d);
      setPage((p) => Math.max(0, Math.min(PAGES - 1, p + d)));
      s.t = performance.now();
      // a fast flick turns one page per frame-ish instead of skipping; the rest is not capped
      if (Math.abs(s.acc) >= 100) s.timer = window.setTimeout(tick, 140);
    };
    if (!s.timer) s.timer = window.setTimeout(tick, Math.max(0, 140 - (now - s.t)));
  };

  // spring everything to its target
  useEffect(() => {
    if (dragging) return;
    const s = sizeOf(mode, vertical, hover, page);
    const t = target(edge, along.current, s, tucked, monRef.current);
    const key = `${edge}|${nonce}`, fly = key !== flyKey.current; // fly = just released from a drag
    flyKey.current = key;
    animate(w, s.w, spring); animate(h, s.h, spring); animate(r, s.r, spring);
    animate(ex, t.ex, fly ? move : settle); animate(ey, t.ey, fly ? move : settle);
    animate(fx, t.fx, move); animate(fy, t.fy, move);
    animate(lift, 1, move); animate(flat, 1, spring);
  }, [edge, mode, hover, tucked, dragging, nonce, page]);

  // tuck into the edge when idle (if that edge's axis is enabled)
  useEffect(() => {
    setTucked(false);
    // a windowed fullscreen app in front (and the setting on): leave the screen to it
    if (hideFs && fsApp === "windowed") { setTucked(true); return; }
    const can = edge === "top" || edge === "bottom" ? hideUD : hideLR;
    if (!can || hover || dragging || stt.active || pick) return;
    const id = setTimeout(() => setTucked(true), 2500);
    return () => clearTimeout(id);
  }, [edge, hover, dragging, stt.active, nonce, hideUD, hideLR, pick, hideFs, fsApp]);

  // report hit rect so Rust can toggle click-through; driven by the springs themselves, so an idle
  // island costs nothing instead of running a 60 Hz timer
  useEffect(() => {
    if (!TAURI) return;
    let last = "";
    const push = () => {
      const s = lift.get(), cw = w.get() * s, ch = h.get() * s;
      const px = x.get() - (cw - w.get()) / 2, py = y.get() - (ch - h.get()) / 2;
      const k = [px, py, cw, ch].map((n) => Math.round(n)).join();
      if (k !== last) { last = k; invoke("set_hit_rect", { x: px, y: py, w: cw, h: ch }); }
    };
    const stops = [x, y, w, h, lift].map((v) => v.on("change", push));
    push();
    return () => stops.forEach((off) => off());
  }, []);

  // drag + snap
  const d = useRef<{ gx: number; gy: number; sx: number; sy: number; on: boolean; vx: number; vy: number; lx: number; ly: number; lt: number; lock: boolean; hz: boolean; mid: number } | null>(null);
  const pdown = (cx: number, cy: number) => {
    d.current = { gx: cx - x.get(), gy: cy - y.get(), sx: cx, sy: cy, on: false, vx: 0, vy: 0, lx: cx, ly: cy, lt: performance.now(), lock: false, hz: true, mid: 0 };
  };
  const monAt = (cx: number, cy: number) => {
    const list = mons.current;
    return list.find((q) => cx >= q.x && cx < q.x + q.w && cy >= q.y && cy < q.y + q.h) ||
      list.reduce((a, q) => (Math.hypot(cx - (q.x + q.w / 2), cy - (q.y + q.h / 2)) < Math.hypot(cx - (a.x + a.w / 2), cy - (a.y + a.h / 2)) ? q : a));
  };
  const edgeAt = (m: Rect, px: number, py: number) => {
    const dist: Record<Edge, number> = { top: py - m.y, bottom: m.y + m.h - py, left: px - m.x, right: m.x + m.w - px };
    return (Object.keys(dist) as Edge[]).reduce((a, b) => (dist[a] < dist[b] ? a : b));
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
    // magnetic center (like Word's guides): near the middle of the screen the island sticks there until you pull away
    let tx = cx - s.gx, ty = cy - s.gy;
    // (judged by the cursor: while dragging the island is the big expanded card, its own center is not where you aim)
    const iw = w.get(), ih = h.get();
    const m = monAt(cx, cy), e = edgeAt(m, cx, cy), hz = e === "top" || e === "bottom";
    const mid = hz ? m.x + m.w / 2 : m.y + m.h / 2, off = Math.abs((hz ? cx : cy) - mid);
    if (s.hz !== hz) s.lock = false;
    s.hz = hz; s.mid = mid;
    if (!s.lock && off < 14) s.lock = true; else if (s.lock && off > 34) s.lock = false;
    if (s.lock) { if (hz) tx = mid - iw / 2; else ty = mid - ih / 2; }
    ex.set(tx); ey.set(ty);
  };
  const pup = () => {
    const s = d.current;
    d.current = null;
    if (!s?.on) return;
    call("set_dragging", { on: false });
    const cx = s.lx, cy = s.ly; // last cursor position
    const px = cx + s.vx * 0.15, py = cy + s.vy * 0.15;
    const m = monAt(cx, cy);
    monRef.current = m;
    const e = edgeAt(m, px, py), hz = e === "top" || e === "bottom";
    let a = hz ? px : py;
    if (s.lock && s.hz === hz) a = s.mid; // stuck to the center: stay exactly there
    along.current = a;
    setEdge(e); setDragging(false); setNonce((n) => n + 1);
  };
  const fn = useRef({ pdown, pmove, pup });
  fn.current = { pdown, pmove, pup };

  // seek bar: click or drag
  const seekTo = (f: number, commit: boolean, id?: string) => {
    if (id === "vol" || id === "mic") return slider(id, f, commit);
    const m = mediaRef.current;
    if (!m || m.duration <= 0) return;
    f = Math.min(1, Math.max(0, f));
    if (!commit) return setScrub(f);
    setScrub(null);
    const ms = Math.round(f * m.duration);
    freeze.current = performance.now() + 1500;
    setMedia({ ...m, position: ms, at: performance.now() });
    call("media_seek", { ms });
  };
  const seekFn = useRef(seekTo);
  seekFn.current = seekTo;

  // browser fallback (no Tauri): normal DOM pointer events
  const skb = useRef<DOMRect | null>(null), skid = useRef<string | undefined>(undefined);
  const down = (e: React.PointerEvent) => {
    const t = e.target as HTMLElement;
    if (TAURI || t.closest("button")) return;
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    const sk = t.closest(".seek:not(.off)");
    if (sk) { skb.current = sk.getBoundingClientRect(); skid.current = (sk as HTMLElement).dataset.id; seekTo((e.clientX - skb.current.left) / skb.current.width, false, skid.current); return; }
    pdown(e.clientX, e.clientY);
  };
  const mv = (e: React.PointerEvent) => {
    if (TAURI) return;
    if (skb.current) seekTo((e.clientX - skb.current.left) / skb.current.width, false, skid.current); else pmove(e.clientX, e.clientY);
  };
  const up = (e: React.PointerEvent) => {
    if (TAURI) return;
    if (skb.current) { seekTo((e.clientX - skb.current.left) / skb.current.width, true, skid.current); skb.current = null; } else pup();
  };

  // Tauri: Rust reads the mouse (window stays click-through elsewhere) and pushes every change, so a
  // fast click can never fall between two polls and idle costs no timer at all.
  useEffect(() => {
    if (!TAURI) return;
    let active = false, btn: HTMLElement | null = null, sk: DOMRect | null = null, skId: string | undefined;
    let held = false; // the dictation mic is being held down in Hold mode
    const frac = (p: P) => (sk ? (p.x - sk.left) / sk.width : 0);
    const onPtr = (p: P) => {
      if (!d.current?.on) setHover(p.inside);
      const f = fn.current;
      if (p.down && !active) {
        if (!p.inside) { if (pickRef.current && pxRef.current) commitPickRef.current(); return; }
        active = true;
        const el = document.elementFromPoint(p.x, p.y);
        btn = el?.closest("button") as HTMLElement | null;
        const seek = !btn ? (el?.closest(".seek:not(.off)") as HTMLElement | null) : null;
        if (seek) { sk = seek.getBoundingClientRect(); skId = seek.dataset.id; seekFn.current(frac(p), false, skId); }
        else if (btn) {
          btn.classList.add("pressed");
          // Hold mode: the mic button is push-to-talk, exactly like the global shortcut
          if (holdRef.current && btn.dataset.dict === "1") {
            held = true;
            if (!sttRef.current.active) call("stt_toggle");
          }
        } else f.pdown(p.x, p.y);
      } else if (p.down && active) {
        if (sk) seekFn.current(frac(p), false, skId);
        else if (!btn) f.pmove(p.x, p.y);
      } else if (!p.down && active) {
        active = false;
        if (sk) { seekFn.current(frac(p), true, skId); sk = null; skId = undefined; }
        else if (btn) {
          btn.classList.remove("pressed");
          if (held) { held = false; if (sttRef.current.active) call("stt_toggle"); }
          else if (document.elementFromPoint(p.x, p.y)?.closest("button") === btn) btn.click();
          btn = null;
        } else f.pup();
      }
    };
    let un: (() => void) | undefined, gone = false;
    listen<P>("ptr", (e) => onPtr(e.payload))
      .then((f) => { if (gone) f(); else un = f; })
      .catch((err) => dbg("ptr listener failed: " + String(err)));
    return () => { gone = true; un?.(); };
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
                {!vertical && timerActive && <span className="tt">{tm.done ? "Done" : fmtT(tm.left)}</span>}
                <Bars playing={media.playing} />
              </motion.div>
            )}
            {mode === "timer" && (
              <motion.div key={"timer" + vertical} className={`row ${vertical ? "v" : ""}`} custom={0} {...vp}>
                <Ring frac={tm.total ? tm.left / tm.total : 0} size={22} done={tm.done} />
                {!vertical && <span className="tt">{tm.done ? "Done" : fmtT(tm.left)}</span>}
              </motion.div>
            )}
            {mode === "listening" && (
              <motion.div key="listening" className="row" style={{ padding: "0 20px" }} custom={0} {...vp}>
                <motion.button className="mic" aria-label="Stop dictation" onClick={toggleStt} animate={{ scale: [1, 1.08, 1] }} transition={{ repeat: Infinity, duration: 1.6 }}>{Icon.mic}</motion.button>
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
                    {SCRUBBER && <Progress m={media} scrub={scrub} />}
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
                  <button className={`micbtn ${stt.active ? "on" : ""}`} data-dict="1" onClick={toggleStt}>{Icon.mic}</button>
                  <div className="col">
                    <div className="title">Dictation</div>
                    <div className={`artist ${stt.err || recErr ? "err" : ""}`}>{stt.err || recErr || (rec ? "Press keys…" : stt.active ? (stt.partial || "Listening…") : stt.text.trim() || (cfg.hold ? "Hold to talk" : "Tap to talk"))}</div>
                  </div>
                  <Bars playing={stt.active} level={stt.active ? level : undefined} />
                </div>
                <div className="cfg">
                  <button className={`chip ${rec ? "rec" : ""}`} aria-label="Record shortcut" onClick={record}>
                    <i className="dot" />{rec ? (recErr || "Press keys…") : pretty(cfg.shortcut)}
                  </button>
                  <div className="seg" role="group" aria-label="Shortcut mode">
                    <button className={cfg.hold ? "on" : ""} onClick={() => setHold(true)}>Hold</button>
                    <button className={cfg.hold ? "" : "on"} onClick={() => setHold(false)}>Toggle</button>
                  </div>
                </div>
              </motion.div>
            )}
            {mode === "expanded" && page === 2 && (
              <motion.div key="ex2" className="row ex" custom={dir} {...vp}>
                {tmIdle ? (
                  <div className="top">
                    <div className="picker">
                      <WheelCol v={tmH} max={23} label="h" on={(h) => tmSet(h * 3600 + tmM * 60 + tmS)} />
                      <span className="wsep">:</span>
                      <WheelCol v={tmM} max={59} label="m" on={(m) => tmSet(tmH * 3600 + m * 60 + tmS)} />
                      <span className="wsep">:</span>
                      <WheelCol v={tmS} max={59} label="s" on={(x) => tmSet(tmH * 3600 + tmM * 60 + x)} />
                    </div>
                    <button className="micbtn" aria-label="Start timer" onClick={tmToggle}>{Icon.play}</button>
                  </div>
                ) : (
                  <div className="top">
                    <Ring frac={tm.total ? tm.left / tm.total : 0} size={46} done={tm.done} />
                    <div className="col">
                      <div className="title big">{tm.done ? "Done" : fmtT(tm.left)}</div>
                      <div className="artist">{tm.running ? "Running" : tm.done ? "Time's up" : "Paused"}</div>
                    </div>
                    <button className="micbtn" aria-label={tm.running ? "Pause timer" : "Start timer"} onClick={tmToggle}>{tm.running ? Icon.pause : Icon.play}</button>
                  </div>
                )}
                <div className="cfg">
                  <div className="seg" role="group" aria-label="Presets">
                    {[5, 15, 25].map((m) => <button key={m} className={tm.total === m * 60 ? "on" : ""} onClick={() => tmStart(m * 60)}>{m}m</button>)}
                  </div>
                  {!tmIdle && <button className="chip" aria-label="Reset timer" onClick={tmReset}>Reset</button>}
                </div>
              </motion.div>
            )}
            {mode === "expanded" && page === 3 && (
              <motion.div key="ex3" className="row ex" custom={dir} {...vp}>
                {([["vol", mix.vol, mix.mute, Icon.spk, Icon.spkOff, "Speakers"], ["mic", mix.mic, mix.mic_mute, Icon.mic, Icon.micOff, "Microphone"]] as const).map(([id, v, m, on, off, label]) => (
                  <div className="mrow" key={id}>
                    <button className={`mbtn ${m ? "off" : ""}`} aria-label={`Mute ${label}`} onClick={() => muteSet(id === "mic", !m)}>{m || v === 0 ? off : on}</button>
                    <div className={`seek ${slOn === id ? "on" : ""}`} data-id={id}><div className="track"><b style={{ width: `${(m ? 0 : v) * 100}%` }} /></div></div>
                    <span className="pct">{Math.round((m ? 0 : v) * 100)}</span>
                  </div>
                ))}
              </motion.div>
            )}
            {mode === "expanded" && page === 4 && (
              <motion.div key="ex4" className="row ex" custom={dir} {...vp}>
                {([["CPU", st.cpu], ["RAM", st.ram], ["GPU", st.gpu]] as const).map(([n, v]) => (
                  <div className="srow" key={n}>
                    <span className="sl">{n}</span>
                    <div className="sbar"><b style={{ width: `${Math.max(0, Math.min(100, v))}%`, background: load(v) }} /></div>
                    <span className="sv">{v < 0 ? "n/a" : `${Math.round(v)}%`}</span>
                  </div>
                ))}
                <div className="srow"><span className="sl">NET</span><span className="net">↓ {fmtB(st.down)}</span><span className="net">↑ {fmtB(st.up)}</span></div>
              </motion.div>
            )}
            {mode === "expanded" && page === 5 && (
              <motion.div key="ex5" className="row ex" custom={dir} {...vp}>
                <div className="tools">
                  <button className="tool" aria-label="Screenshot" onClick={shot}>{Icon.cam}<span>Screenshot</span></button>
                  <button className={`tool ${pick ? "on" : ""}`} aria-label="Pick color" onClick={togglePick}>
                    {pick && px ? <i className="sw" style={{ background: px.hex }} /> : Icon.drop}<span>{pick ? (px ? px.hex : "Click a pixel…") : "Pick color"}</span>
                  </button>
                </div>
                <div className="clh"><span>{note || "Clipboard"}</span><button className="chip" aria-label="Clear clipboard history" onClick={clipClear}>Clear</button></div>
                <div className="cl">
                  {clips.slice(0, 4).map((c, i) => <button key={i + c} className="clip" onClick={() => clipCopy(i)}>{c.replace(/\s+/g, " ").slice(0, 70)}</button>)}
                  {!clips.length && <div className="hint cle">Copy something to see it here</div>}
                </div>
              </motion.div>
            )}
            {mode === "expanded" && page === 6 && (
              <motion.div key="ex6" className="row ex" custom={dir} {...vp}>
                <div className="top">
                  <div className="col">
                    <div className="title">Settings</div>
                    <div className="artist">
                      {hideFs && fsApp === "windowed" ? "Hidden: fullscreen app" : !hideUD && !hideLR ? "Always visible" : hideUD && hideLR ? "Hides on every edge" : hideUD ? "Hides when up or down" : "Hides when left or right"}
                      {autoOn ? " · Starts with Windows" : ""}
                    </div>
                  </div>
                </div>
                <div className="cfgcol">
                  <div className="cfg">
                    <span className="hint">Up / down</span>
                    <div className="seg" role="group" aria-label="Auto-hide on top and bottom">
                      <button className={hideUD ? "" : "on"} onClick={() => setHidePref("ud", false)}>Off</button>
                      <button className={hideUD ? "on" : ""} onClick={() => setHidePref("ud", true)}>On</button>
                    </div>
                  </div>
                  <div className="cfg">
                    <span className="hint">Left / right</span>
                    <div className="seg" role="group" aria-label="Auto-hide on left and right">
                      <button className={hideLR ? "" : "on"} onClick={() => setHidePref("lr", false)}>Off</button>
                      <button className={hideLR ? "on" : ""} onClick={() => setHidePref("lr", true)}>On</button>
                    </div>
                  </div>
                  <div className="cfg">
                    <span className="hint">Fullscreen</span>
                    <div className="seg" role="group" aria-label="Hide while a windowed fullscreen app is in front">
                      <button className={hideFs ? "" : "on"} onClick={() => setHideFsPref(false)}>Off</button>
                      <button className={hideFs ? "on" : ""} onClick={() => setHideFsPref(true)}>On</button>
                    </div>
                  </div>
                  <div className="cfg">
                    <span className="hint">Start with Windows</span>
                    <div className="seg" role="group" aria-label="Start IslandWin with Windows">
                      <button className={autoOn ? "" : "on"} onClick={() => setAuto(false)}>Off</button>
                      <button className={autoOn ? "on" : ""} onClick={() => setAuto(true)}>On</button>
                    </div>
                  </div>
                </div>
              </motion.div>
            )}
          </AnimatePresence>
          <motion.div className="dots" initial={false} animate={{ opacity: mode === "expanded" ? 1 : 0 }} style={{ pointerEvents: mode === "expanded" ? "auto" : "none" }}>
            {Array.from({ length: PAGES }, (_, i) => <button key={i} aria-label={`Page ${i + 1}`} className={page === i ? "on" : ""} onClick={() => { setDir(i > page ? 1 : -1); setPage(i); }} />)}
          </motion.div>
        </motion.div>
      </motion.div>
    </div>
  );
}
