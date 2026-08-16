// How a caption line identifies its speaker: name, avatar, or both.
// Joint ("N speaking") and unknown ("?") lines always render as text.
import type { CaptionLine, Member } from "./useCallout";

export type IdentityMode = "name" | "avatar" | "both";

export function SpeakerIdentity({
  line,
  members,
  mode,
}: {
  line: CaptionLine;
  members: Member[];
  mode: IdentityMode;
}) {
  const member =
    line.speaker_ids.length === 1 ? members.find((m) => m.id === line.speaker_ids[0]) : undefined;

  // Joint/unknown lines: a gray default-avatar silhouette with the speaker
  // count as a corner badge (Discord's visual language).
  if (!member) {
    const count = line.speaker_ids.length > 1 ? String(line.speaker_ids.length) : "?";
    const title =
      line.speaker_ids.length > 1 ? `${line.speaker_ids.length} people speaking` : "unknown speaker";
    return (
      <i className="joint-badge" title={title} aria-label={title}>
        <svg viewBox="0 0 24 24" aria-hidden="true">
          <circle cx="12" cy="8.2" r="4.2" />
          <path d="M3.5 20.5c0-4.2 4-6.8 8.5-6.8s8.5 2.6 8.5 6.8v1h-17z" />
        </svg>
        <em className="joint-count">{count}</em>
      </i>
    );
  }

  const avatar = (mode === "avatar" || mode === "both") && member.avatar_url;
  const name = mode !== "avatar";
  return (
    <>
      {avatar && <img className="cap-avatar" src={member.avatar_url!} alt={member.display_name} />}
      {name && <b style={{ color: line.color }}>{`${line.speaker_label}:`}</b>}
    </>
  );
}
