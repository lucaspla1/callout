// Shared subscription to the backend's presence/caption/status event streams.
// Used by both the dev window (App) and the in-game overlay (Overlay).
import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";

export type Member = {
  id: string;
  display_name: string;
  color: string;
  avatar_url: string | null;
  muted: boolean;
};

export type PresenceEvent =
  | { type: "channel_joined"; channel_name: string; members: Member[] }
  | { type: "channel_left" }
  | { type: "member_joined"; member: Member }
  | { type: "member_updated"; member: Member }
  | { type: "member_left"; user_id: string }
  | { type: "speaking_start"; user_id: string; at_ms: number }
  | { type: "speaking_stop"; user_id: string; at_ms: number };

export type Status =
  | { state: "waiting_for_discord" }
  | { state: "connecting" }
  | { state: "awaiting_approval" }
  | { state: "ready"; username: string }
  | { state: "auth_error"; message: string }
  | { state: "disconnected" }
  | { state: "mock" };

export type CaptionsStatus =
  | { state: "downloading_models" }
  | { state: "loading_model" }
  | { state: "stt_ready" }
  | { state: "waiting_for_discord_audio" }
  | { state: "capturing"; native_rate: number }
  | { state: "capture_error"; message: string }
  | { state: "stt_error"; message: string };

export type ModelDl =
  | { state: "progress"; id: string; got: number; total: number }
  | { state: "done"; id: string }
  | { state: "failed"; id: string; message: string }
  | { state: "all_ready" };

export type CaptionLine = {
  speaker_ids: string[];
  speaker_label: string;
  color: string;
  text: string;
  is_final: boolean;
  t_start_ms: number;
};

const MAX_FINAL_LINES = 3;
/// Finals disappear after this long — captions are for the moment, not history.
const LINE_TTL_MS = 7000;

type TimedLine = CaptionLine & { shown_at: number };

export function useCallout() {
  const [status, setStatus] = useState<Status | null>(null);
  const [captions, setCaptions] = useState<CaptionsStatus | null>(null);
  const [channel, setChannel] = useState<string | null>(null);
  const [members, setMembers] = useState<Member[]>([]);
  const [speaking, setSpeaking] = useState<string[]>([]);
  const [finals, setFinals] = useState<TimedLine[]>([]);
  const [partial, setPartial] = useState<CaptionLine | null>(null);

  // Expire old finals once a second.
  useEffect(() => {
    const timer = setInterval(() => {
      const cutoff = Date.now() - LINE_TTL_MS;
      setFinals((prev) => (prev.some((l) => l.shown_at < cutoff) ? prev.filter((l) => l.shown_at >= cutoff) : prev));
    }, 1000);
    return () => clearInterval(timer);
  }, []);

  useEffect(() => {
    const unStatus = listen<Status>("status", (e) => {
      setStatus(e.payload);
      // Discord gone → captions are stale context; drop them immediately.
      if (e.payload.state === "disconnected" || e.payload.state === "waiting_for_discord") {
        setFinals([]);
        setPartial(null);
      }
    });
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
          setFinals([]);
          setPartial(null);
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
        setFinals((prev) => [...prev.slice(-(MAX_FINAL_LINES - 1)), { ...line, shown_at: Date.now() }]);
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

  return { status, captions, channel, members, speaking, finals, partial };
}
