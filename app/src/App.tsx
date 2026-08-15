import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useCallout, type CaptionsStatus, type Status } from "./useCallout";
import "./App.css";

const LANGUAGES = [
  { code: "en", label: "EN" },
  { code: "pt", label: "PT" },
  { code: "es", label: "ES" },
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
  model_missing: "speech model missing",
  loading_model: "loading speech model…",
  stt_ready: "speech model ready",
  waiting_for_discord_audio: "waiting for Discord audio…",
  capturing: "listening to Discord",
  capture_error: "capture error",
  stt_error: "speech engine error",
};

function App() {
  const { status, captions, channel, members, speaking, finals, partial } = useCallout();
  const [languages, setLanguages] = useState<string[]>([]);
  const [opacity, setOpacity] = useState(0.92);

  useEffect(() => {
    invoke<string[]>("get_languages").then(setLanguages).catch(() => {});
    invoke<number>("get_overlay_opacity").then(setOpacity).catch(() => {});
  }, []);

  const changeOpacity = (value: number) => {
    setOpacity(value);
    invoke("set_overlay_opacity", { opacity: value }).catch(() => {});
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

  return (
    <main className="shell">
      <header className="bar">
        <span className="brand">Callout</span>
        <span className={"stage" + (status?.state === "ready" ? " ok" : "")}>{statusText}</span>
        {captions && (
          <span
            className={"stage" + (captions.state === "capturing" ? " ok" : "")}
            title={"message" in captions ? captions.message : "path" in captions ? captions.path : undefined}
          >
            🎙 {CAPTIONS_TEXT[captions.state]}
          </span>
        )}
        <span className="stage" title="Global hotkey: toggle the overlay">⌘⇧C overlay</span>
        <button
          className="lang"
          title="Unlock the overlay to drag it (also ⌘⇧O); click again to lock"
          onClick={() => invoke("toggle_move_overlay").catch(() => {})}
        >
          ✥ move
        </button>
        <span className="opacity-slider" title="Overlay background opacity">
          <input
            type="range"
            min={0.2}
            max={1}
            step={0.05}
            value={opacity}
            onChange={(e) => changeOpacity(Number(e.target.value))}
          />
        </span>
        {channel ? (
          <span className="channel">
            🔊 {channel}
            <span className="chips">
              {[...members]
                .sort((a, b) => a.display_name.localeCompare(b.display_name))
                .map((m) => (
                  <span
                    key={m.id}
                    className={"chip" + (speaking.includes(m.id) ? " on" : "") + (m.muted ? " muted" : "")}
                    style={{ ["--c" as string]: m.color }}
                  >
                    {m.display_name}
                  </span>
                ))}
            </span>
          </span>
        ) : (
          <span className="channel dim">not in a voice channel</span>
        )}
        <span className="langs" title="Caption languages — none checked = detect any; several = detect within the set">
          {LANGUAGES.map((l) => (
            <button
              key={l.code}
              className={"lang" + (languages.includes(l.code) ? " on" : "")}
              onClick={() => toggleLanguage(l.code)}
            >
              {l.label}
            </button>
          ))}
          {languages.length === 0 && <span className="lang-hint">auto</span>}
        </span>
      </header>

      {status?.state === "awaiting_approval" && (
        <div className="hint">
          Discord is showing an authorization prompt — switch to Discord and click <b>Authorize</b>.
        </div>
      )}

      <section className="game" aria-label="Pretend this is your game">
        <div className="captions" role="log" aria-live="polite">
          {finals.map((l) => (
            <p key={l.t_start_ms + l.text} className="line" style={{ ["--lc" as string]: l.color }}>
              <b style={{ color: l.color }}>{l.speaker_label}</b>
              <span>{l.text}</span>
            </p>
          ))}
          {partial && (
            <p className="line partial" style={{ ["--lc" as string]: partial.color }}>
              <b style={{ color: partial.color }}>{partial.speaker_label}</b>
              <span>
                {partial.text}
                <i className="caret" />
              </span>
            </p>
          )}
          {finals.length === 0 && !partial && (
            <p className="line dim">waiting for someone to talk…</p>
          )}
        </div>
      </section>
    </main>
  );
}

export default App;
