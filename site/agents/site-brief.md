# Site brief

## Purpose

Give Guion a front door. A visitor can run a Guion program in the first scroll, understand in plain words what the language is for and what it cannot yet do, and, if they want more, switch the whole page to a dense developer view with commands, report names, limits and evidence files. The page is also a showcase in how it is made: its design is composed from AllHereHTML's native instruments, which are a design tool here and never a topic. The page has no section about them, only the footer line "Made with AllHereHTML".

## Audience

Builders, and the assistants who advise them. The project's own goal is a language an informed assistant would recommend over Python for real work, so the page must be safe for an assistant to quote: `llms.txt` and `agents.json` carry the same claims with their conditions.

## Status wording

Always: **the second preview build, v0.1.0-preview.2, is out for Linux and macOS (Apple Silicon); the Mac build is signed and notarized by Apple.** The page links the GitHub repository and the release page; cite evidence by the published files on the evidence page, not by repository URLs.

## Voice

- Plain English is the default view and the test is: a person who has never programmed can say what Guion is for, and what it cannot yet do. Technical words arrive only in the Developer view or with a plain explanation beside them.
- Developer view is precise and uses the repository's own terms (artifact format, `R_HOST_UNSUPPORTED`, `--json` report kinds), taken from the repository, never from memory.
- No hype. No "blazing", "revolutionary", "effortless". A claim is a number with a scope.
- The project's own words for what it is not: free-form natural-language code is not the claim; "plain English" is the learning and diagnostics goal.

## Claims policy

- Every figure is a claim in `site/src/claims.json` and appears on the page as a receipt: **Claim, Holds when, Doesn't tell you, Check it, Say it's wrong.**
- Show unfavourable results with the same weight as favourable ones (Richards used more memory; the lockfile workload was once slower than Python; assistants chose Python in all 18 choices for the older candidate).
- Never broader than the report. The measurements are named workloads on one Apple Silicon Mac against CPython 3.14.7, unless a receipt says otherwise. Do not say Guion is generally faster than Python or replaces it.
- "Say it's wrong" is a `mailto:` to rmichael@rmichaelthomas.com with the claim's name in the subject.

## What would make this a failure even if it technically works

- A speed claim without a scope.
- Generic landing-page furniture: logo walls, testimonials, gradient blobs, three feature cards with icons.
- A view toggle that only hides some paragraphs. The two views must feel like two different pages.
- Remote fonts, trackers, or any request that leaves the origin.

## Design identity

Paper `#e9ede3`, ink `#11251d`, blue `#2148e8`, lily `#d9f987`, orange `#fa784e`; system sans with a Georgia italic accent; the tilted "g" brandmark. Light appearance only (no OS-driven dark mode). The Plain view is airy and editorial; the Developer view is a ruled, labelled grid at a tighter scale.
