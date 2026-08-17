# UNMUTE brand and launch direction

_Working direction, 2026-08-17. Validate with deaf and hard-of-hearing gamers before treating this as a final identity or launch plan._

## Positioning

UNMUTE is not a generic transcription utility. Its distinctive promise is voice-chat participation while gaming: real-time captions over the game, labeled by speaker, processed locally.

Primary audience:

- deaf and hard-of-hearing PC gamers who use Discord voice chat;

Adjacent audiences:

- people with auditory-processing differences;
- non-native speakers;
- players who cannot or prefer not to use game audio.

Translation, meetings, and “caption any app” are later expansion paths. Do not dilute the v0.1 narrative with them.

Recommended message order:

1. participate in fast voice chat while playing;
2. know who said each line;
3. keep audio and transcription on the device;
4. avoid bots and client modification;
5. show measured game/CPU impact and limitations.

## Working copy

**Tagline:** Every voice, on screen.

**Descriptor:** Local, real-time captions for Discord voice chat — labeled by speaker and overlaid on your game.

**Positioning statement:** For deaf and hard-of-hearing gamers who use Discord, UNMUTE turns voice chat into local captions with each speaker's name over the game, without a bot, client modification, or cloud audio processing.

Avoid absolute claims such as “ToS-safe,” “nothing for anti-cheat to dislike,” “perfect,” “zero impact,” or “private by definition.” State observable architecture and measured behavior instead.

## Name

Keep **UNMUTE** as the working title during development, not as a settled trademark. It is memorable and emotionally aligned with the mission, but it is also a generic voice verb, can imply turning on the user's microphone, and has neighboring voice/accessibility products using similar names.

The US trademark register shows an active **UNMUTE** registration (serial 98290215, registration 8,254,640) for adjacent audio/music software services. That is not a legal conclusion about infringement, but it makes the name a launch blocker until a qualified trademark search clears the intended markets or the product is renamed. Do not buy domains, commission final identity work, or submit app-store listings under this name yet.

Before public beta:

- run professional trademark screening in intended launch jurisdictions;
- check domain, app-store, package, repository, and social-handle availability;
- test name comprehension with target users;
- decide whether the product name needs a more distinctive modifier without placing “Discord” in the brand or lockup.

## Visual direction

The overlay's readability is the brand in use. Keep it quieter than the game:

- preserve speaker name/avatar plus chronological caption text;
- use color as a redundant speaker cue, never the only identity cue;
- keep stable, tested contrast across bright and dark gameplay;
- prioritize adjustable font size, opacity, position, and persistence;
- avoid decorative logo treatment inside the caption overlay.

The current `mockups/branding/` work is exploratory. Its exact Discord Blurple and Discord-like multicolor palette are too close to Discord's brand system. Build a distinct UNMUTE accent and an accessible speaker palette before launch. Discord's name should remain descriptive, with a clear non-affiliation statement.

The current mark direction (“Chorus,” multiple voice bars) tells the speaker-attribution story better than a generic chat bubble, but waveform/bar marks are common. Do not invest heavily in final production assets until name screening and palette work are complete.

## Overlay decision

Use layout variant 6 from `mockups/overlay-variants.html` as the current default direction:

- stacked caption pills preserve chronology;
- name/avatar preserve speaker identity;
- removing the permanent participant chip row reduces redundant UI and screen occlusion.

The roster-attached speech-bubble concept is not a default candidate because overlapping speech becomes spatial rather than chronological. A possible future experiment is short-lived join/leave/mute notifications that disappear after a few seconds; this is not yet an approved requirement.

## Third-party assets

`mockups/fortnite.png` is a design-context image, not an approved launch asset. Epic's fan-content policy limits the permission it describes to personal, noncommercial fan content, requires a disclaimer, and reserves revocation rights. Do not use this file in a README hero, website, social preview, advertisement, store listing, press kit, or paid/commercial campaign.

Before launch, replace it with one of:

- gameplay recorded by the maintainer with confirmed reuse rights and no third-party personal data;
- an explicitly licensed/open game scene compatible with the launch use;
- an original game-like scene or product demo created for UNMUTE.

Never imply affiliation, endorsement, or approval by Discord, Epic Games, Fortnite, OpenAI, or model publishers.

## Launch sequence

### 1. Preparation gates

- decide the license and accurate “open source” versus “source-available” language;
- screen the name and establish a distinct palette;
- replace third-party gameplay imagery in public materials;
- restore green CI and complete a reproducible Windows CPU/latency benchmark;
- complete Discord approval, signing, privacy, and legal review.

### 2. Private alpha

- recruit 10–20 testers, prioritizing deaf and hard-of-hearing gamers and mixed English/pt-BR speech;
- test installation, first-caption success, real-game CPU impact, latency, attribution, crosstalk, chronology, overlay occlusion, and recovery;
- use layout variant 6 as the default and test event-only participant notifications separately.

### 3. Closed beta

- ship signed builds and reliable onboarding;
- publish a simple landing page and a 20–30 second demo using owned/licensed media;
- recruit through gaming-accessibility communities and partners before general tech channels.

### 4. v0.1 and first 30 days

- launch only when Discord distribution, Windows QA, privacy, and performance gates are satisfied;
- make accessibility communities and specialist press the primary channels; use developer/launch sites secondarily;
- optimize for successful first captions, stability, caption usefulness, and retained use rather than download count;
- publish measured benchmarks and consented user feedback, then prioritize fixes before expanding the product story.

## Current policy references

- Discord Brand Guidelines: <https://discord.com/branding>
- Epic Games Fan Content Policy (pt-BR): <https://legal.epicgames.com/epicgames/fan-art-policy?lang=pt-BR>
- USPTO TSDR, UNMUTE serial 98290215: <https://tsdr.uspto.gov/statusview/sn98290215>

Re-check both before using third-party names, colors, logos, screenshots, or trade dress in a release.
