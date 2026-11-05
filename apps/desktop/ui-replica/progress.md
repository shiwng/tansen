
## Batch 2 — SHELL geometry + glass (2026-11-04, in progress)

Measured geometry applied to the real shell (Change Order 1: shell first):
- Tokens: --toolbar-height 52 (new), --player-pill-height 54, --player-bottom-
  offset 19, --body-gutter 34. SearchField 421×38 (was 600×32). Pill width
  700 (was 600).
- Sidebar: rows h-8 (32), icons 16 px (was 20), labels 13 pt type-body (was
  14), gap-4 so labels start x54, row pl-[10px] (icon centre x30), radius
  rounded-lg (8, was 6), headers 11 px regular text-secondary pl-[3px] (x15).
- Verified by capture (dark, real content): sidebar rows/icons/labels match
  the measured Music geometry; pill 700×54 renders; toolbar 52 with the
  narrower centred search field.
- Light branch (forced data-theme, sync temporarily suspended + reverted):
  content #ffffff ✓, pill #fefefe ✓ (exact vs Music light), search field
  #ffffff ✓. Sidebar material stays dark under forced CSS-light — expected:
  the native material follows the WINDOW appearance, pairs with CSS only in
  the real follow-OS flow (light material #ededec measured in the earlier
  light session). Real light-material pairing = OWNER SESSION toggle.
- Dark pill interior sampled #2a2a2f vs Music #2a2a2a (Δ5, within ±6 glass
  threshold). Content #1f1f22 vs #1f1f20 (Δ2).
- WS-E gap found (owner navigated to Settings): three decorative placeholder
  rows — Last.fm/ListenBrainz, Discord Rich Presence, Listening parties, all
  "Not yet implemented" — R6 violation; to be removed/hidden in WS-E.

NOT RUN: interactive regression (input events still blocked — ZCode restart
pending). Dark accent on selected row: INFERRED (#0a84ff).
