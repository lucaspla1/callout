import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import "./App.css";

const LANGUAGES = [
  { code: "en", label: "EN" },
  { code: "pt", label: "PT" },
  { code: "es", label: "ES" },
];

type Member = {
  id: string;
  display_name: string;
  color: string;
  avatar_url: string | null;
  muted: boolean;
};

type PresenceEvent =
  | { type: "channel_joined"; channel_name: string; members: Member[] }
  | { type: "channel_left" }
  | { type: "member_joined"; member: Member }
  | { type: "member_updated"; member: Member }
  | { type: "member_left"; user_id: string }
  | { type: "speaking_start"; user_id: string; at_ms: number }
  | { type: "speaking_stop"; user_id: string; at_ms: number };

type Status =
  | { state: "waiting_for_discord" }
  | { state: "connecting" }
  | { state: "awaiting_approval" }
  | { state: "ready"; username: string }
  | { state: "auth_error"; message: string }
  | { state: "disconnected" }
  | { state: "mock" };

type CaptionLine = {
  speaker_ids: string[];
  speaker_label: string;
  color: string;
  text: string;
  is_final: boolean;
  t_start_ms: number;
};

type CaptionsStatus =
  | { state: "model_missing"; path: string }
  | { state: "loading_model" }
  | { state: "stt_ready" }
  | { state: "waiting_for_discord_audio" }
  | { state: "capturing"; native_rate: number }
  | { state: "capture_error"; message: string }
  | { state: "stt_error"; message: string };

const CAPTIONS_TEXT: Record<CaptionsStatus["state"], string> = {
  model_missing: "speech model missing",
  loading_model: "loading speech model…",
  stt_ready: "speech model ready",
  waiting_for_discord_audio: "waiting for Discord audio…",
  capturing: "listening to Discord",
  capture_error: "capture error",
  stt_error: "speech engine error",
};

const MAX_FINAL_LINES = 3;

const STATUS_TEXT: Record<Status["state"], string> = {
  waiting_for_discord: "Waiting for Discord — open the desktop app",
  connecting: "Connecting to Discord…",
  awaiting_approval: "Approve the authorization prompt inside Discord",
  ready: "Connected",
  auth_error: "Authorization failed",
  disconnected: "Disconnected — retrying…",
  mock: "Mock mode (CALLOUT_MOCK=1)",
};

function App() {
  const [status, setStatus] = useState<Status | null>(null);
  const [captions, setCaptions] = useState<CaptionsStatus | null>(null);
  const [languages, setLanguages] = useState<string[]>([]);

  useEffect(() => {
    invoke<string[]>("get_languages").then(setLanguages).catch(() => {});
  }, []);

  const toggleLanguage = (code: string) => {
    setLanguages((prev) => {
      const next = prev.includes(code) ? prev.filter((c) => c !== code) : [...prev, code];
      invoke("set_languages", { languages: next }).catch(() => {});
      return next;
    });
  };
  const [channel, setChannel] = useState<string | null>(null);
  const [members, setMembers] = useState<Member[]>([]);
  const [speaking, setSpeaking] = useState<string[]>([]);
  const [finals, setFinals] = useState<CaptionLine[]>([]);
  const [partial, setPartial] = useState<CaptionLine | null>(null);

  useEffect(() => {
    const unStatus = listen<Status>("status", (e) => setStatus(e.payload));
    const unCaptionsStatus = listen<CaptionsStatus>("captions_status", (e) => setCaptions(e.payload));
    const unPresence = listen<PresenceEvent>("presence", (e) => {
      const ev = e.payload;
      switch (ev.type) {
        case "channel_joined":
          setChannel(ev.channel_name);
          setMembers(ev.members);
          setSpeaking([]);
          break;
        case "channel_left":
          setChannel(null);
          setMembers([]);
          setSpeaking([]);
          break;
        case "member_joined":
        case "member_updated":
          setMembers((prev) => {
            const rest = prev.filter((m) => m.id !== ev.member.id);
            return [...rest, ev.member];
          });
          break;
        case "member_left":
          setMembers((prev) => prev.filter((m) => m.id !== ev.user_id));
          setSpeaking((s) => s.filter((id) => id !== ev.user_id));
          break;
        case "speaking_start":
          setSpeaking((s) => (s.includes(ev.user_id) ? s : [...s, ev.user_id]));
          break;
        case "speaking_stop":
          setSpeaking((s) => s.filter((id) => id !== ev.user_id));
          break;
      }
    });
    const unCaption = listen<CaptionLine>("caption", (e) => {
      const line = e.payload;
      if (line.is_final) {
        setFinals((prev) => [...prev.slice(-(MAX_FINAL_LINES - 1)), line]);
        setPartial(null);
      } else {
        setPartial(line);
      }
    });
    return () => {
      unStatus.then((f) => f());
      unCaptionsStatus.then((f) => f());
      unPresence.then((f) => f());
      unCaption.then((f) => f());
    };
  }, []);

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
      </header>

      {status?.state === "awaiting_approval" && (
        <div className="hint">
          Discord is showing an authorization prompt — switch to Discord and click <b>Authorize</b>.
        </div>
      )}

      <section className="game" aria-label="Pretend this is your game">
        <div className="captions" role="log" aria-live="polite">
          {finals.map((l) => (
            <p key={l.t_start_ms + l.text} className="line">
              <b style={{ color: l.color }}>{l.speaker_label}</b>
              <span>{l.text}</span>
            </p>
          ))}
          {partial && (
            <p className="line partial">
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
