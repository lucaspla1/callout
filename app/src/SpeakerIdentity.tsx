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

  // Joint/unknown lines: a gray Discord-style avatar dot with the speaker
  // count (or "?") instead of a wordy label.
  if (!member) {
    const badge = line.speaker_ids.length > 1 ? String(line.speaker_ids.length) : "?";
    const title = line.speaker_ids.length > 1 ? `${line.speaker_ids.length} people speaking` : "unknown speaker";
    return (
      <span className="joint-badge" title={title} aria-label={title}>
        {badge}
      </span>
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
