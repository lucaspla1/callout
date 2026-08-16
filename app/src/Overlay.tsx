// The in-game overlay: transparent, click-through window rendering only the
// caption box and a compact who's-speaking strip.
// ⌘⇧C toggles visibility · ⌘⇧M unlocks move mode (drag, then ⌘⇧M again to lock).
import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { useCallout } from "./useCallout";
import { SpeakerIdentity, type IdentityMode } from "./SpeakerIdentity";
import "./App.css";

function Overlay() {
  const { channel, members, speaking, finals, partial } = useCallout();
  const [moveMode, setMoveMode] = useState(false);
  const [opacity, setOpacity] = useState(0.92);
  const [fontPx, setFontPx] = useState(16);
  const [identity, setIdentity] = useState<IdentityMode>("name");

  useEffect(() => {
    invoke<number>("get_overlay_opacity").then(setOpacity).catch(() => {});
    invoke<number>("get_caption_font").then(setFontPx).catch(() => {});
    invoke<IdentityMode>("get_caption_identity").then(setIdentity).catch(() => {});
    const unMove = listen<boolean>("overlay_move_mode", (e) => setMoveMode(e.payload));
    const unOpacity = listen<number>("overlay_opacity", (e) => setOpacity(e.payload));
    const unFont = listen<number>("caption_font", (e) => setFontPx(e.payload));
    const unIdentity = listen<IdentityMode>("caption_identity", (e) => setIdentity(e.payload));
    return () => {
      unMove.then((f) => f());
      unOpacity.then((f) => f());
      unFont.then((f) => f());
      unIdentity.then((f) => f());
    };
  }, []);

  const empty = finals.length === 0 && !partial;

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
      {channel && (
        <div className="overlay-chips">
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
