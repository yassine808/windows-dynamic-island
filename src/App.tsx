import { useEffect, useRef, useState } from "react";
import { AnimatePresence, motion } from "framer-motion";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

type Media = { title: string; artist: string; playing: boolean } | null;
type Mode = "idle" | "media" | "listening" | "expanded";

// iOS-like Dynamic Island sizes
const SIZES: Record<Mode, { w: number; h: number; r: number }> = {
  idle: { w: 126, h: 37, r: 18.5 },
  media: { w: 250, h: 37, r: 18.5 },
  listening: { w: 340, h: 84, r: 42 },
  expanded: { w: 370, h: 162, r: 44 },
};

const spring = { type: "spring", stiffness: 380, damping: 30, mass: 0.9 } as const;
const fade = {
  initial: { opacity: 0, filter: "blur(6px)", scale: 0.92 },
  animate: { opacity: 1, filter: "blur(0px)", scale: 1 },
  exit: { opacity: 0, filter: "blur(6px)", scale: 0.92, transition: { duration: 0.12 } },
  transition: { duration: 0.25, delay: 0.06 },
};

function useMedia() {
  const [media, setMedia] = useState<Media>(null);
  const [art, setArt] = useState<string | null>(null);
  const lastKey = useRef("");

  useEffect(() => {
    const tick = async () => {
      try {
        const m = await invoke<Media>("media_poll");
        setMedia(m);
        const key = m ? `${m.title}|${m.artist}` : "";
        if (key !== lastKey.current) {
          lastKey.current = key;
          setArt(m ? await invoke<string | null>("media_art") : null);
        }
      } catch {
        setMedia(null);
      }
    };
    tick();
    const id = setInterval(tick, 1000);
    return () => clearInterval(id);
  }, []);

  return { media, art };
}

function useMic(active: boolean) {
  useEffect(() => {
    if (!active) return;
    let stream: MediaStream | undefined;
    let ctx: AudioContext | undefined;
    navigator.mediaDevices
      .getUserMedia({ audio: true })
      .then((s) => {
        stream = s;
        ctx = new AudioContext();
        // TODO: feed ctx/analyser levels to the bars, and PCM to Whisper
      })
      .catch(() => {});
    return () => {
      stream?.getTracks().forEach((t) => t.stop());
      ctx?.close();
    };
  }, [active]);
}

const Bars = ({ playing }: { playing: boolean }) => (
  <div className={`bars ${playing ? "live" : "paused"}`}>
    <i /><i /><i /><i />
  </div>
);

const Icon = {
  prev: <svg viewBox="0 0 24 24"><path d="M6 6h2v12H6zM9.5 12 18 6v12z" /></svg>,
  next: <svg viewBox="0 0 24 24"><path d="M16 6h2v12h-2zM6 18V6l8.5 6z" /></svg>,
  play: <svg viewBox="0 0 24 24"><path d="M8 5v14l11-7z" /></svg>,
  pause: <svg viewBox="0 0 24 24"><path d="M6 5h4v14H6zM14 5h4v14h-4z" /></svg>,
  mic: <svg viewBox="0 0 24 24" width="18" height="18" fill="#fff"><path d="M12 14a3 3 0 0 0 3-3V6a3 3 0 0 0-6 0v5a3 3 0 0 0 3 3zm5-3a5 5 0 0 1-10 0H5a7 7 0 0 0 6 6.9V21h2v-3.1A7 7 0 0 0 19 11z" /></svg>,
};

export default function App() {
  const { media, art } = useMedia();
  const [listening, setListening] = useState(false);
  const [hover, setHover] = useState(false);
  const [text, setText] = useState("");

  useMic(listening);

  useEffect(() => {
    const a = listen("toggle-listen", () => {
      setListening((v) => !v);
      setText("");
    });
    const b = listen<string>("stt-partial", (e) => setText(e.payload));
    return () => {
      a.then((f) => f());
      b.then((f) => f());
    };
  }, []);

  const mode: Mode = listening ? "listening" : media && hover ? "expanded" : media ? "media" : "idle";
  const s = SIZES[mode];
  const control = (action: string) => invoke("media_control", { action });
  const artEl = (big = false) => (art ? <img className={`art ${big ? "big" : ""}`} src={art} alt="" /> : <div className={`art ${big ? "big" : ""}`} />);

  return (
    <div className="stage">
      <motion.div
        className="island"
        animate={{ width: s.w, height: s.h, borderRadius: s.r }}
        transition={spring}
        style={{ marginTop: 8 }}
        onHoverStart={() => setHover(true)}
        onHoverEnd={() => setHover(false)}
      >
        <AnimatePresence mode="popLayout" initial={false}>
          {mode === "media" && media && (
            <motion.div key="media" className="row" {...fade}>
              {artEl()}
              <div className="col" />
              <Bars playing={media.playing} />
            </motion.div>
          )}

          {mode === "listening" && (
            <motion.div key="listening" className="row" style={{ padding: "0 20px" }} {...fade}>
              <div className="mic">{Icon.mic}</div>
              <div className="col">
                <div className="transcript">{text || <span className="hint">Listening…</span>}</div>
              </div>
              <Bars playing />
            </motion.div>
          )}

          {mode === "expanded" && media && (
            <motion.div key="expanded" className="row" style={{ flexDirection: "column", alignItems: "stretch", justifyContent: "center", padding: "0 20px", gap: 12 }} {...fade}>
              <div style={{ display: "flex", gap: 14, alignItems: "center" }}>
                {artEl(true)}
                <div className="col">
                  <div className="title">{media.title}</div>
                  <div className="artist">{media.artist}</div>
                </div>
                <Bars playing={media.playing} />
              </div>
              <div className="controls">
                <button onClick={() => control("prev")}>{Icon.prev}</button>
                <button onClick={() => control("toggle")}>{media.playing ? Icon.pause : Icon.play}</button>
                <button onClick={() => control("next")}>{Icon.next}</button>
              </div>
            </motion.div>
          )}
        </AnimatePresence>
      </motion.div>
    </div>
  );
}
