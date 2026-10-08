# Character motion

The character is **bloub** (Jérémy Perret, MIT — `src/character/bloub/LICENSE`):
one filled body that morphs between 14 measured states, two capsule eyes punched out of
it as holes, decor dots, a notification pastille and 3D rings. Its engine is
framework-free and clock-free — `engine.sample(t)` is a pure function of time — and is
ported unchanged apart from four marked additions (`Roadeep :` comments): a numeric eye
matrix for Canvas, a `setLife` eye-motion preference, a dated `blink(now)`, and a lazily
built eye-fit table (same values, no 100 ms at start-up). bloub's own docs explain the
numbers; do not "round" them.

## Layers

| File | Role |
| --- | --- |
| `bloub/*` | The ported engine, states, shapes, colours, expressions, gaze rules, arrival, and their tests (French, as upstream). |
| `motion.ts` | Pure tables: app state / emote / celebration → bloub states, montage blocks and expressions; `SETTLE_AT`; `choreographyAt`. |
| `fixed.ts` | Keeps the body the chosen shape: composes the drawn frame from the sampled one (see below). |
| `render.ts` | `paintFrame` draws one frame on Canvas 2D with `Path2D` (the island's renderer). |
| `engine.ts` | `BotEngine` facade with the API the island, mini characters and settings use. Owns the clock. |
| `appearance.ts` | `{shape, color, expression}` setting, migration, `CHARACTER_OPTIONS`, `applyCharacterPatch`. |
| `labels.ts` | Persian/English names of every shape, colour, expression and state (`character.*` i18n keys). |
| `greeting.ts` | The launch greeting, now drawn with bloub. |
| `minibots.ts` | Mini characters (agents, integrations): bloub in the chosen shape and the agent's colour. |

## The body never changes shape

The outline and colour are always the ones chosen in Settings — in every state, emote,
celebration, slap, arrival, file drag-over, the greeting and the drop sequence. bloub's
states morph the body itself, so `fixed.ts` overrides the sampled frame (the engine is
untouched): the body path is the chosen shape's silhouette; everything else — eyes,
expressions, gaze, blinks, rings, ribbons, pastille, particles — is kept as sampled.

- **Eyes are always there.** A second bloub engine held on the resting face (same
  expression, cursor gaze and blinks) supplies the eyes whenever the animated state
  shows none (`thinking`, `alert`, `exclaim`, `sleep`, early `burst` / `comet`); the two
  cross-fade during state fades.
- **Glyph states.** Where the silhouette *was* the animation — the thinking dots, the
  "!" of `alert` and `exclaim`, the bouncing `sleep` dot — that glyph plays as sampled
  at 40 % beside the head (top right), fading in when entering from a body state.
- `burst`'s particles spiral in front of the body (behind it they would never show).
- `egg`, `hexagon`, `play`, `orbit`, `comet` keep their eyes, rings and ribbons on the
  fixed body. The arrival's turn of the eyes plays on the chosen shape; the drag-over
  mailbox only opens its slot.

## App states

| App state | bloub (intro → loop) | Expression | Reduced motion |
| --- | --- | --- | --- |
| `idle` | `idle` | chosen in Settings | `idle` |
| `working` | `play` → `thinking` (three dots) | — | `thinking` |
| `thinking` | `orbit` ⇄ `egg` | — | `egg` |
| `searching` | `comet` ⇄ `wide` | — | `wide` |
| `approval` | `alert` ⇄ `exclaim` | — | `exclaim` |
| `question` | `notify` | — | `notify` |
| `error` | `burst` → `idle` | `triste` | `idle` |
| `finished` | `wink` → `idle` | `heureux` | `idle` |
| `ratelimit` | `hexagon` (a stop sign) | — | `hexagon` |
| `sleeping` | `sleep` | `somnolent` | `sleep` |
| `dizzy` (3 slaps) | `burst` → `idle` | `confus` | `idle` |

Emotes turn the face toward you for their duration with an expression — love
`heureux`, surprised `surpris` + `wide`, proud `fier`, wink `heureux` + `wink`, yawn
`somnolent`, happy `hilare`, annoyed `colere`. Task and focus celebrations play `swirl`
(rings and a turn of the eyes) with `heureux` / `hilare`; with reduced motion only the
face changes; mini characters never celebrate. A slap squashes the body and frowns.
While a file hovers the island a mailbox slot opens in the body (surprised while open,
happy while chewing). When the island reveals from hidden, bloub's arrival plays: the
eyes travel a full turn round the ball. All 14 catalogue states and
`swirl` are used; `motion.test.ts` locks that.

## Resting life, cursor and eye motion

On the resting face (`idle`, `swirl`) the eyes follow the cursor through bloub's `Look`
(absolute yaw/pitch mixed by the engine). Settings → General → eye motion: `normal`
follows fully with bloub's drift and blink schedule; `calm` follows at half range with
slower blinks; `still` neither follows nor drifts and blinks slowly. An OS reduced-motion
preference turns `normal` into `calm`, skips intros/loops and the arrival, and pins
looping and glyph states (dots, sleep bounce, "!") to the still frame after their
fade, so the frame loop stops.

## CPU

The facade's clock advances only in `update(dt)` (dt capped like bloub's player), so a
sleeping frame loop freezes the character rather than making it jump. `busy` is false
once the current bloub state has settled (`SETTLE_AT`) and nothing else moves (fade,
look catch-up, expression or shape morph, emote, squash, mailbox, arrival); looping
states (`thinking`, `sleep`, `alert`, alternating montages) keep it true, as the old
bouncing/scanning states did. The island never draws while hidden.

## Settings

Settings → General → Character appearance: 8 shapes, 12 colours and 16 resting
expressions as accessible radio groups (arrow keys, Home/End) with still previews, a
live animated preview on the island's black (eyes follow the pointer), and a strip of
the 14 states on the fixed body — tap one to play it, or play them all. Stored as bloub ids in
`characterAppearance`; pre-bloub `{body, eyes, color, accessory}` migrates (Rust and TS).

## Review in a browser

```text
http://127.0.0.1:1420/dev/character-preview.html?shape=nuage&color=violet&expression=curieux&state=thinking
```

Shows the island character at 240 px and at the native diameters 20, 44 and 62 px,
every app state, emote, arrival, celebration, slap and mailbox, and a board of all 14
states (+ `swirl`) on the fixed body, replaying on their measured durations. Automation hook:

```js
window.characterPreview.setState("approval");
window.characterPreview.renderAt(1.2); // pauses; renders 1.2 s after a fresh start
window.characterPreview.play();
```

## Verification

```powershell
npx vitest run src/character src/settings/appearance.test.ts
npx tsc --noEmit
cargo test -p roadeep settings
node_modules/.bin/tsc.cmd --noEmit --target ES2022 --module ESNext --moduleResolution bundler --lib ES2022,DOM,DOM.Iterable --strict --noUnusedLocals --noUnusedParameters --skipLibCheck --types vite/client dev/character-preview.ts
```
