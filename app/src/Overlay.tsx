// The in-game overlay: transparent, click-through window rendering only the
// caption box and a compact who's-speaking strip. Toggled with Cmd/Ctrl+Shift+C.
import { useCallout } from "./useCallout";
import "./App.css";

function Overlay() {
  const { channel, members, speaking, finals, partial } = useCallout();
  const empty = finals.length === 0 && !partial;

  return (
    <div className="overlay-root">
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
      {!empty && (
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
        </div>
      )}
    </div>
  );
}

export default Overlay;
