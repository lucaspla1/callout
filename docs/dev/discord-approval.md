# Getting Unmute past Discord's RPC gate

The application dossier for petitioning Discord to lift the tester restriction on Unmute's
`rpc` scope, plus the honest state of the world and the plan that doesn't depend on winning.

Facts last verified **2026-08-16**. App: **Unmute**, application ID `1538241556560085065`,
scopes `rpc` + `identify`, no bot, owned by Lucas (individual developer, Brazil).

---

## 1. The situation, precisely

- Official RPC docs: *"We currently do not allow access to RPC for unapproved apps without
  being on the game's list of testers."* You get *"50 testing spots"*; *"After approval,
  this restriction is removed and your app will be accessible to anyone."* The historical
  preamble was blunter: *"RPC is in a private beta, which means that only apps that have
  signed up and been approved can access it"* — and the signup form it once pointed to has
  been closed for years.
- Userdoccers (tracks the live client) states the enforcement rule: unless an application is
  approved for general RPC access, **the `rpc` scope works for the application owner and
  whitelisted (tester) users only**. Approval = Discord manually setting the
  `RPC_PRIVATE_BETA` application flag ("can use the rpc scope without limitation" — not
  publicly grantable, no portal toggle).
- **There is no application form in 2026.** No portal button, no request queue. The current
  docs instead steer new projects to the **Social SDK** — which does not cover our use case
  (it embeds Discord features into a game; it cannot read the local client's current voice
  channel or speaking events, which is the entire product).
- Known approved apps (community-tracked): **Discord StreamKit** (Discord's own),
  **Overlayed**, **Reactive Images**, **Elgato Stream Deck**. Four apps, barely grown in
  years. Overlayed — an overlay app by an individual/small team, carrying the full `rpc.*`
  scope family — is the proof it's attainable for exactly our category, via direct contact
  with Discord rather than any form.

### Which "verification" track applies? (spoiler: none of them)

| Track | Trigger | Applies to Unmute? |
|---|---|---|
| **Bot/app verification** (portal "App Verification" tab, checklist: privacy policy + ToS URLs, identity verification, policy compliance) | Bot approaching **100 servers** (email invite at 75) | **No.** Unmute has no bot and is never installed into servers; the counter that triggers this track never moves. Completing it also would *not* clear the RPC gate — that's a separate manual flag. |
| **App Directory / Activities verification** | Discoverable installable apps and Activities | **No.** Nothing installable, no Activity. |
| **RPC general access** (`RPC_PRIVATE_BETA` flag) | Manual grant by Discord | **Yes — this is the one.** No self-serve path exists; it takes a human at Discord saying yes. |

Practical consequence: the portal checklist items (privacy policy URL, ToS URL, clean
description) are still worth completing — they're what any human reviewer will look at
first — but the *ask* itself has to travel through support channels or a warm contact.

---

## 2. Where to actually apply

Ranked by expected value. Do all of them; they don't conflict.

1. **Developer support ticket** — <https://support-dev.discord.com/hc/en-us/requests/new>
   (also reachable via <https://dis.gd/contact>). Category: Developer Support / "My question
   is about something else". Paste the dossier from §3. This is the only formal intake that
   exists.
2. **Discord Developers server** — <https://discord.gg/discord-developers>. Ask in the API
   help channels whether RPC general-access requests are being taken and what the current
   route is. Staff and mods do answer here; even a "not right now" is signal, and a thread
   you can point to later.
3. **Discord's accessibility team, in parallel** — Discord publishes an accessibility
   statement (<https://discord.com/accessibility-statement>) and an **Accessibility Feedback
   Form** (linked from it). File the *user-side gap* there: "party-chat captions exist on
   Xbox/PS5/Switch 2; Discord has open accessibility requests for captions since 2020; an
   open-source app exists that closes the gap but is capped at 50 users by the RPC gate."
   The a11y team can't flip the flag, but an internal nudge from them lands very differently
   than a cold ticket.
4. **Warm paths.** The Overlayed maintainers (github.com/overlayeddev/overlayed) walked this
   exact road and are the only public precedent; asking them "who did you talk to" is free.
   Any Discord employee contact (conference, mutuals, DDevs server) beats all of the above.

**Does accessibility framing help?** There's no documented fast lane, but it is the
strongest card we hold: it reframes the request from "let my app scale" to "50 deaf users
per app-ID is the current ceiling on captioning Discord voice." Lead with it everywhere.

---

## 3. The dossier (copy-paste)

> **Subject: Request for general RPC access (rpc scope) — open-source accessibility app for deaf/HoH users**
>
> Hi — I'm requesting that my application be approved for general RPC access (the
> restriction described in the RPC docs where unapproved apps can only be authorized by the
> owner and up to 50 testers).
>
> **Application:** Unmute — application ID `1538241556560085065`
> **Developer:** Lucas Pitchon (individual developer, Brazil) — account email on the application
> **Source code (MIT):** https://github.com/lucaspla1/unmute
> **Privacy policy:** https://github.com/lucaspla1/unmute/blob/main/PRIVACY.md
> **Demo video:** [LINK — record 60–90s: join VC, captions appear with names, hotkeys]
>
> **What it is.** Unmute provides live captions for Discord voice channels for deaf and
> hard-of-hearing users, as an on-screen overlay with each caption labeled by speaker.
> Xbox, PlayStation and Nintendo all ship party-chat captions; Discord users have been
> requesting them since 2020. Unmute closes that gap today, without modifying Discord.
>
> **How it uses Discord.** It connects to the local Discord desktop client over RPC — the
> same local surface StreamKit uses — solely to learn *who is in the user's current voice
> channel and who is speaking right now* (GET_SELECTED_VOICE_CHANNEL, SPEAKING_START/STOP,
> VOICE_STATE_* events). Speech-to-text runs entirely on the user's machine against
> Discord's local audio output; transcripts are displayed and discarded.
>
> **Scopes: `rpc` and `identify` only.** No bot, no guild installs, no message content, no
> `rpc.*` write scopes, no server data of any kind. OAuth is PKCE public-client — the app
> ships no client secret. Audio and transcripts never leave the user's machine; the only
> network traffic is Discord's own OAuth token exchange and a one-time model download.
>
> **Why RPC and not the Social SDK.** The Social SDK embeds Discord features into a game;
> it cannot observe the local client's current voice channel, which is the entire function
> of an accessibility overlay. RPC is the only mechanism for this use case — as it is for
> StreamKit and Overlayed (an approved app in the same category).
>
> **The problem.** The 50-tester allowlist is, in practice, a cap of 50 deaf/HoH users. I'd
> like the restriction lifted so the app can be public. I'm happy to meet any requirements:
> the code is MIT-licensed and auditable, I'll complete identity verification, add Terms of
> Service, restrict scopes further if asked, and make any changes your team needs.
>
> Thank you for your time — and for StreamKit having proven this local surface can be done
> safely for a decade.

Keep the ticket short; the repo README and PRIVACY.md carry the depth. If a form asks for
data-use answers, crib from §4.

---

## 4. Data-use answers (for any form or follow-up)

| Question | Answer |
|---|---|
| What user data do you access? | Via `identify`: username/avatar of the authorizing user (shown in-app only). Via `rpc`: the local client's current voice channel roster and speaking start/stop events. |
| What do you store, and where? | OAuth tokens and app settings, locally on the user's device. Nothing server-side — we run no servers. |
| What leaves the user's machine? | Nothing we generate. Only Discord's own OAuth token exchange (discord.com) and one-time STT model downloads (Hugging Face/GitHub). |
| Audio handling? | OS-level capture of Discord's process audio only (never the game/mic mix), transcribed locally on CPU/iGPU, then discarded. No recordings, no cloud STT. |
| Message content / guild data / bot? | None. No bot user exists on the application. |
| Data retention / deletion? | No transcripts or audio are retained anywhere. Uninstalling removes tokens and settings. |
| Monetization? | None. MIT open source. |
| Who can access the data? | Only the user, on their own device. Developers never see user data. |

---

## 5. Portal checklist before sending anything

All in <https://discord.com/developers/applications> → Unmute. A reviewer *will* open this
page; make it look finished.

- [ ] **General Information:** clear description (first line of §3's "What it is" works), icon,
      tags. **Privacy Policy URL** → `https://github.com/lucaspla1/unmute/blob/main/PRIVACY.md`.
      **Terms of Service URL** → add one (a short `TERMS.md` in the repo; create before applying —
      verification checklists elsewhere require both URLs, reviewers expect them).
- [ ] **OAuth2:** Public Client **on**; redirect `http://127.0.0.1` present; nothing else enabled.
- [ ] **No bot** added to the application (keep it that way — it's part of the pitch).
- [ ] **App Testers:** list actively maintained (shows real usage).
- [ ] Account hygiene: 2FA on the owner account, verified email. Consider moving the app into a
      **Team** later for continuity, but don't block on it.
- [ ] Record the **demo video** and put the link in the dossier.

---

## 6. Honest odds and timeline

- **Odds: low on any single attempt; nonzero over time.** The approved list is four apps and
  Discord's docs actively point newcomers elsewhere. The realistic failure mode isn't "no" —
  it's silence or a canned "RPC isn't accepting new apps."
- **Timeline if it works: months.** Overlayed-style approval came through human contact, not
  process. Budget a quarter minimum; don't put it on the roadmap's critical path.
- **What improves the odds over time:** a full 50/50 tester list, a public waitlist number,
  BYO-client-id adoption (proof of demand Discord's gate is suppressing), press/community
  attention on the accessibility angle, and a spotless portal page. Re-approach quarterly
  with the updated numbers rather than spamming.

## 7. The plan that doesn't need permission (fallback ladder)

1. **Now — 50 testers.** Portal → App Testers → invite by Discord username; tester accepts
   the emailed invite. Covers the first testing wave. Recycle slots of inactive testers.
2. **At the cap — BYO client ID.** Any user can create their own Discord application
   (2 minutes, free, no bot), flip on Public Client, and run Unmute as its owner —
   Discord's documented rule ("owner and whitelisted users") makes this work for everyone,
   forever, with zero Discord approval. Already shipped via `CALLOUT_CLIENT_ID`
   (tester-facing steps in [docs/TESTING.md](../TESTING.md)); the M4 settings UI should make
   it a paste-one-field affair. This is the real public-release mechanism until approval.
3. **In parallel — the §2 campaign.** Ticket + DDevs server + a11y form, refreshed
   quarterly with growth numbers. If Discord ever flips `RPC_PRIVATE_BETA`, the default
   client ID starts working for everyone and steps 1–2 become legacy.

---

## Sources

- Official RPC topic (tester gate, 50 spots, post-approval wording): <https://docs.discord.com/developers/topics/rpc>
- Historical "private beta / signed up and been approved" preamble: <https://github.com/discord/discord-api-docs/blob/3caae2cf67fa807961ce5591f1ebb9ed12ee7efd/docs/topics/RPC.md>
- Userdoccers — `rpc` scope rule (owner + whitelisted users), `RPC_PRIVATE_BETA` flag: <https://docs.discord.food/topics/oauth2> · <https://docs.discord.food/resources/application>
- Approved-RPC-apps tracking gist (Hacksore/Overlayed): <https://gist.github.com/Hacksore/24bf9f8a950b740cd914d62975accff0>
- App Testers flow (invite by username, accept via email): <https://support-dev.discord.com/hc/en-us/articles/21204493235991-How-Can-Users-Discover-and-Play-My-Activity>
- App verification (the 100-server bot track, checklist): <https://support-dev.discord.com/hc/en-us/articles/23926564536471-How-Do-I-Get-My-App-Verified>
- Developer support intake: <https://support-dev.discord.com/hc/en-us/requests/new> · <https://dis.gd/contact>
- Accessibility statement + feedback form: <https://discord.com/accessibility-statement>
- In-repo protocol notes (§4.4 approval gate): [discord-rpc.md](discord-rpc.md)
