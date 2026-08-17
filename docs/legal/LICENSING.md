# Licensing decision for UNMUTE

_Decision memo, 2026-08-17. Engineering-grade legal hygiene, not legal advice. Have qualified counsel review the final choice, especially before monetization or broad binary distribution._

## Current state

UNMUTE is currently published under MIT. The present `LICENSE` grants anyone permission to use, modify, distribute, sublicense, and sell copies, subject primarily to preserving the notice and disclaimer. The README, `Cargo.toml`, `NOTICE.md`, `DISCLAIMER.md`, `TERMS.md`, and strategy documents currently reflect MIT/open-source positioning.

The Git history lists one human committer, but relicensing still requires confirming ownership of every copied or contributed portion. Third-party libraries and downloaded models keep their own licenses regardless of the project license.

Treat already published MIT versions and existing copies as still carrying their original grant unless counsel concludes otherwise. A new license should be presented prospectively with a clear effective version or commit; it should not claim to erase earlier recipients' rights.

## The unavoidable terminology choice

“Source code is public” and “open source” are not synonyms. An OSI-style open-source license cannot prohibit business use or resale. If UNMUTE prohibits commercial use, describe it as **source-available** (or “source-visible under a noncommercial license”), not open source.

## Maintainer direction

**Decision recorded 2026-08-17: option B.** Future licensing should allow personal, community, nonprofit, and internal accessibility use — including use by an employee at a for-profit organization — while prohibiting resale, white-label distribution, paid embedding, and competing products or hosted services built from UNMUTE.

Incidental commercial context, such as a monetized streamer using captions for their own access, should not by itself become a prohibited competing use. Selling UNMUTE, charging for access to a derivative, bundling it into a paid product, or using its code to offer a competing caption service should require separate written permission.

This records the business intent, not the final legal text. `LICENSE` remains MIT until counsel selects or approves exact standardized source-available terms, confirms definitions such as “competing,” “product,” and “service,” and the full relicensing change set is ready. PolyForm Shield/Perimeter are candidates to evaluate; PolyForm Noncommercial is broader than the chosen intent because it can also restrict benign internal commercial use.

## Practical options

| Goal | Direction | What it does not do |
|---|---|---|
| Keep true open source, discourage closed forks, protect the official identity | GPL/AGPL-style copyleft plus a separate UNMUTE trademark policy | Cannot prohibit sale or commercial use |
| Allow noncommercial use, modification, and redistribution; require permission for commercial use | PolyForm Noncommercial 1.0.0, optionally paired with a separate commercial license | Is not open source; “commercial purpose” is broader than simply reselling the app |
| Allow most use but block competing products/services | A standardized source-available competitor-focused license such as PolyForm Shield/Perimeter, after counsel reviews fit | May still allow commercial internal use and may be harder for users to interpret |
| Let people inspect/use but not redistribute or modify | A stricter source-available/proprietary grant | Gives up much of the community and accessibility benefit of public development |
| Keep all rights reserved | No public software license, with the repository public only for viewing/forking under platform terms | Others generally cannot legally contribute, redistribute, or build derivatives; poor fit for community development |

Avoid inventing a custom one-paragraph “no selling” addendum. Standardized terms reduce ambiguity and make package scanners, contributors, and commercial users more likely to understand the boundary.

## Recommended decision path

The chosen intent is narrower than “no commercial use”: protect accessibility use while blocking resale, white-labeling, paid embedding, and competing offerings. Start counsel review with a competitor-focused standardized source-available license such as PolyForm Shield/Perimeter rather than PolyForm Noncommercial.

Before adopting exact terms, settle these implementation questions with counsel:

1. How should “competing product or service” be defined without accidentally blocking integrations and accessibility research?
2. Which forms of paid support, consulting, app bundling, gaming-café use, and esports-team use require permission?
3. Should noncommercial forks and modified binaries be redistributable?
4. What commercial-exception process, pricing, and trademark permission should exist?
5. What version/commit starts the new grant, and how will earlier MIT code remain identifiable?

The answers may point to a narrower competition restriction or a trademark/distribution policy instead of a blanket noncommercial license.

## Atomic migration checklist

After the maintainer chooses the policy and counsel reviews the terms, change all of these together:

- replace `LICENSE` with the exact standardized license text and required notice;
- update `app/src-tauri/Cargo.toml` with the correct SPDX identifier or an accurate custom-license reference;
- update README status, badges, contribution language, download copy, and the words “free,” “open source,” and “commercial” consistently;
- update `NOTICE.md`, `DISCLAIMER.md`, `TERMS.md`, `docs/STRATEGY.md`, and `docs/TESTING.md` without changing third-party licenses;
- add a short notice explaining the effective version/commit and the continued MIT status of earlier releases;
- decide whether contributions require a contributor license agreement or developer certificate of origin so future relicensing authority is clear;
- separate the UNMUTE name/logo policy from copyright permissions;
- audit release artifacts so the chosen license and third-party notices ship with binaries;
- remove or separately license marketing/mockup assets that the software license cannot cover, including third-party game screenshots.

Do not merge a partial license change. Contradictory metadata can mislead users and undermine enforcement.

## Primary references reviewed

- Open Source Initiative, [Open Source Definition](https://opensource.org/osd) and [FAQ](https://opensource.org/faq): commercial use and sale cannot be prohibited by an open-source license.
- [PolyForm Noncommercial 1.0.0](https://polyformproject.org/licenses/noncommercial/1.0.0/): standardized source-available terms for noncommercial purposes with modification and distribution permissions.
- [PolyForm licenses](https://polyformproject.org/licenses/): standardized alternatives when the intended boundary is competition rather than all commercial use.
- [GitHub repository-licensing documentation](https://docs.github.com/en/repositories/managing-your-repositorys-settings-and-features/customizing-your-repository/licensing-a-repository): a public repository is not automatically open source.

Re-check the current text of every chosen license and platform policy at the time of the actual change.
