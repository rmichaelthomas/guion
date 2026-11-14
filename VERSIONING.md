# Versioning policy

Guion uses semantic versioning, plus a small set of rules for the surfaces a language has and a library does not.

This document states **rules**, not status. For the current version and artifact format, run `guion --version`. A status table would drift; the binary is the source of truth.

---

## 1. Two things a version says

A version string carries two separate promises, and conflating them causes most versioning arguments.

| Part | Promise |
|---|---|
| The **number** (`1.4.2`) | "I will not break your code." Compatibility. |
| The **label** (`-beta`, `-rc.1`) | "You can trust this build this far." Audience and confidence. |

They move independently. All of these are coherent:

```
2.0.0-beta.1   a breaking change, still in testing
1.9.4          a routine patch
0.2.0          no compatibility promise, but shipped and supported
1.0.0-rc.1     the code that will become 1.0.0 unless something blocks it
```

## 2. What each slot means

`MAJOR.MINOR.PATCH`

| Slot | Meaning |
|---|---|
| **MAJOR** | Code written for the previous major may need edits. A clean break. |
| **MINOR** | Additions only. Nothing removed, nothing re-signed. |
| **PATCH** | Fixes only. No new features, no breaks. |

So `1.4.2 → 1.5.0` is safe to take. `1.4.2 → 2.0.0` requires reading the migration guide.

### The 0.x rule

Semantic Versioning says: *"Major version zero (0.y.z) is for initial development. Anything MAY change at any time. The public API SHOULD NOT be considered stable."*

`0.x` therefore means one specific thing — **no compatibility promise** — and not "unfinished" or "not for use." At `0.x`, a *minor* bump is allowed to break. At `1.x` and above, only a *major* bump is.

**Breakage lives in MINOR at 0.x, and in MAJOR at 1.x.** That sentence is most of this policy.

### The 0.x exception is voluntary discipline, not law

While Guion is at `0.x`, the rules in §4 and §5 are what this project chooses to do, not what SemVer forces. They are followed so that the habit, and the tooling, exist before they become binding. Any deviation must be stated in the changelog with the same prominence as the change.

## 3. The seven compatibility surfaces

A language and toolchain has more public surface than a library. All seven are contracts.

| # | Surface | Includes |
|---|---|---|
| 1 | Source syntax | Keywords, statement forms, type syntax, literals |
| 2 | Builtins | Names and signatures of non-`std:` functions |
| 3 | Standard library | `std:` module names, function names, signatures, effects |
| 4 | Artifact format | The compiled artifact a release produces and the runtime accepts |
| 5 | Manifest | `guion.toml` schema, `[runtime]` keys, defaults |
| 6 | Error codes | `E_*` and `R_*` identifiers, and the fields they report |
| 7 | CLI | Command names, flags, and machine-readable output envelopes |

Anything not on this list is internal and may change at any time.

## 4. Deciding a bump

Ask in order. First match wins.

1. Does it **remove, rename, or re-sign** anything on the seven surfaces? → **MAJOR**
2. Does it change **behavior that was documented as correct**, such that existing code silently does something different? → **MAJOR**
3. Does it **add** to any surface? → **MINOR**
4. Does it change only behavior that was wrong, without touching any surface? → **PATCH**
5. Does it change the artifact format while old artifacts still load? → **MINOR** (see §5)
6. Does it change the artifact format so old artifacts no longer load? → **MAJOR**
7. At `0.x`: still apply 1–6, then state any break explicitly in the changelog.

Two consequences worth stating outright:

- **A new keyword is MINOR only if it cannot already be a valid identifier.** Otherwise existing programs stop compiling, which is MAJOR by rule 1.
- **An error code that moves is MAJOR**, because tooling matches on it. `E_NAME_UNKNOWN` becoming `E_UNKNOWN_NAME` breaks a reader that was written against the docs.

## 5. The artifact format rule

This is the surface SemVer does not answer for a language, so it gets its own rule.

- The format has a number, reported by `guion --version`.
- **The number is not the contract. Loading is.** Bumping the format number while old artifacts still load is MINOR.
- Artifacts that no longer load must be rejected with a typed, actionable error naming the format they carry and the format this build accepts. Silent misreads are never acceptable.
- A release may state a compatibility window, such as *"artifacts from this release forward run on all subsequent releases of the same major version."* That window is a promise; "all artifacts ever" is not, and is not offered.

## 6. Pre-release labels

Guion uses Semantic Versioning's pre-release syntax: `-alpha.N`, `-beta.N`, `-rc.N`.

| Label | Meaning |
|---|---|
| `-alpha.N` | Feature-incomplete. May not run. |
| `-beta.N` | Feature-complete or close; bug-hunting; surface may still shift. |
| `-rc.N` | Feature-frozen. Only blocking fixes accepted. If nothing blocks, **this exact build** becomes the release, with the label removed and the number unchanged. |

Guion does not use `-preview`. It is a channel word with no agreed meaning in version strings; `-beta` and `-rc` already say what it would be trying to say.

Dropping a pre-release label is a statement: *this build is supported and documented*. For example, the first release without a pre-release label was `0.2.0` — supported, still `0.x`, no compatibility promise yet. Those two facts are not in conflict.

## 7. Deprecation

Deprecation is how a removal becomes a MAJOR instead of a surprise.

- **Announce.** The thing is marked deprecated in the spec and in `guion doc` output, naming what replaces it.
- **Keep working.** A deprecated surface behaves exactly as before. Deprecation is a warning, never a behavior change.
- **Remove no earlier than a MAJOR.** Never in a MINOR, never in a PATCH.
- **One changelog line per deprecation**, naming the release it was announced in and the earliest release it may be removed in.

## 8. Applying this at 0.x

From `0.3.0` onward, treat MINOR as non-breaking **by choice**, even though `0.x` permits otherwise. Practising the promise before making it surfaces where the rules are inconvenient while the cost of adjusting them is zero.
