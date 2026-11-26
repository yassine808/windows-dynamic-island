// Streaming speech recognition through the browser engine that ships inside WebView2 (Edge speech
// service): interim words appear while you talk, finals land in a few hundred ms, no API key and no
// per-request quota. Everything stays CPU-cheap because the audio never leaves through our code.
type Rec = any;

export type SpeechHandlers = {
  partial: (t: string) => void; // live, unconfirmed text
  final: (t: string) => void; // one finished phrase
  error: (msg: string) => void; // engine gave up: caller should fall back
  end: () => void; // recognizer is done for good
};

export const speechAvailable = () =>
  typeof window !== "undefined" && !!((window as any).SpeechRecognition || (window as any).webkitSpeechRecognition);

// these never recover on their own; no-speech and aborted are normal during hold-to-talk
const FATAL = new Set(["not-allowed", "service-not-allowed", "network", "audio-capture", "language-not-supported"]);

function hint(code: string) {
  if (code === "not-allowed" || code === "service-not-allowed") return "Microphone blocked, using Windows speech";
  if (code === "network") return "Speech service unreachable, using Windows speech";
  if (code === "audio-capture") return "No microphone, using Windows speech";
  return `Speech error: ${code}`;
}

export function createSpeech(h: SpeechHandlers) {
  const w = window as any;
  const rec: Rec = new (w.SpeechRecognition || w.webkitSpeechRecognition)();
  rec.lang = navigator.language || "en-US";
  rec.continuous = true;
  rec.interimResults = true;
  rec.maxAlternatives = 1;

  let want = false; // the user still wants to listen
  let started = false; // rec.start() accepted
  let dead = false; // fatal error, do not restart
  let restarts = 0;
  let startedAt = 0;
  let flapping = 0; // sessions that die within 400 ms of starting

  rec.onstart = () => { startedAt = performance.now(); };

  rec.onresult = (e: Rec) => {
    let interim = "";
    for (let i = e.resultIndex; i < e.results.length; i++) {
      const r = e.results[i];
      const t = String(r[0]?.transcript ?? "").trim();
      if (!t) continue;
      if (r.isFinal) h.final(t);
      else interim = `${interim} ${t}`.trim();
    }
    h.partial(interim);
  };

  rec.onerror = (e: Rec) => {
    const code = String(e?.error ?? "error");
    if (code === "no-speech" || code === "aborted") return; // transient, keep listening
    if (!FATAL.has(code)) return;
    dead = true;
    want = false;
    h.error(hint(code));
    h.end();
  };

  // Chromium ends the session on its own after long silence; restart while the user still holds on.
  rec.onend = () => {
    started = false;
    if (dead || !want) return h.end();
    // a service that closes right after opening will never produce words: give up and fall back
    flapping = startedAt && performance.now() - startedAt < 400 ? flapping + 1 : 0;
    if (flapping > 3) {
      dead = true;
      want = false;
      h.error("Speech service keeps closing, using Windows speech");
      h.end();
      return;
    }
    if (++restarts > 40) return h.end();
    try { rec.start(); started = true; }
    catch { setTimeout(() => { if (want && !dead) { try { rec.start(); started = true; } catch {} } }, 250); }
  };

  return {
    start() {
      if (dead) return;
      want = true;
      if (started) return;
      try { rec.start(); started = true; } catch {}
    },
    stop() {
      want = false;
      try { rec.stop(); } catch {}
    },
    abort() {
      want = false;
      dead = true;
      try { rec.abort(); } catch {}
    },
  };
}
