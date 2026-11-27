import { useEffect, useRef, useState, type CSSProperties } from "react";
import { AnimatePresence, animate, motion, MotionValue, useMotionValue, useTransform } from "framer-motion";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { createSpeech, speechAvailable } from "./speech";

type Media = { title: string; artist: string; playing: boolean; position: number; duration: number; at: number } | null;
type Stt = { active: boolean; partial: string; text: string; err: string };
type Mode = "idle" | "media" | "listening" | "alarm" | "expanded" | "timer";
type Tm = { total: number; left: number; end: number; running: boolean; done: boolean };
type Mix = { vol: number; mute: boolean; mic: number; mic_mute: boolean; ok: boolean };
type St = { cpu: number; ram: number; gpu: number; down: number; up: number };
type MicInfo = { engine: boolean; perm: string };
const PAGES = 6; // 0 media, 1 dictation, 2 timer, 3 mixer, 4 stats, 5 settings
// Each page keeps its own height. The settings rows need more room than the compact pages.
const PAGE_H = [190, 132, 160, 140, 150, 286];
// The overlay window is only as big as the largest island plus room for the halo and the drag
// shadow. The island is still positioned in desktop coordinates; the window is just the patch of
// screen it is painted on, and Rust moves that patch to follow it.
const BOX_MAX_W = 370, BOX_MAX_H = 286, BOX_PAD = 80;
const BOX_W = BOX_MAX_W + BOX_PAD * 2, BOX_H = BOX_MAX_H + BOX_PAD * 2;
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
// Browser preview only: `?nomedia` starts with nothing playing, so the idle clock can be checked.
const DEMO = !TAURI && !new URLSearchParams(location.search).has("nomedia");
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
  if (m === "listening" || m === "alarm") return { w: 340, h: 84, r: 42 };
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

type MediaSnap = Omit<NonNullable<Media>, "at">;
type ArtReply = { key: string; art: string };

function useMedia() {
  const [media, setMedia] = useState<Media>(DEMO ? { title: "Blinding Lights", artist: "The Weeknd", playing: true, position: 42000, duration: 215000, at: performance.now() } : null);
  const [art, setArt] = useState<string | null>(DEMO ? demoArt : null);
  const last = useRef("");
  const freeze = useRef(0); // after a seek, ignore the (stale) polled position for a moment
  // Every snapshot has a generation. A poll or thumbnail that started before a newer song was
  // announced is dropped instead of overwriting what is playing now.
  const gen = useRef(0);
  const artGen = useRef(0);

  const apply = (m: MediaSnap | null, g: number) => {
    if (g !== gen.current) return;
    const now = performance.now();
    setMedia((prev) => {
      if (!m) return null;
      // Keep the old anchor while the player reports the same position. Resetting `at` on every
      // poll made a stale position stand still (and the remaining time drift away).
      if (now < freeze.current && prev) return { ...m, position: prev.position, at: prev.at };
      if (prev && prev.title === m.title && prev.artist === m.artist && prev.position === m.position && prev.duration === m.duration && prev.playing === m.playing) {
        return prev;
      }
      return { ...m, at: now };
    });
    const k = m ? `${m.title}|${m.artist}` : "";
    if (k === last.current) return;
    last.current = k;
    const ag = ++artGen.current;
    setArt(null); // never keep the previous cover under a new title while its own art loads
    if (!m) return;
    // The song can switch between reading the metadata and reading the thumbnail, in which case
    // Rust refuses to hand back the wrong cover. Ask again for the title the UI is showing now.
    const loadArt = (tries: number) => {
      invoke<ArtReply | null>("media_art", { key: k })
        .then((r) => {
          if (ag !== artGen.current) return;
          if (r && r.key === k) { setArt(r.art); return; }
          if (tries < 3) window.setTimeout(() => loadArt(tries + 1), 250);
        })
        .catch(() => {});
    };
    loadArt(0);
  };

  useEffect(() => {
    if (!TAURI) return;
    let stopped = false;
    let timer = 0;
    // Position anchor, once a second, sequential: a slow WinRT call can never queue up behind itself.
    const tick = async () => {
      const g = ++gen.current;
      try {
        const m = await invoke<MediaSnap | null>("media_poll");
        if (!stopped) apply(m, g);
      } catch {
        if (!stopped) apply(null, g);
      }
      if (!stopped) timer = window.setTimeout(tick, 1000);
    };
    tick();
    // Rust pushes a new song, artist or play/pause as soon as the media session reports it; this is
    // what removes the wait for the next poll.
    let un: (() => void) | undefined, gone = false;
    listen<MediaSnap | null>("media", (e) => {
      gen.current++; // supersede any poll that started before this change
      apply(e.payload, gen.current);
    })
      .then((f) => { if (gone) f(); else un = f; })
      .catch((err) => dbg("media listener failed: " + String(err)));
    return () => {
      stopped = true;
      window.clearTimeout(timer);
      gone = true;
      un?.();
    };
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
  const [mic, setMic] = useState<MicInfo>({ engine: speechAvailable(), perm: "unknown" });
  const last = useRef("");
  const lastPartialLog = useRef(0);
  const local = useRef(false); // the browser engine is running the show (no WinRT polling then)
  useEffect(() => {
    if (!TAURI) return;
    const ok = speechAvailable();
    local.current = ok;
    setMic((m) => ({ ...m, engine: ok }));
    dbg(`stt: browser engine ${ok ? "available" : "missing"}`);
    // no audio involved: just report what WebView2 thinks of the microphone
    navigator.permissions?.query({ name: "microphone" as PermissionName })
      .then((p) => {
        setMic((m) => ({ ...m, perm: p.state }));
        p.onchange = () => setMic((m) => ({ ...m, perm: p.state }));
        dbg(`stt: mic permission ${p.state}`);
      })
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
        if (now - lastPartialLog.current > 700) { lastPartialLog.current = now; dbg(`stt: interim ${t.length} chars`); }
      },
      final: (t) => {
        if (!alive) return;
        setStt((s) => ({ ...s, text: `${s.text}${t} `, partial: "" }));
        dbg(`stt: final ${t.length} chars`);
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
        // the words were typed into the focused field; nothing is kept in the app afterwards
        setStt((s) => (s.active || s.partial || s.text ? { ...s, active: false, partial: "", text: "" } : s));
        call("stt_ended"); // keep the Rust state machine in sync when the engine stops by itself
      },
      debug: (m) => dbg(`stt: ${m}`),
    });
    let gone = false;
    const offs: Array<() => void> = [];
    const sub = (p: Promise<() => void>) => p.then((f) => (gone ? f() : offs.push(f))).catch(() => {});
    sub(listen("stt://start", () => { if (!alive) return; reported = false; setStt((s) => ({ ...s, active: true, err: "", text: "", partial: "" })); rec.start(); }));
    sub(listen("stt://stop", () => rec.stop()));
    // Windows engine state changes land immediately instead of waiting for the next poll.
    sub(listen("stt://state", () => {
      if (!alive || local.current) return;
      invoke<Stt>("stt_state").then((s) => {
        const k = JSON.stringify(s);
        if (k !== last.current) { last.current = k; setStt(s); }
      }).catch(() => {});
    }));
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
  return { stt, setStt, mic };
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

// One audio context for the timer alarm, primed by the play button so the first chime can start
// even after a long countdown (browsers otherwise block audio that has no user gesture behind it).
let alarmCtx: AudioContext | null = null;
const primeAlarm = () => {
  try {
    alarmCtx ??= new AudioContext();
    if (alarmCtx.state === "suspended") void alarmCtx.resume();
  } catch {}
};
// A soft iOS-style two-phrase chime with a little space around it. It repeats until the caller
// invokes the returned stop function; there is no automatic timeout.
function startAlarmSound() {
  primeAlarm();
  const ctx = alarmCtx;
  if (!ctx) return () => {};
  const master = ctx.createGain();
  const dry = ctx.createGain();
  const wet = ctx.createGain();
  const delay = ctx.createDelay(0.5);
  const feedback = ctx.createGain();
  dry.gain.value = 0.16;
  wet.gain.value = 0.07;
  delay.delayTime.value = 0.23;
  feedback.gain.value = 0.24;
  dry.connect(master);
  delay.connect(feedback).connect(delay);
  delay.connect(wet).connect(master);
  master.connect(ctx.destination);
  let stopped = false;
  const note = (freq: number, at: number, len: number, gain: number) => {
    const a = ctx.createOscillator(), b = ctx.createOscillator(), g = ctx.createGain();
    a.type = "sine";
    a.frequency.value = freq;
    b.type = "triangle";
    b.frequency.value = freq * 2.002;
    const end = at + len;
    g.gain.setValueAtTime(0.0001, at);
    g.gain.exponentialRampToValueAtTime(gain, at + 0.015);
    g.gain.exponentialRampToValueAtTime(0.0001, end);
    a.connect(g); b.connect(g); g.connect(dry); g.connect(delay);
    a.start(at); b.start(at); a.stop(end + 0.02); b.stop(end + 0.02);
  };
  const ring = () => {
    if (stopped) return;
    const t = ctx.currentTime + 0.04;
    const notes: [number, number, number, number][] = [
      [659.25, 0.00, 0.55, 0.10], [987.77, 0.16, 0.55, 0.09], [1318.51, 0.32, 0.85, 0.075],
      [987.77, 0.62, 0.45, 0.055], [659.25, 1.05, 0.55, 0.09], [987.77, 1.21, 0.55, 0.08],
      [1318.51, 1.37, 0.95, 0.07],
    ];
    notes.forEach(([f, o, l, g]) => note(f, t + o, l, g));
  };
  ring();
  const id = window.setInterval(ring, 3400);
  return () => {
    stopped = true;
    clearInterval(id);
    try {
      master.gain.cancelScheduledValues(ctx.currentTime);
      master.gain.setTargetAtTime(0, ctx.currentTime, 0.04);
    } catch {}
    window.setTimeout(() => { try { master.disconnect(); } catch {} }, 600);
  };
}

const clock12 = (d: Date, short = false) => {
  const t = d.toLocaleTimeString("en-US", { hour: "numeric", minute: "2-digit", hour12: true });
  return short ? t.replace(/\s*[AP]M$/i, "") : t;
};
function ClockText({ short }: { short?: boolean }) {
  const [d, setD] = useState(() => new Date());
  useEffect(() => { const id = setInterval(() => setD(new Date()), 1000); return () => clearInterval(id); }, []);
  return <span className="pilltime">{clock12(d, short)}</span>;
}
function ProgressText({ m }: { m: NonNullable<Media> }) {
  const [, f] = useState(0);
  useEffect(() => { const id = setInterval(() => f((n) => n + 1), 500); return () => clearInterval(id); }, []);
  const d = m.duration > 0 ? m.duration : 0;
  const pos = Math.min(d || 1e12, Math.max(0, m.position + (m.playing ? performance.now() - m.at : 0)));
  return <span className="pilltime">{fmt(pos)}</span>;
}
// The running timer takes the player's place in the collapsed pill, with a stopwatch so the two
// times can never be confused. Same .pilltime as the track, so both are exactly the same size.
function TimerText({ left, done }: { left: number; done: boolean }) {
  return <span className="timerstat">{Icon.timer}<span className="pilltime">{done ? "Done" : fmtT(left)}</span></span>;
}
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
const Bars = ({ playing, level }: { playing: boolean; level?: MotionValue<number> }) => {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const el = ref.current;
    if (!el || !playing || level) return;
    // Ten frames a second instead of a 60 fps CSS animation. The waveform is five small bars, so
    // the eye cannot tell; the compositor, however, only draws when a bar actually changes instead
    // of presenting the whole window on every vsync while music plays.
    const kids = Array.from(el.children) as HTMLElement[];
    const delays = [0, -0.3, -0.6, -0.15, -0.45];
    const draw = () => {
      const t = performance.now() / 900;
      for (let i = 0; i < kids.length; i++) {
        const p = ((t + delays[i]) % 1 + 1) % 1;
        const f = 0.3 + 0.7 * (0.5 - 0.5 * Math.cos(2 * Math.PI * p));
        kids[i].style.transform = `scaleY(calc(var(--h) * ${f.toFixed(3)}))`;
      }
    };
    draw();
    const id = window.setInterval(draw, 100);
    return () => { window.clearInterval(id); kids.forEach((k) => { k.style.transform = ""; }); };
  }, [playing, level]);
  if (level) return <div className="bars">{GAIN.map((k, i) => <Bar key={i} level={level} k={k} />)}</div>;
  return (
    <div ref={ref} className={`bars ${playing ? "live" : "paused"}`}>
      {PROFILE.map((h, i) => <i key={i} style={{ "--h": h } as CSSProperties} />)}
    </div>
  );
};

// both times round down, so the clock never runs ahead of the audio
const fmt = (ms: number) => { const s = Math.max(0, Math.floor(ms / 1000)); return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`; };
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
      {d > 0 && <span>{fmt(d)}</span>}
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
  bell: <svg viewBox="0 0 24 24" width="22" height="22" fill="currentColor"><path d="M12 2a2 2 0 0 0-2 2v.55C6.9 5.6 5 8.35 5 11.65V16l-2 2v1h18v-1l-2-2v-4.35c0-3.3-1.9-6.05-5-7.1V4a2 2 0 0 0-2-2zm0 20a3 3 0 0 0 3-3H9a3 3 0 0 0 3 3z" /></svg>,
  mic: <svg viewBox="0 0 24 24" width="20" height="20" fill="#fff"><path d="M12 14a3 3 0 0 0 3-3V6a3 3 0 0 0-6 0v5a3 3 0 0 0 3 3zm5-3a5 5 0 0 1-10 0H5a7 7 0 0 0 6 6.9V21h2v-3.1A7 7 0 0 0 19 11z" /></svg>,
  reset: <svg viewBox="0 0 24 24" width="15" height="15" fill="none" stroke="currentColor" strokeWidth="2.4" strokeLinecap="round" strokeLinejoin="round"><path d="M4.5 10a8 8 0 1 1 2.2 6.4" /><path d="M4.5 4.5V10H10" /></svg>,
  timer: <svg viewBox="0 0 24 24" width="15" height="15" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round"><circle cx="12" cy="13" r="8" /><path d="M12 9v4l2.6 2.1" /><path d="M9 2.5h6" /><path d="M12 2.5v2.5" /></svg>,
};

// play <-> pause morph, like SF Symbols' content transition
const glyphV = {
  initial: { scale: 0.55, opacity: 0, filter: "blur(4px)" },
  animate: { scale: 1, opacity: 1, filter: "blur(0px)", transition: { duration: 0.22, ease: [0.2, 0.8, 0.2, 1] } },
  exit: { scale: 0.55, opacity: 0, filter: "blur(4px)", transition: { duration: 0.14 } },
} as const;

// show the elapsed / total scrubber on the Media page (the iOS 17 layout). false = art, title and controls only
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
  const [pin, setPin] = useState(false); // tray "Settings" keeps the card open until the user is done
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
  const tmToggle = () => {
    primeAlarm();
    setTm((t) => {
      if (t.running) return { ...t, running: false, left: leftNow(t) || t.total };
      const left = t.done || t.left <= 0 ? t.total : t.left;
      if (left <= 0) return t;
      return { ...t, left, end: Date.now() + left * 1000, running: true, done: false };
    });
  };
  const tmSet = (total: number) => setTm((t) => ({ ...t, total, left: total, running: false, done: false }));
  const tmIdle = !tm.running && !tm.done && tm.left === tm.total;
  const tmH = Math.floor(tm.total / 3600), tmM = Math.floor((tm.total % 3600) / 60);
  const tmReset = () => setTm((t) => ({ ...t, left: t.total, running: false, done: false }));
  useEffect(() => {
    if (!tm.running) return;
    const id = setInterval(() => setTm((t) => {
      if (!t.running) return t;
      const left = leftNow(t);
      if (left <= 0) return { ...t, left: 0, running: false, done: true };
      return left === t.left ? t : { ...t, left };
    }), 250);
    return () => clearInterval(id);
  }, [tm.running]);
  // Ring until the user presses Stop: no quiet auto-dismiss after a few seconds.
  useEffect(() => {
    if (!tm.done) return;
    const stop = startAlarmSound();
    return stop;
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
  // step aside only for a true fullscreen-exclusive app; a windowed (borderless) fullscreen app keeps
  // the island visible and usable over it
  const [hideFs, setHideFs] = useState<boolean>(() => {
    try { return localStorage.getItem("hideFS") === "on"; } catch { return false; }
  });
  const [fsApp, setFsApp] = useState<"none" | "windowed" | "full">("none");
  const setHideFsPref = (v: boolean) => {
    setHideFs(v);
    try { localStorage.setItem("hideFS", v ? "on" : "off"); } catch {}
  };
  // Lock the island in place: buttons and scrolling still work, but a drag on the card does nothing.
  const [posLock, setPosLock] = useState<boolean>(() => {
    try { return localStorage.getItem("posLock") === "on"; } catch { return false; }
  });
  const setPosLockPref = (v: boolean) => {
    setPosLock(v);
    try { localStorage.setItem("posLock", v ? "on" : "off"); } catch {}
  };
  // Start with Windows: the setup turns it on while installing, this switch flips the same Run value.
  // (The browser demo has no registry, so it shows the shipping default.)
  const [autoOn, setAutoOn] = useState<boolean>(() => !TAURI);
  useEffect(() => { if (TAURI) invoke<boolean>("autostart_get").then(setAutoOn).catch(() => {}); }, []);
  // The tray menu can flip the same Run value while the Settings page is closed.
  useEffect(() => {
    if (!TAURI) return;
    let gone = false;
    let un: (() => void) | undefined;
    listen<boolean>("autostart://changed", (e) => setAutoOn(e.payload))
      .then((f) => { if (gone) f(); else un = f; })
      .catch(() => {});
    return () => { gone = true; un?.(); };
  }, []);
  const setAuto = (on: boolean) => {
    setAutoOn(on);
    call("autostart_set", { on }).catch((e) => { setAutoOn(!on); setRecErr(`Autostart: ${String(e)}`); });
  };
  useEffect(() => {
    if (!TAURI) return;
    let gone = false;
    let un: (() => void) | undefined;
    listen<{ full: boolean; windowed: boolean; exclusive: boolean }>("fs", (e) => setFsApp(e.payload.exclusive ? "full" : e.payload.full ? "windowed" : "none"))
      .then((f) => { if (gone) f(); else un = f; })
      .catch(() => {});
    return () => { gone = true; un?.(); };
  }, []);
  // Tray icon: left click or "IslandWin Settings" opens the island on the Settings page and keeps
  // it there long enough to use it without chasing the hover state.
  useEffect(() => {
    if (!TAURI) return;
    let gone = false;
    let un: (() => void) | undefined;
    listen("show://settings", () => {
      setDir(1);
      setPage(5);
      setTucked(false);
      setPin(true);
      invoke<boolean>("autostart_get").then(setAutoOn).catch(() => {});
    }).then((f) => { if (gone) f(); else un = f; }).catch(() => {});
    return () => { gone = true; un?.(); };
  }, []);
  useEffect(() => {
    if (!pin) return;
    const id = window.setTimeout(() => setPin(false), 12000);
    return () => clearTimeout(id);
  }, [pin]);

  // ---- mixer / stats state ----
  const [mix, setMix] = useState<Mix>({ vol: 0.5, mute: false, mic: 0.8, mic_mute: false, ok: true });
  const [slOn, setSlOn] = useState<string | null>(null);
  const [st, setSt] = useState<St>({ cpu: 0, ram: 0, gpu: -1, down: 0, up: 0 });
  const mixFreeze = useRef(0);
  const sendT = useRef<Record<string, number>>({});
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
  // Spawn: the island is hidden until the monitor list arrives, seeded at its final edge position
  // at full size, then scaled up from that edge point. Scaling (instead of animating width and
  // height) is what makes it grow out of the edge like a tree, with no sideways slide.
  const [placed, setPlaced] = useState(!TAURI);
  // Desktop coordinates are unchanged; this shift puts the island inside the small overlay window.
  // Rust moves the window, the island itself never moves because of it. Motion values keep the
  // shift off React's render path: it changes on every frame of a drag or a spring.
  const ox = useMotionValue(0), oy = useMotionValue(0);
  const panX = useTransform(ox, (v) => -v), panY = useTransform(oy, (v) => -v);
  const orgRef = useRef({ x: 0, y: 0 });
  const w = useMotionValue(126), h = useMotionValue(37), r = useMotionValue(18.5), lift = useMotionValue(1), flat = useMotionValue(1);
  const spawn = useMotionValue(TAURI ? 0.2 : 1);
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
  const scale = useTransform([lift, spawn], (v: number[]) => v[0] * v[1]);

  useMic(stt.active, level);

  const vertical = edge === "left" || edge === "right";
  // With the fullscreen setting on, a true fullscreen-exclusive D3D app hides the island completely.
  // A windowed / borderless fullscreen app (a game that hides the cursor while playing included)
  // only tucks it into its edge: out of the way, but never gone, so it can always be reached. Its
  // input is still paused in Rust while the app captures the mouse.
  // An alarm always stays visible: it has to be reachable to press Stop, even over a fullscreen app.
  const fsHide = hideFs && fsApp === "full" && !pin && !tm.done;
  const fsTuck = hideFs && fsApp === "windowed" && !pin && !tm.done;
  useEffect(() => {
    if (!TAURI) return;
    const why = fsHide ? "island hidden" : fsTuck ? "island tucked" : pin && fsApp !== "none" ? "settings pinned" : tm.done && fsApp !== "none" ? "alarm visible" : "island visible";
    dbg(`fullscreen: ${fsApp} (${why})`);
  }, [fsApp, fsHide, fsTuck, pin, tm.done]);
  // The alarm must stay visible until Stop is pressed, so hovering does not open another page over it.
  const expanded = (hover || pin) && !dragging && !tm.done;
  // Hovering while dictating expands the full report; away from it, the compact island shows the words.
  const mode: Mode = tm.done ? "alarm" : stt.active && !expanded ? "listening" : expanded ? "expanded" : media ? "media" : timerActive ? "timer" : "idle";
  const modeRef = useRef(mode); modeRef.current = mode;
  const full = (stt.text + " " + stt.partial).trim();
  const done = stt.text.split(/\s+/).filter(Boolean), live = stt.partial.split(/\s+/).filter(Boolean);
  // a toggle can be triggered twice by one physical click; ignore the echo
  const gate = useRef(0);
  const once = () => { const n = performance.now(); if (n - gate.current < 350) return false; gate.current = n; return true; };
  const toggleStt = () => !once() ? undefined : (TAURI ? call("stt_toggle") : setStt((s) => ({ ...s, active: !s.active, text: s.active ? s.text : "Book a table for two", partial: s.active ? "" : "on Friday at seven" })));

  useEffect(() => { if (stt.active) { setDir(0); setPage(1); } }, [stt.active]);
  // refs read from the pointer handler and the timers below (they run outside React's render)
  const sttRef = useRef(stt); sttRef.current = stt;
  const holdRef = useRef(cfg.hold); holdRef.current = cfg.hold;
  const posLockRef = useRef(posLock); posLockRef.current = posLock;
  useEffect(() => { if (TAURI) invoke<{ shortcut: string; hold: boolean }>("stt_settings").then(setCfg).catch(() => {}); }, []);
  const PRETTY: Record<string, string> = {
    Escape: "Esc", ArrowLeft: "Left", ArrowUp: "Up", ArrowRight: "Right", ArrowDown: "Down",
    MouseRight: "Right click", MouseMiddle: "Middle click", Mouse4: "Mouse 4", Mouse5: "Mouse 5",
    NumpadMultiply: "Num *", NumpadAdd: "Num +", NumpadSubtract: "Num -", NumpadDecimal: "Num .",
    NumpadDivide: "Num /", NumpadSeparator: "Num ,", NumpadEnter: "Num Enter", PrintScreen: "PrtSc",
    PageUp: "PgUp", PageDown: "PgDn", ContextMenu: "Menu", Backquote: "`", Minus: "-", Equal: "=",
  };
  const pretty = (k: string) => k.split("+").map((p) => PRETTY[p] ??
    (p.startsWith("Vk") ? `VK ${p.slice(2)}` : p.replace(/^Key/, "").replace(/^Digit/, "").replace(/^Numpad/, "Num "))).join(" + ");
  const combo = (e: { ctrlKey: boolean; altKey: boolean; shiftKey: boolean; metaKey: boolean }, key: string) =>
    [e.ctrlKey && "Ctrl", e.altKey && "Alt", e.shiftKey && "Shift", e.metaKey && "Super", key].filter(Boolean).join("+");
  const setHold = (hold: boolean) => { setCfg((c) => ({ ...c, hold })); call("set_stt_hold", { hold }); };
  const recordGate = useRef(0);
  const recordSeq = useRef(0);
  const resetShortcut = async () => {
    recordSeq.current++;
    setRecErr("");
    if (rec) {
      setRec(false);
      if (TAURI) call("cancel_record_shortcut");
    }
    try {
      const k = TAURI ? await invoke<string>("reset_shortcut") : "Ctrl+Alt+Space";
      setCfg((c) => ({ ...c, shortcut: k }));
      dbg(`shortcut reset: ${k}`);
    } catch (e) {
      setRecErr(String(e));
      dbg(`shortcut reset failed: ${String(e)}`);
    }
  };
  const record = async () => {
    // Cancel is immediate (a second press on the chip must never be swallowed); only re-arming is debounced.
    if (rec) {
      dbg("shortcut record: cancelled from the UI");
      recordSeq.current++;
      setRec(false);
      if (TAURI) call("cancel_record_shortcut"); else recCancel.current?.();
      return;
    }
    const now = performance.now();
    if (now - recordGate.current < 400) return;
    recordGate.current = now;
    const seq = ++recordSeq.current;
    setRecErr("");
    setRec(true);
    dbg("shortcut record: armed, waiting for a key or mouse button");
    if (TAURI) {
      try {
        const k = await invoke<string>("record_shortcut");
        if (seq !== recordSeq.current) return;
        setCfg((c) => ({ ...c, shortcut: k }));
        setRecErr("");
        dbg(`shortcut record: saved ${k}`);
      } catch (e) {
        if (seq !== recordSeq.current) return;
        const msg = String(e);
        if (msg !== "cancelled") { setRecErr(msg); dbg(`shortcut record: rejected (${msg})`); }
      }
    } else {
      try {
        const k = await new Promise<string>((res, rej) => {
          const done = (k?: string) => {
            removeEventListener("keydown", h, true);
            removeEventListener("mousedown", m, true);
            removeEventListener("contextmenu", ctx, true);
            recCancel.current = null;
            if (k == null) rej("cancelled"); else res(k);
          };
          const h = (e: KeyboardEvent) => {
            if (e.repeat || /^(Control|Shift|Alt|Meta|OS|AltGraph)/.test(e.code)) return;
            e.preventDefault(); e.stopPropagation();
            done(combo(e, e.code));
          };
          const m = (e: MouseEvent) => {
            if (e.button === 0) return; // left click is never a shortcut
            e.preventDefault(); e.stopPropagation();
            const button = e.button === 1 ? "MouseMiddle" : e.button === 2 ? "MouseRight" : e.button === 3 ? "Mouse4" : "Mouse5";
            done(combo(e, button));
          };
          const ctx = (e: Event) => e.preventDefault();
          addEventListener("keydown", h, true);
          addEventListener("mousedown", m, true);
          addEventListener("contextmenu", ctx, true);
          recCancel.current = () => done();
        });
        if (seq !== recordSeq.current) return;
        setCfg((c) => ({ ...c, shortcut: k }));
        dbg(`shortcut record: saved ${k}`);
      } catch (e) {
        if (seq !== recordSeq.current) return;
        if (String(e) !== "cancelled") setRecErr(String(e));
      }
    }
    if (seq === recordSeq.current) setRec(false);
  };

  // While recording, the island takes the keyboard (Rust lifts its no-activate style), so the
  // webview sees the keys itself. That is the path that still works when the foreground app is
  // elevated and a low-level hook gets nothing; Rust takes whichever source reports first.
  useEffect(() => {
    if (!TAURI || !rec) return;
    const submit = (key: string) => call("record_submit", { key }).catch((err) => setRecErr(String(err)));
    const h = (e: KeyboardEvent) => {
      if (e.repeat || /^(Control|Shift|Alt|Meta|OS|AltGraph)/.test(e.code)) return;
      e.preventDefault(); e.stopPropagation();
      submit(combo(e, e.code));
    };
    const m = (e: MouseEvent) => {
      if (e.button === 0) return; // left click is never a shortcut
      e.preventDefault(); e.stopPropagation();
      const button = e.button === 1 ? "MouseMiddle" : e.button === 2 ? "MouseRight" : e.button === 3 ? "Mouse4" : "Mouse5";
      submit(combo(e, button));
    };
    const ctx = (e: Event) => e.preventDefault();
    addEventListener("keydown", h, true);
    addEventListener("mousedown", m, true);
    addEventListener("contextmenu", ctx, true);
    return () => {
      removeEventListener("keydown", h, true);
      removeEventListener("mousedown", m, true);
      removeEventListener("contextmenu", ctx, true);
    };
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
    if (dragging || !placed) return;
    const s = sizeOf(mode, vertical, hover, page);
    const t = target(edge, along.current, s, tucked, monRef.current);
    const key = `${edge}|${nonce}`, fly = key !== flyKey.current; // fly = just released from a drag
    flyKey.current = key;
    animate(w, s.w, spring); animate(h, s.h, spring); animate(r, s.r, spring);
    animate(ex, t.ex, fly ? move : settle); animate(ey, t.ey, fly ? move : settle);
    animate(fx, t.fx, move); animate(fy, t.fy, move);
    animate(lift, 1, move); animate(flat, 1, spring);
  }, [edge, mode, hover, tucked, dragging, nonce, page, placed]);

  // tuck into the edge when idle (if that edge's axis is enabled)
  useEffect(() => {
    setTucked(false);
    // A fullscreen-exclusive app owns the display: hide completely instead of tucking, unless the
    // tray icon explicitly asked for Settings.
    if (fsHide) return;
    // A windowed / borderless fullscreen app tucks the island even when this edge's own auto-hide is
    // off: the fullscreen setting is what asks for it, and a sliver stays reachable.
    const can = fsTuck || (edge === "top" || edge === "bottom" ? hideUD : hideLR);
    if (!can || hover || pin || dragging || stt.active || tm.done) return;
    const id = setTimeout(() => setTucked(true), fsTuck ? 1200 : 2500);
    return () => clearTimeout(id);
  }, [edge, hover, pin, dragging, stt.active, nonce, hideUD, hideLR, fsHide, fsTuck, tm.done]);

  // Keep the overlay window over the island. The box is the largest island plus room for the glow
  // and the drag shadow, anchored to the same edge point, so the island can grow, tuck and spring
  // inside it without ever reaching the window edge.
  useEffect(() => {
    if (!TAURI || !placed) return;
    let raf = 0, last = "";
    const push = () => {
      raf = 0;
      const bx = Math.round(ex.get() - BOX_MAX_W * fx.get() - BOX_PAD);
      const by = Math.round(ey.get() - BOX_MAX_H * fy.get() - BOX_PAD);
      const k = `${bx},${by}`;
      if (k === last) return;
      last = k;
      orgRef.current = { x: bx, y: by };
      ox.set(bx); oy.set(by);
      invoke("set_box", { x: bx, y: by, w: BOX_W, h: BOX_H }).catch(() => {});
    };
    const sched = () => { if (!raf) raf = requestAnimationFrame(push); };
    const stops = [ex, ey, fx, fy].map((v) => v.on("change", sched));
    sched();
    return () => { stops.forEach((off) => off()); if (raf) cancelAnimationFrame(raf); };
  }, [placed]);

  // report hit rect so Rust can toggle click-through; driven by the springs themselves, so an idle
  // island costs nothing instead of running a 60 Hz timer
  useEffect(() => {
    if (!TAURI) return;
    if (!placed) return; // nothing to hit until the spawn placement is known
    if (fsHide) {
      // invisible means untouchable: keep the click-through target at zero so a game still gets the mouse
      invoke("set_hit_rect", { x: 0, y: 0, w: 0, h: 0 });
      return;
    }
    let last = "";
    const push = () => {
      const s = lift.get() * spawn.get(), cw = w.get() * s, ch = h.get() * s;
      const px = x.get() - (cw - w.get()) / 2, py = y.get() - (ch - h.get()) / 2;
      const k = [px, py, cw, ch].map((n) => Math.round(n)).join();
      if (k !== last) { last = k; invoke("set_hit_rect", { x: px, y: py, w: cw, h: ch }); }
    };
    const stops = [x, y, w, h, lift, spawn].map((v) => v.on("change", push));
    push();
    return () => stops.forEach((off) => off());
  }, [fsHide, placed]);

  // drag + snap
  const d = useRef<{ gx: number; gy: number; sx: number; sy: number; on: boolean; vx: number; vy: number; lx: number; ly: number; lt: number; lock: boolean; hz: boolean; mid: number } | null>(null);
  const pdown = (cx: number, cy: number) => {
    if (posLockRef.current) return; // locked in place: buttons still work, dragging does not
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
    let hand = false; // last cursor shape we pushed to the OS
    const frac = (p: P) => (sk ? (p.x - orgRef.current.x - sk.left) / sk.width : 0);
    const onPtr = (p: P) => {
      // Pointer coordinates are desktop coordinates; element hit-testing and DOM rects are window
      // coordinates, which start at the overlay's top-left corner.
      const vx = p.x - orgRef.current.x, vy = p.y - orgRef.current.y;
      if (!d.current?.on) setHover(p.inside);
      // Keep the OS cursor honest even when WebView2 misses the element change: hand only over a
      // button or a seek bar, arrow everywhere else.
      const overHand = !!document.elementFromPoint(vx, vy)?.closest("button, .seek:not(.off)");
      if (overHand !== hand) { hand = overHand; invoke("set_cursor", { hand }).catch(() => {}); }
      const f = fn.current;
      if (p.down && !active) {
        if (!p.inside) return;
        active = true;
        const el = document.elementFromPoint(vx, vy);
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
          else if (document.elementFromPoint(vx, vy)?.closest("button") === btn) btn.click();
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

  // all monitors (the overlay spans them), primary first. Rust re-sends this list whenever Windows
  // changes the display layout (resolution switch, Parsec / remote display), so the island follows
  // the desktop instead of staying at the old monitor's coordinates.
  useEffect(() => {
    if (!TAURI) return;
    let seeded = false;
    let un: (() => void) | undefined, gone = false;
    const apply = (l: number[][], initial: boolean) => {
      if (!l.length) return;
      const next = l.map(([x, y, w, h]) => ({ x, y, w, h }));
      const prev = monRef.current;
      const prevPrimary = mons.current[0];
      mons.current = next;
      // A Parsec / remote display can become the new primary: follow it, otherwise keep the island
      // on the monitor it was on if that monitor survived, and fall back to the primary.
      const primaryMoved = next[0].x !== prevPrimary.x || next[0].y !== prevPrimary.y;
      const near = (q: Rect) => Math.hypot(q.x + q.w / 2 - (prev.x + prev.w / 2), q.y + q.h / 2 - (prev.y + prev.h / 2));
      const keep = primaryMoved ? next[0] : next.find((q) => q.x === prev.x && q.y === prev.y) ?? next.reduce((a, q) => (near(q) < near(a) ? q : a));
      monRef.current = keep;
      const horiz = edgeRef.current === "top" || edgeRef.current === "bottom";
      if (initial) {
        along.current = keep.x + keep.w / 2;
        // Seed the final position with the real starting size, so the grow-in is a pure scale from
        // the edge: no spring can drag the centre sideways after the first frame.
        const s = sizeOf(modeRef.current, edgeRef.current === "left" || edgeRef.current === "right", false, 0);
        const t = target(edgeRef.current, along.current, s, false, keep);
        ex.set(t.ex); ey.set(t.ey); fx.set(t.fx); fy.set(t.fy);
        dbg(`spawn: seeded at ${Math.round(t.ex)},${Math.round(t.ey)} on ${Math.round(keep.w)}x${Math.round(keep.h)}`);
      } else {
        // same relative spot along the edge, rescaled to the monitor's new size
        const span = horiz ? prev.w : prev.h;
        const f = span > 0 ? clamp01((along.current - (horiz ? prev.x : prev.y)) / span) : 0.5;
        along.current = (horiz ? keep.x : keep.y) + f * (horiz ? keep.w : keep.h);
        dbg(`display change: island moved to ${Math.round(keep.x)},${Math.round(keep.y)} ${Math.round(keep.w)}x${Math.round(keep.h)}`);
      }
      setNonce((n) => n + 1);
    };
    const onMons = (l: number[][]) => {
      if (!l.length) return;
      if (!seeded) { apply(l, true); seeded = true; setPlaced(true); }
      else apply(l, false);
    };
    listen<number[][]>("mons", (e) => onMons(e.payload))
      .then((f) => { if (gone) f(); else un = f; })
      .catch((err) => dbg("mons listener failed: " + String(err)));
    invoke<number[][]>("get_monitors").then(onMons).catch(() => setPlaced(true));
    return () => { gone = true; un?.(); };
  }, []);

  // The first frame after placement is the seed at the edge; pop it open from there.
  useEffect(() => { if (placed) animate(spawn, 1, spring); }, [placed]);

  const control = (action: string) => {
    if (!TAURI && media) setMedia({ ...media, playing: action === "toggle" ? !media.playing : media.playing });
    call("media_control", { action });
  };
  const artEl = (big = false) => <div className={`art ${big ? "big" : ""}`} style={{ backgroundImage: art ? `url("${art}")` : undefined }} />;

  const siri = mode === "listening";
  // scale from the point where the island touches the screen edge, like a tree growing out of it
  const origin = edge === "top" ? "50% 0%" : edge === "bottom" ? "50% 100%" : edge === "left" ? "0% 50%" : "100% 50%";
  const box = { x, y, width: w, height: h, scale, transformOrigin: origin };
  const vp = { variants: pageV, initial: "initial", animate: "animate", exit: "exit" } as const;
  return (
    <div className={`stage ${dragging ? "dragging" : ""} ${fsHide ? "fsgone" : ""}`} style={{ ["--accent" as string]: tint ? `color-mix(in srgb, ${tint} 55%, white)` : "#fff" }}>
      <motion.div className="pan" style={{ x: panX, y: panY }}>
      <motion.div className="glow" style={{ ...box, borderRadius: rad }} animate={{ opacity: placed ? (siri ? 0.35 : tint && media ? (mode === "expanded" ? 0.16 : 0.1) : 0) : 0 }} transition={{ duration: 0.6 }}>
        <div className={`halo ${siri ? "siri" : ""}`} style={{ background: siri ? undefined : tint }} />
      </motion.div>
      <motion.div
        className="shell"
        style={{ ...box, opacity: placed ? 1 : 0 }}
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
                {!vertical && <div className="col center">{timerActive ? <TimerText left={tm.left} done={tm.done} /> : <ProgressText m={media} />}</div>}
                <Bars playing={media.playing} />
              </motion.div>
            )}
            {mode === "idle" && (
              <motion.div key={"idle" + vertical} className={`row idle ${vertical ? "v" : ""}`} custom={0} {...vp}>
                <ClockText />
              </motion.div>
            )}
            {mode === "timer" && (
              <motion.div key={"timer" + vertical} className={`row idle ${vertical ? "v" : ""}`} custom={0} {...vp}>
                <TimerText left={tm.left} done={tm.done} />
              </motion.div>
            )}
            {mode === "listening" && (
              <motion.div key="listening" className="row" style={{ padding: "0 20px" }} custom={0} {...vp}>
                <motion.button className="mic" aria-label="Stop dictation" onClick={toggleStt} animate={{ scale: [1, 1.08, 1] }} transition={{ repeat: Infinity, duration: 1.6 }}>{Icon.mic}</motion.button>
                <div className="col"><div className="tx sm">{full ? <div className="tw">{done.map((t, i) => <span key={"d" + i} className="w">{t} </span>)}{live.map((t, i) => <span key={"l" + i + t} className="w hint">{t} </span>)}</div> : <span className="hint">Listening…</span>}</div></div>
                <Bars playing level={level} />
              </motion.div>
            )}
            {mode === "alarm" && (
              <motion.div key="alarm" className="row alarm" custom={0} {...vp}>
                <motion.div className="alarmbell" animate={{ rotate: [0, -14, 12, -8, 6, 0] }} transition={{ repeat: Infinity, duration: 1.5, ease: "easeInOut" }}>{Icon.bell}</motion.div>
                <div className="col"><div className="title">Time's up</div><div className="artist">{fmtT(tm.total)} timer</div></div>
                <button className="stopbtn" onClick={tmReset}>Stop</button>
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
                    <div className={`artist ${stt.err || recErr ? "err" : ""}`}>
                      {recErr || stt.err || (rec ? "Press a key or mouse button…" : stt.active ? "Listening…" : cfg.hold ? "Hold to talk" : "Tap to talk")}
                    </div>
                  </div>
                  <Bars playing={stt.active} level={stt.active ? level : undefined} />
                </div>
                <div className="cfg">
                  <div className="chipgroup">
                    <button className={`chip short ${rec ? "rec" : ""}`} aria-label="Record shortcut" onClick={record}>
                      <i className="dot" />{rec ? (recErr || "Press keys or mouse…") : pretty(cfg.shortcut)}
                    </button>
                    <button className="chip reset" aria-label="Reset shortcut to Ctrl Alt Space" title="Reset to Ctrl + Alt + Space" onClick={resetShortcut}>{Icon.reset}</button>
                  </div>
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
                      <WheelCol v={tmH} max={23} label="h" on={(h) => tmSet(h * 3600 + tmM * 60)} />
                      <span className="wsep">:</span>
                      <WheelCol v={tmM} max={59} label="m" on={(m) => tmSet(tmH * 3600 + m * 60)} />
                    </div>
                    <button className="playbtn" aria-label="Start timer" onClick={tmToggle}>{Icon.play}</button>
                  </div>
                ) : (
                  <div className="top">
                    <Ring frac={tm.total ? tm.left / tm.total : 0} size={46} done={tm.done} />
                    <div className="col">
                      <div className="title big">{tm.done ? "Done" : fmtT(tm.left)}</div>
                      <div className="artist">{tm.running ? "Running" : tm.done ? "Time's up" : "Paused"}</div>
                    </div>
                    <button className="playbtn" aria-label={tm.running ? "Pause timer" : "Start timer"} onClick={tmToggle}>{tm.running ? Icon.pause : Icon.play}</button>
                  </div>
                )}
                <div className="cfg">
                  <div className="seg" role="group" aria-label="Presets">
                    {[5, 15, 25].map((m) => <button key={m} className={tm.total === m * 60 ? "on" : ""} onClick={() => tmSet(m * 60)}>{m}m</button>)}
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
                <div className="top">
                  <div className="col">
                    <div className="title">Settings</div>
                    <div className="artist">
                      {hideFs && fsApp !== "none" ? "Hidden for a fullscreen window or app" : !hideUD && !hideLR ? "Always visible" : hideUD && hideLR ? "Hides on every edge" : hideUD ? "Hides when up or down" : "Hides when left or right"}
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
                    <span className="hint" title="With this on, a windowed / borderless fullscreen app tucks the island into its edge and a fullscreen-exclusive app hides it completely">Fullscreen window / app</span>
                    <div className="seg" role="group" aria-label="Hide while a fullscreen window or app is in front">
                      <button className={hideFs ? "" : "on"} onClick={() => setHideFsPref(false)}>Off</button>
                      <button className={hideFs ? "on" : ""} onClick={() => setHideFsPref(true)}>On</button>
                    </div>
                  </div>
                  <div className="cfg">
                    <span className="hint">Lock position</span>
                    <div className="seg" role="group" aria-label="Lock the island in place">
                      <button className={posLock ? "" : "on"} onClick={() => setPosLockPref(false)}>Off</button>
                      <button className={posLock ? "on" : ""} onClick={() => setPosLockPref(true)}>On</button>
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
      </motion.div>
    </div>
  );
}
