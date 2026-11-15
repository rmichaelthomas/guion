# Guion first-hour trial, with people (protocol, script, scoring sheet)

Status: written 2026-11-14, not yet run. For Rob to run when there is time.
Companion to the agent website track (T6, harness H-1.4), which measures whether the
website alone gets an agent to a first running program. This one asks the question an agent
cannot answer: **does a person get there, and does it feel easy?**

## What this measures

One thing: a person who has never used Guion, given only the website, gets a first program
running and changes it, within an hour. Two numbers, never blended, as in the harness:

1. **Closed or not**, and the time to first output. Closed means: they ran the website's
   hello-world on their own machine and then ran their own change to it.
2. **Stumbles**, counted by the observer (see the sheet): each time the person stops, backtracks,
   says they are unsure, or does something that fails.

Plus two things the harness cannot see: **what they say in their own words** (think-aloud)
and **how they rate it afterwards** (three questions).

It does not measure whether the language is good, how fast people learn it over weeks, or
whether they would choose it. It is a first-hour test and nothing more.

## Who

- **3 to 5 people.** Three finds the big problems; five finds most of the rest. More than five
  repeats what the first five found.
- Each has written some code before, in any language, and has not used Guion. Mix: at least one
  person who writes code at work, and one who is newer to it. Not people who have seen the
  site or heard you explain Guion. Not collaborators on this project.
- Each uses their own computer. Mac (Apple silicon) or Linux (Debian 12-compatible, x86_64
  or ARM64) only, because those are the builds that exist. Anyone on another system is not a
  fit for this round.
- Record, for each person: age band not needed; years coding, main language, OS, terminal
  comfort (low/medium/high). Use a code (P1, P2, ...), never a name, in notes.

## Setup (10 minutes, before the first person)

1. Confirm the site is the version you mean to test. Note the date and the release (v0.4.0).
2. Run the website's own steps on a clean user account or machine yourself once, exactly as
   written, so you know the expected output and how long it takes.
3. Have a screen recorder ready if the person agrees (screen and voice only, no face). If they
   do not agree, take written notes only.
4. Print or open the scoring sheet below, one per person.
5. Have a way to tell the person the clock: you will keep it, they should not.

## Consent (read aloud, before anything starts)

> I am testing the website, not you. If something is confusing, that is a problem with the site,
> and that is exactly what I need to find. You can stop at any time. I would like to record your
> screen and your voice while you work; the recording is only for me, it will not be shared, and
> I will delete it after I have taken notes unless you say otherwise. Is that all right?

Record yes/no for recording. Do not proceed to recording without a yes.

## The task (give it on paper or on screen, word for word)

> Here is a website for a programming language called Guion: [the site URL].
> Using only this website, get a Guion program running on your computer. Then change it so it
> prints exactly: Hello, Ada!
> Please think out loud as you go: say what you are looking at, what you expect, and what
> confuses you. I will not be able to help you with the language, but I will be here.

Time cap: **60 minutes**. If they are not done at 60, stop. That is a result, not a failure of
the person.

## What the facilitator does and does not do

Do:
- Sit where you can see the screen. Say nothing except the prompts below.
- Keep the clock. Write down timestamps.
- If they ask a question, say: "What would you do if I weren't here?" Write the question down.
  A question asked is a stumble.

Do not:
- Explain Guion, the site, or the terminal. Do not point at the page. Do not say "try
  scrolling down".
- React to wrong guesses with sounds or faces.
- Rescue them early. Rescue only if (a) they say they want to stop, or (b) they are stuck on
  the same point for 10 minutes. Then give the smallest hint, write down exactly what you said
  and at what time, and mark the run "hinted".

Prompts you may use (only these):
- If quiet for 20 seconds: "What are you thinking right now?"
- If they say "I don't know what to do": "What are you looking for?"
- If they ask whether they are right: "What would tell you?"

## Marked moments (write the time for each)

- T0 task given
- T1 first scrolls to or clicks the Download / install area
- T2 archive on their machine
- T3 install command run
- T4 `guion` runs for the first time (any output, even `--version`)
- T5 hello-world prints `Hello, Guion!`
- T6 their change prints `Hello, Ada!`
- Stop time and reason: done / cap reached / person stopped / hinted

## After (10 minutes)

Ask these, in order, and write the answers as close to word for word as you can:

1. "In a sentence, what happened?"
2. "Was there a moment you almost gave up? What was it?"
3. "What was the first thing you looked for on the page that you couldn't find?"
4. "If you could change one thing on the site, what would it be?"
5. Rate each from 1 (very hard) to 5 (very easy): **Getting it installed. Getting hello-world to run. Changing it.**
6. "Would you tell a friend who codes to try it? Why or why not?" (Treat the answer as colour,
   not as a score.)

Do not defend the site. Thank them.

## Scoring sheet (one per person)

```
Person: P__    Date: ____    Facilitator: ____    Release/site version: ____
OS: ____  Years coding: ___  Main language: ____  Terminal comfort: L / M / H
Recording consent: yes / no

Moments (clock time and elapsed minutes)
T0 ____   T1 ____   T2 ____   T3 ____   T4 ____   T5 ____   T6 ____
Stopped: done / cap reached / person stopped      Hinted: no / yes (what, when)

Result:  CLOSED  /  NOT CLOSED        Time to T4: ___ min   Time to T6: ___ min

Stumbles (one line each; time; what the person did or said; where on the site it
happened, or "off the site" if they left it)
 #  time   kind   what happened / exact words                         site passage involved
 1
 2
 ...
Kinds:  S1 stopped >30 s with no action      S2 went backwards / redid a step
        S3 command or step failed            S4 asked me a question
        S5 left the site to search elsewhere S6 said they were unsure or confused
        S7 followed the site and it was wrong/missing (a site defect)

Totals:  stumbles ___   (S7 site defects: ___)   questions asked: ___

After the task
Quotes (verbatim):
  almost gave up: ____
  couldn't find: ____
  one change: ____
Ratings (1-5):  install ___   hello-world ___   change it ___
Colour (would recommend, why): ____
```

## Reading the results

After all sessions, one summary page, in this order:

1. How many of the people closed, out of how many. Median time to T4 and T6. (With three to
   five people say "3 of 4", not a percentage.)
2. Each distinct problem, with how many people hit it, and the site passage involved. A
   problem that two or more people hit is a defect to fix. One person hitting it is a note.
3. Which problems the agent track (T6) also found, and which only people found. The second
   list is the reason to run people.
4. Ratings as three numbers per person, not an average.
5. The verbatim quotes, grouped by theme.

Receipts: append the per-person sheets and the summary under `evaluation/comprehension/receipts/`
as H-1.4 human receipts, append-only, with the same caveats stated: small n, volunteers, one
site version, Mac and Linux only.

## Rules that keep it honest

- Do not fix the site between sessions. If P1 finds a big problem, finish the round on the
  same site, then fix, then re-run with new people. Otherwise the numbers mean nothing.
- Never count a hinted run as closed without saying it was hinted.
- A person who cannot get to the end is data. Do not exclude them.
- Report what people said even when it is unflattering to the site.
- Do not use the result as a ranking against any other language. It is only a test of this site.

## Before you run it, a checklist

- [ ] Site and release you are testing are live and match what you noted.
- [ ] You have walked the steps yourself once.
- [ ] 3 to 5 people confirmed, each with a Mac (Apple silicon) or Debian-compatible Linux.
- [ ] Recording method ready, consent text printed.
- [ ] Scoring sheets printed or open, one per person.
- [ ] Time blocked: 90 minutes per person (10 setup, 60 task, 10 questions, 10 slack).
