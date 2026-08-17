# Release legal and privacy review

_Snapshot: 2026-08-17. Engineering-grade legal hygiene, not legal advice. Qualified counsel must review the final privacy, biometric, trademark, licensing, and distribution decisions in each launch jurisdiction._

## Verdict

**RED for public or commercial release.** Private development may continue, but a build should not be distributed beyond a tightly controlled development/test group until every red item below is closed and verified in the packaged artifact.

## Red release blockers

### 1. OAuth credentials are stored in plaintext

`app/src-tauri/src/rpc/auth.rs` persists Discord access and refresh tokens in `tokens.json`; OS keychain support is currently disabled. File permissions do not provide encryption at rest.

Before distribution:

- use macOS Keychain and Windows Credential Manager, or encryption whose key is protected by OS secure storage;
- migrate or securely remove legacy token files and revoke/rotate affected test tokens;
- test upgrade, logout, revocation, deletion, and secure-storage failure paths.

Discord's Developer Terms require commercially reasonable security and encryption of API data at rest.

### 2. Voice embeddings are automatically enrolled and persisted

`app/src-tauri/src/voiceid.rs` and `app/src-tauri/src/lib.rs` create and store voice embeddings without a separate informed opt-in. A backend clear command exists, but there is no complete user-facing consent, per-person deletion, retention, or encryption flow.

Default release posture:

- disable persistence and keep voice identity session-only;
- treat embeddings as sensitive biometric data, even though they are not raw audio or intended to retain words;
- require separate informed opt-in before creating a persistent embedding;
- disclose purpose and retention, provide per-person and full deletion, encrypt at rest, prohibit secondary use, and address consent from other speakers and minors.

Whether a fully local implementation falls within a particular biometric/privacy law depends on the facts and jurisdiction. Obtain advice for the intended markets.

### 3. Logs disclose transcript and identity data

Current stderr paths in `app/src-tauri/src/lib.rs` and `app/src-tauri/src/stt/whisper_engine.rs` include transcript text, channel/name data, Discord IDs, voice-refinement mappings, and similarity scores. This contradicts the local/private product promise.

Production diagnostics must contain only event types, counts, timings, and redacted error codes. Add automated tests that capture stderr and scan packaged release binaries. Keep WAV debug recording absent from public builds or behind a conspicuous development-only warning and informed consent.

### 4. Discord distribution and approval are unresolved

The default application is still subject to Discord's RPC tester/approval gate. `CALLOUT_CLIENT_ID` is an experimental development mechanism, not an approved public workaround. Do not use it to evade tester or App Review limits.

Obtain written Discord guidance covering:

- general RPC access and App Review;
- whether a bring-your-own application-ID flow is permitted;
- the application ID in public source code;
- local voice attribution and persisted embeddings;
- the final privacy/deletion behavior.

Do not submit the current approval dossier until it accurately discloses voiceprints, diagnostic logs, debug audio, token storage, post-uninstall data, licensing, and monetization.

### 5. Packaged artifacts omit legal notices

The inspected DMG contained the application binary/plist/icon but not the product license, privacy policy, or third-party/model notices. `NOTICE.md` is also not a complete dependency inventory: target trees include MPL-2.0 crates, and the WeSpeaker model requires CC BY 4.0 attribution.

Before release:

- generate an exact per-target Rust/npm/model inventory and SBOM;
- review obligations with tools such as `cargo-about`/`cargo-deny` plus npm production inventory;
- include the product license, privacy policy, third-party license texts/notices, model provenance/attribution, and required source location in the app and installer;
- make CI inspect the final DMG/installer and fail if required materials are absent.

### 6. UNMUTE is not a cleared product name

The USPTO shows an active UNMUTE registration, serial 98290215 and registration 8,254,640, for adjacent audio/music software services. This is not itself a finding of infringement, but it blocks serious investment in the working name until a qualified trademark search clears the US, Canada, EU, and other priority markets or the project is renamed.

### 7. Fortnite mockup media is not launch-cleared

`mockups/fortnite.png` is a Fortnite screenshot with a third-party “KWC” watermark and no documented provenance. Keep it out of README heroes, websites, social previews, advertisements, store listings, press kits, and commercial campaigns. Replace it with an original fictional game scene or media whose promotional rights are explicit.

## Yellow decisions and follow-up

- A commercial-use restriction makes future releases source-available, not OSI open source. See `LICENSING.md`; prior MIT grants remain available to their recipients.
- Add in-app Privacy, Terms, issue-reporting, delete-all-local-data, and revoke-Discord-access controls before release.
- Replace the Discord-derived Blurple/multicolor visual direction with a distinct accessible palette and keep a clear non-affiliation statement.
- Support performance, latency, accuracy, GPU, compatibility, and privacy claims with target-specific evidence; avoid absolutes.
- Preserve original notices for adapted/copied code and keep provenance/review records for AI-assisted work.
- Use a contributor agreement granting appropriate relicensing rights if future dual licensing is important; a DCO alone typically does not grant that authority.

## Green foundations

- No telemetry or cloud STT was found; observed network use is limited to Discord authorization/content and model downloads.
- Model downloads use pinned SHA-256 hashes for the initial download path.
- `DISCLAIMER.md` and `TERMS.md` state that the project is not affiliated with Discord.
- `AGENTS.md`, `PROJECT_STATE.md`, `BRAND.md`, and `LICENSING.md` now encode the release constraints for future agents.

## Recommended remediation order

1. Remove personal/transcript logging and disable persisted voiceprints by default.
2. Move OAuth credentials to OS secure storage and migrate/revoke plaintext files.
3. Request written Discord guidance and approval.
4. Correct Privacy/Terms and implement in-app consent, deletion, and revocation controls.
5. Generate the dependency/model inventory and package every required notice.
6. Replace Fortnite media and Discord-derived brand colors.
7. Clear or replace the working product name.
8. Choose and execute any prospective relicensing as one counsel-reviewed change.

## Primary sources reviewed

- [Discord Developer Terms of Service](https://support-dev.discord.com/hc/en-us/articles/8562894815383-Discord-Developer-Terms-of-Service)
- [Discord Developer Policy](https://support-dev.discord.com/hc/en-us/articles/8563934450327-Discord-Developer-Policy)
- [Discord RPC documentation](https://docs.discord.com/developers/topics/rpc)
- [Discord Brand Guidelines](https://discord.com/branding)
- [Epic Games Fan Content Policy](https://legal.epicgames.com/epicgames/fan-art-policy?lang=pt-BR)
- [USPTO TSDR: UNMUTE serial 98290215](https://tsdr.uspto.gov/statusview/sn98290215)
- [Open Source Definition](https://opensource.org/osd) and [OSI FAQ](https://opensource.org/faq)
- [PolyForm licenses](https://polyformproject.org/licenses/)
- [Mozilla Public License 2.0](https://www.mozilla.org/en-US/MPL/2.0/)
- [WeSpeaker pretrained-model list](https://github.com/wenet-e2e/wespeaker/blob/master/docs/pretrained.md) and [CC BY 4.0 legal code](https://creativecommons.org/licenses/by/4.0/legalcode.en)
- [Office of the Privacy Commissioner of Canada biometric guidance](https://www.priv.gc.ca/en/privacy-topics/health-information-genetics-biometrics/biometrics/gd_bio_org-final/)
- [OPC voiceprint findings](https://www.priv.gc.ca/en/opc-actions-and-decisions/investigations/investigations-into-businesses/2022/pipeda-2022-003/)
- [Illinois BIPA §10](https://ilga.gov/legislation/ilcs/fulltext?DocName=074000140K10) and [§15](https://ilga.gov/legislation/ilcs/fulltext?DocName=074000140K15)
- [GDPR](https://eur-lex.europa.eu/legal-content/EN/ALL/?uri=celex%3A32016R0679)
- [British Columbia Personal Information Protection Act](https://www.bclaws.gov.bc.ca/civix/document/id/complete/statreg/03063_01)

Re-check all external requirements when implementation, distribution, monetization, or launch jurisdictions change.
