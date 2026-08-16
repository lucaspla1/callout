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
  const avatar = (mode === "avatar" || mode === "both") && member?.avatar_url;
  const name = mode !== "avatar" || !member;
  return (
    <>
      {avatar && <img className="cap-avatar" src={member.avatar_url!} alt={member.display_name} />}
      {name && <b style={{ color: line.color }}>{line.speaker_label}</b>}
    </>
  );
}
