# Study notes (REA) — third-party repos

Purpose: techniques and structure to check against the real app, never a
source of truth. Clones live in gitignored `ui-study/`; only code from a clear
permissive licence may be copied, with attribution in `NOTICE`.

Checked this session (shallow clone, `git log -1`, top-level licence file):

| Repo | Licence | Last commit | What to reuse | What NOT to trust |
|---|---|---|---|---|
| Musish/Musish | **AGPL-3.0** | 2020-08-30 | Sidebar/list/queue structure and route naming as a checklist only | Anything visual: it is a 2020 web player, years behind this Music version. Stale. No code. |
| agmmnn/tauri-controls | **none found** (no top-level LICENSE in the clone) | 2026-02-27 | Confirms the shape of the problem (custom macOS traffic lights in Tauri 2) | No code. Unlicensed = all rights reserved. Native traffic lights are preferred anyway. |
| ciderapp/Cider-2 | **none found** (no top-level LICENSE in the clone) | 2026-10-26 | Read its window-chrome and now-playing patterns as concepts | No code. Unlicensed until proven otherwise; also an Apple Music client, so its look may be deliberately different. |

Not studied (NOT DONE, no time in M1): `adityogi/Music_Project`,
`QenTerra/cadence` (MIT), `nklmantey/apple-music-ui-clone`, Apple's macOS
Design Resources / HIG, plus repos found by further `gh search` queries
(`gh` is not installed on this machine; web search only).

Consequences for the build:

- No code from any studied repo will be copied; `NOTICE` stays unnecessary
  unless that changes.
- Every layout number comes from `reference.md` (real app), not from these
  repos.
- Tauri window chrome (M2) is built from Tauri's documented window options and
  macOS's own traffic lights, not from `tauri-controls`.
