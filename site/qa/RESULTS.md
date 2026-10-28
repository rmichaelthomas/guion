# Browser and quality results

Recorded on 2026-10-28 against `site/public` served by `node site/tools/serve.mjs` at `http://127.0.0.1:8080/guion/` (the same `/guion/` subpath GitHub Pages uses). Browser: **headless Chrome 156 only**. Firefox and WebKit are **not installed on this machine, so nothing here was run in them** (pending, not assumed). The page relies on `:has()`, native `<details>`/radios, all of which Firefox 121+ and Safari 15.4+ support, but that has not been checked.

Screenshots in this folder are evidence for the items below; failures found and fixed are listed with their fix.

## Passed

| # | Check | Result |
|---|---|---|
| 1 | Playground makes no request before Run | Network log after load: the page and `loader.mjs` only. After Run: `guion_compiler.wasm`, `guion_vm.wasm`. |
| 2 | Playground examples (real compile + ABI 1.0 run, in the browser) | hello → `Hello, Guion!`, 6 of 10,000 steps, 342 artifact bytes · option → `24` · records → `5`,`3` · ask-for-a-file → "The browser said no." (`R_HOST_UNSUPPORTED`) · never-stop → step limit · make-a-mistake → `E_TYPE_OPERATOR`, line 2, column 19. Run re-enabled after every failure. |
| 3 | Hostile playground input | `guion_vm.wasm` answered 404 → "The playground files did not load" with the file named, Run re-enabled · empty source → "There is nothing to run yet." · 70,000-byte source → "accepts at most 65536". |
| 4 | View switch | Mouse and keyboard (Tab to the radio, Arrow keys change view, visible focus ring). `/developer/` opens in Developer view with the Developer radio checked, is `noindex`, and the playground runs from it. |
| 5 | JavaScript disabled | View switch, Read-it tabs, permission ladder and receipt filter all work; the no-script notice and the "needs WebAssembly" notice are visible; the Run button is hidden rather than dead; the example source is readable; no console errors. |
| 6 | Overflow at 320, 360, 768, 1280, 1920 px, both views | No horizontal page scroll at any width (document width equals viewport width). 320 px is the layout of 400% zoom on a 1280 px window; real browser zoom was not driven. |
| 7 | Reduced motion | Transitions 0 s and `scroll-behavior: auto` under `prefers-reduced-motion: reduce` (0.3 s otherwise). |
| 8 | Forced colors | Selected view, tabs, ladder rungs and filters are marked with system `Highlight` colours (see `forced-colors-*-header.png`). |
| 9 | Print | **Plain English**: re-printed after the print fixes and pages 3–5 inspected: dark text on white, no reliance on background graphics, script-only controls hidden (25 pages). **Developer**: printed (29 pages) *before* those fixes and not re-checked afterwards. |
| 10 | Console | No errors or warnings on a normal load. |
| 10b | "Open in the playground" buttons | Read it (Handle absence) loads that example, selects it in the chooser, scrolls to the playground and focuses the editor; Run prints `24`. Permissions "Ask for a file" loads the refusal example; Run shows "The browser said no." With JavaScript off, no such button is visible. (A defect found in the final review: they were not wired and showed as dead buttons without JavaScript.) |
| 11 | Structure | One `h1`, one `main`, two labelled `nav`s, `lang="en"`, no unnamed buttons/inputs/radios, no duplicate ids, no positive tabindex, every link has text. |
| 12 | Contrast (computed WCAG ratios for every text/background pair used) | All at least 4.5:1 after the fixes below (lowest: `#52675b` muted text on the light-green panel, 4.76). |

## Failures found, and fixed

- **Overflow at 320/360 px and in Developer view at ≤768 px.** `1fr` grid tracks would not shrink below code blocks. Fixed with `minmax(0, 1fr)` and `min-width: 0` on grid children.
- **Forced colors lost the selected view.** The pill background is stripped in forced-colors mode, so Plain and Developer looked identical. Fixed with system-colour rules for every selected control.
- **Print was unreadable in the dark sections and the playground.** Browsers do not print backgrounds by default, so light-on-dark text vanished. Fixed with print rules that need no backgrounds.
- **Muted text failed contrast** (4.17:1 on paper, 3.87:1 on the light-green panel). Darkened `--muted` from `#607569` to `#52675b`.
- **Sticky header took about 15% of a phone screen** (wordmark and switch wrapped). Wordmark text is now hidden below 460 px; the header is 87 px tall.
- **Missing favicon logged a 404 console error.** Added an inline SVG favicon.
- **Tab order began at the view switch.** The skip link now comes first.

## Not done

- Firefox and WebKit (not installed).
- A screen reader walkthrough (VoiceOver, NVDA) was not run.
- Real browser zoom at 200% and 400%; width reflow at 640 px and 320 px was measured instead.
- No CI has run for the site. Actions is blocked by billing, so the site deploys from the `gh-pages` branch instead (`site/tools/publish-branch.mjs`). The live site was checked once in Chrome after the first branch deploy: playground run, both views, one origin contacted, clean console.
