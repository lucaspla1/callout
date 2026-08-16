// The main window is just settings + status. Captions live in the overlay.
import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useCallout, type CaptionsStatus, type ModelDl, type Status } from "./useCallout";
import { type IdentityMode } from "./SpeakerIdentity";
import "./App.css";

const LANGUAGES = [
  { code: "en", label: "English" },
  { code: "pt", label: "Português" },
  { code: "es", label: "Español" },
];

const STATUS_TEXT: Record<Status["state"], string> = {
  waiting_for_discord: "Waiting for Discord — open the desktop app",
  connecting: "Connecting to Discord…",
  awaiting_approval: "Approve the authorization prompt inside Discord",
  ready: "Connected",
  auth_error: "Authorization failed",
  disconnected: "Disconnected — retrying…",
  mock: "Mock mode (CALLOUT_MOCK=1)",
};

const CAPTIONS_TEXT: Record<CaptionsStatus["state"], string> = {
  downloading_models: "Downloading speech models…",
  loading_model: "Loading speech model…",
  stt_ready: "Speech model ready",
  waiting_for_discord_audio: "Waiting for Discord audio…",
  capturing: "Listening to Discord",
  capture_error: "Capture error",
  stt_error: "Speech engine error",
};

function App() {
  const { status, captions, channel, members } = useCallout();
  const [languages, setLanguages] = useState<string[]>([]);
  const [opacity, setOpacity] = useState(0.92);
  const [dl, setDl] = useState<{ id: string; pct: number } | null>(null);
  const [fontPx, setFontPx] = useState(16);
  const [identity, setIdentity] = useState<IdentityMode>("name");
  const [layout, setLayout] = useState<"captions" | "feed">("captions");

  useEffect(() => {
    invoke<string[]>("get_languages").then(setLanguages).catch(() => {});
    invoke<number>("get_overlay_opacity").then(setOpacity).catch(() => {});
    invoke<number>("get_caption_font").then(setFontPx).catch(() => {});
    invoke<IdentityMode>("get_caption_identity").then(setIdentity).catch(() => {});
    invoke<"captions" | "feed">("get_overlay_layout").then(setLayout).catch(() => {});
    const unDl = listen<ModelDl>("model_dl", (e) => {
      const ev = e.payload;
      if (ev.state === "progress") {
        setDl({ id: ev.id, pct: Math.min(100, Math.round((100 * ev.got) / Math.max(ev.total, 1))) });
      } else if (ev.state === "all_ready" || ev.state === "failed") {
        setDl(null);
      }
    });
    return () => {
      unDl.then((f) => f());
    };
  }, []);

  const setLayoutAnd = (next: "captions" | "feed") => {
    setLayout(next);
    invoke("set_overlay_layout", { layout: next }).catch(() => {});
  };
  const setIdentityAnd = (next: IdentityMode) => {
    setIdentity(next);
    invoke("set_caption_identity", { mode: next }).catch(() => {});
  };
  const changeOpacity = (value: number) => {
    setOpacity(value);
    invoke("set_overlay_opacity", { opacity: value }).catch(() => {});
  };
  const changeFont = (value: number) => {
    setFontPx(value);
    invoke("set_caption_font", { px: value }).catch(() => {});
  };
  const toggleLanguage = (code: string) => {
    setLanguages((prev) => {
      const next = prev.includes(code) ? prev.filter((c) => c !== code) : [...prev, code];
      invoke("set_languages", { languages: next }).catch(() => {});
      return next;
    });
  };

  const statusText = status
    ? status.state === "ready"
      ? `Connected as ${status.username}`
      : status.state === "auth_error"
        ? `Authorization failed: ${status.message}`
        : STATUS_TEXT[status.state]
    : "Starting…";
  const capturing = captions?.state === "capturing";

  return (
    <main className="settings">
      <header className="settings-head">
        <span className="brand">Unmute</span>
        <div className="status-lines">
          <span className={"status-line" + (status?.state === "ready" ? " ok" : "")}>
            <i className="status-dot" />
            {statusText}
          </span>
          {captions && (
            <span
              className={"status-line" + (capturing ? " ok" : "")}
              title={"message" in captions ? captions.message : undefined}
            >
              <i className="status-dot" />
              {CAPTIONS_TEXT[captions.state]}
            </span>
          )}
          <span className={"status-line" + (channel ? " ok" : "")}>
            <i className="status-dot" />
            {channel ? `${channel} · ${members.length} in call` : "Not in a voice channel"}
          </span>
        </div>
      </header>

      {status?.state === "awaiting_approval" && (
        <div className="hint">
          Discord is showing an authorization prompt — switch to Discord and click <b>Authorize</b>.
        </div>
      )}
      {dl && (
        <div className="hint">
          ⬇︎ Downloading {dl.id} — {dl.pct}%
        </div>
      )}

      <section className="group">
        <h2>Overlay</h2>
        <div className="row">
          <span className="row-label">Layout</span>
          <span className="segmented">
            <button className={layout === "captions" ? "on" : ""} onClick={() => setLayoutAnd("captions")}>
              ▬ Pills
            </button>
            <button className={layout === "feed" ? "on" : ""} onClick={() => setLayoutAnd("feed")}>
              ☰ Feed
            </button>
          </span>
        </div>
        <div className="row">
          <span className="row-label">Speaker label</span>
          <span className="segmented">
            <button className={identity === "name" ? "on" : ""} onClick={() => setIdentityAnd("name")}>
              Name
            </button>
            <button className={identity === "avatar" ? "on" : ""} onClick={() => setIdentityAnd("avatar")}>
              Avatar
            </button>
            <button className={identity === "both" ? "on" : ""} onClick={() => setIdentityAnd("both")}>
              Both
            </button>
          </span>
        </div>
        <div className="row">
          <span className="row-label">Background</span>
          <input
            type="range"
            min={0.2}
            max={1}
            step={0.05}
            value={opacity}
            onChange={(e) => changeOpacity(Number(e.target.value))}
          />
          <span className="row-value">{Math.round(opacity * 100)}%</span>
        </div>
        <div className="row">
          <span className="row-label">Text size</span>
          <input
            type="range"
            min={12}
            max={26}
            step={1}
            value={fontPx}
            onChange={(e) => changeFont(Number(e.target.value))}
          />
          <span className="row-value">{fontPx}px</span>
        </div>
        <div className="row">
          <span className="row-label">Position</span>
          <button className="action" onClick={() => invoke("toggle_move_overlay").catch(() => {})}>
            ✥ Unlock &amp; drag
          </button>
        </div>
      </section>

      <section className="group">
        <h2>Speech</h2>
        <div className="row">
          <span className="row-label">Languages</span>
          <span className="segmented">
            {LANGUAGES.map((l) => (
              <button
                key={l.code}
                className={languages.includes(l.code) ? "on" : ""}
                onClick={() => toggleLanguage(l.code)}
              >
                {l.label}
              </button>
            ))}
          </span>
        </div>
        <p className="row-hint">
          {languages.length === 0
            ? "None selected — every language is auto-detected."
            : "Detection is restricted to the selected languages."}
        </p>
      </section>

      <footer className="shortcuts">
        <span>
          <b>⌘⇧C</b> show / hide overlay
        </span>
        <span>
          <b>⌘⇧O</b> unlock / lock position
        </span>
      </footer>
    </main>
  );
}

export default App;
