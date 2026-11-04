# Music.app 1.7.2 — motion (light mode)

## Status: NOT MEASURED

No frame-accurate recorder exists on this machine (`ffmpeg` absent; a
ScreenCaptureKit recorder is not built yet), so **no duration, easing or spring
value below is claimed**. Static window captures cannot establish timing.

## What is established from captures

* Page switches (Home → Radio → Library) replace the content between two
  captures taken 1.4 s apart with no intermediate frame observed: the swap is
  faster than the capture cadence, not necessarily unanimated.
* The search field's focus ring appears fully formed in the first capture after
  a click.
* The search suggestion list appears fully formed after typing; no drop shadow
  growth was captured.
* Sidebar hover produces **no** visual change (0-pixel image diff), so there is
  no hover transition to measure on that component.
* The player pill did not change between states because nothing was loaded
  (disabled controls, `#bcbcbc`).

## What would be measured, and how

Method once a recorder exists (owner approval needed for either option):

1. Build `scripts/ui-compare/recorder.swift` (ScreenCaptureKit, 60 fps, window
   only, H.264/ProRes to a temp file). No new runtime dependency for the app;
   dev-only.
2. Trigger one interaction per run (`input.swift`), 3 repetitions.
3. Extract frames, mark the first frame where a tracked region changes and the
   frame where it settles; report duration ± 1 frame, and the position curve of
   the moving element at 1/60 s.

Transitions to record, in order of value: sidebar page switch, search focus,
search-suggestion open, toolbar mode switch (Search ↔ library), player pill
state change (play/pause), queue panel open, lyrics panel open, popover/menu
open, window resize reflow.

## Reference values from Apple's own guidance (INFERRED, not measured here)

Use only as a starting point, clearly labelled in code as an assumption:

| Interaction | Damping | Response |
|---|---|---|
| Panel / popover present | 1.0 | 0.3–0.4 s |
| Momentum gestures | 0.8 | 0.3–0.4 s |

These come from Apple's published design guidance for fluid interfaces, not from
this Music build. They must be replaced by measurements before any claim of
motion parity.
