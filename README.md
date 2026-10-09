# repame web game template

One tree, three targets: desktop for development, wasm for Poki and every
portal the Playgama bridge reaches. The template ships a complete demo game
(move, collect orbs, run out of clock, watch an ad, revive) so every platform
hook is wired and testable before real gameplay exists.

```
Cargo.toml / Cargo.lock      repame 0.3.11 + repose 0.31 from crates.io; commented
                             [patch.crates-io] block redirects to ../Repos
Trunk.toml / index.html      public_url = "./", so portals can serve from a subpath
playgama-bridge-config.json  ad placements: game_over (interstitial), revive (rewarded)
src/
  lib.rs / main.rs           desktop + wasm entries; wasm pulls the bridge before boot
  app/mod.rs                 the shell: frame loop, input merge, schedule gate, entries
  app/platform.rs            the only file that talks to Playgama (ads, pause, mute, cloud)
  app/audio.rs               procedural sfx, no audio files
  sim/state.rs               run state machine: menu, playing, break, death panel, revive
  sim/platform.rs            headless platform intents, no SDK types
  sim/save.rs                local file (localStorage on wasm) + cloud merge
  rigs.rs                    renamite rig hosts: art layer, playback, placement
  ui/                        HUD, panels, virtual stick on a 960x540 design grid
assets/rigs/                 .svg authoring exports, .ren rig sources, Fredoka
scripts/new-game.sh          copies and renames the template into a new game
```

## Start a new game

```sh
scripts/new-game.sh orb-dash "Orb Dash"     # copies to ../orb-dash
cd ../orb-dash
cargo run                                    # desktop
trunk build --release                        # dist/ for the portal
```

## The rules this template encodes

- Engine versions come from crates.io, not local paths. To pick up an
  unreleased fix from `~/Documents/Repos`, uncomment every
  `[patch.crates-io]` line or none: a half-patched tree puts two copies of
  the renderer in one binary and sprites vanish.
- The sim never sees the SDK. Headless tests drive ads by hand; desktop
  behaves like web minus ads.
- Breaks have watchdogs (10 s interstitial, 30 s rewarded) so a silent SDK
  cannot freeze a hosted game. A rewarded ad that pays nothing lands on the
  death panel without the revive button.
- `game_ready` fires only after the first playable frame, and gameplay
  start/stop bracket every run.
- Editor buttons are Material Symbols ligatures (`pause`, `volume_up`,
  `menu`); repose bundles and registers the face. Score chips are text.

## Art

Art is renamite rigs, not sprites. `assets/rigs/*.svg` are the authoring
exports (VectorCraft), `*.ren` is the rig source of truth and what ships via
`include_str!`, and `src/rigs.rs` hosts each rig through `repame-actors` and
paints it as a Repose view. Authoring loop:

```sh
renamite import-svg art.svg --output art.ren   # SVG -> rig
renamite validate art.ren                      # check it
renamite pack art.ren                          # only once the published runtime reads it
```

Pack to `.renb` when the published renamite runtime can read the local
CLI's binary format; today the text form is what parses, so the template
ships `.ren`.

House style: flat saturated fields, no gradients; everything wears the same
ink outline (`INK`, ~3 px at the design grid); the backdrop is a sunburst
over one sky tone with two hills in the darker sibling tone; characters are
stickers with a belly patch and two cream eyes. Text is Fredoka Bold, cream
with an ink glyph stroke on the sky, plain ink on cream cards, always
`single_line`. To restyle a game, change the colors in `ui/palette.rs` and
the tints in `src/rigs.rs`.

## Tests

`cargo test` covers the ad verdict state machine.
