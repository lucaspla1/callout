// The in-game overlay: transparent, click-through window rendering captions in
// one of two layouts — "captions" (stacked pills) or "feed" (chat-style column).
// ⌘⇧C toggles visibility · ⌘⇧M unlocks move mode (drag, then ⌘⇧M again to lock).
import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { useCallout } from "./useCallout";
import { SpeakerIdentity, type IdentityMode } from "./SpeakerIdentity";
import "./App.css";

function Overlay() {
  const { members, finals, partial } = useCallout();
  const [moveMode, setMoveMode] = useState(false);
  const [opacity, setOpacity] = useState(0.92);
  const [fontPx, setFontPx] = useState(16);
  const [identity, setIdentity] = useState<IdentityMode>("name");
  const [layout, setLayout] = useState<"captions" | "feed">("captions");

  useEffect(() => {
    invoke<number>("get_overlay_opacity").then(setOpacity).catch(() => {});
    invoke<number>("get_caption_font").then(setFontPx).catch(() => {});
    invoke<IdentityMode>("get_caption_identity").then(setIdentity).catch(() => {});
    invoke<"captions" | "feed">("get_overlay_layout").then(setLayout).catch(() => {});
    const unMove = listen<boolean>("overlay_move_mode", (e) => setMoveMode(e.payload));
    const unOpacity = listen<number>("overlay_opacity", (e) => setOpacity(e.payload));
    const unFont = listen<number>("caption_font", (e) => setFontPx(e.payload));
    const unIdentity = listen<IdentityMode>("caption_identity", (e) => setIdentity(e.payload));
    const unLayout = listen<"captions" | "feed">("overlay_layout", (e) => setLayout(e.payload));
    return () => {
      unMove.then((f) => f());
      unOpacity.then((f) => f());
      unFont.then((f) => f());
      unIdentity.then((f) => f());
      unLayout.then((f) => f());
    };
  }, []);

  const empty = finals.length === 0 && !partial;
  const allLines = [...finals, ...(partial ? [partial] : [])];

  if (layout === "feed") {
    // Chat-style feed: avatar on the left, name above the text. New partials
    // slide in; finals fade out over their TTL (see .feed-line CSS).
    const feedLine = (l: (typeof allLines)[number], partialLine: boolean) => {
      const member =
        l.speaker_ids.length === 1 ? members.find((m) => m.id === l.speaker_ids[0]) : undefined;
      return (
        <div
          key={partialLine ? "partial" : l.t_start_ms + l.text}
          className={"feed-line" + (partialLine ? " partial" : "")}
          style={{ ["--lc" as string]: l.color }}
        >
          {member?.avatar_url ? (
            <img className="feed-avatar" src={member.avatar_url} alt="" />
          ) : (
            <span className="feed-avatar fallback">{l.speaker_label.charAt(0).toUpperCase()}</span>
          )}
          <span className="feed-meta">
            <b style={{ color: l.color }}>{l.speaker_label}</b>
            <span>
              {l.text}
              {partialLine && <i className="caret" />}
            </span>
          </span>
        </div>
      );
    };
    return (
      <div
        className={"overlay-root feed-mode" + (moveMode ? " moving" : "")}
        style={{ ["--cap-a" as string]: opacity, ["--cap-font" as string]: `${fontPx}px` }}
      >
        {moveMode && (
          <div className="drag-handle" data-tauri-drag-region>
            drag to move · ⌘⇧O or the move button to lock
          </div>
        )}
        {(!empty || moveMode) && (
          <div className="feed" role="log" aria-live="polite">
            {finals.map((l) => feedLine(l, false))}
            {partial && feedLine(partial, true)}
            {empty && moveMode && <p className="line dim">caption preview — captions appear here</p>}
          </div>
        )}
      </div>
    );
  }

  return (
    <div
      className={"overlay-root" + (moveMode ? " moving" : "")}
      style={{ ["--cap-a" as string]: opacity, ["--cap-font" as string]: `${fontPx}px` }}
    >
      {moveMode && (
        <div className="drag-handle" data-tauri-drag-region>
          drag to move · ⌘⇧O or the move button to lock
        </div>
      )}
      {(!empty || moveMode) && (
        <div className="captions" role="log" aria-live="polite">
          {finals.map((l) => (
            <p key={l.t_start_ms + l.text} className="line" style={{ ["--lc" as string]: l.color }}>
              <SpeakerIdentity line={l} members={members} mode={identity} />
              <span>{l.text}</span>
            </p>
          ))}
          {partial && (
            <p className="line partial" style={{ ["--lc" as string]: partial.color }}>
              <SpeakerIdentity line={partial} members={members} mode={identity} />
              <span>
                {partial.text}
                <i className="caret" />
              </span>
            </p>
          )}
          {empty && moveMode && <p className="line dim">caption preview — captions appear here</p>}
        </div>
      )}
    </div>
  );
}

export default Overlay;
