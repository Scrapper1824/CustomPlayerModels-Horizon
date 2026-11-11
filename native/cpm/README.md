# CPM (Horizon native mod)

Draws you (the local player) with a [Customizable Player Models](https://github.com/tom5454/CustomPlayerModels)
model: a `.cpmmodel` file exported from the CPM editor. The model's own animations play (walking, sneaking,
swimming, flying, ...), and a gesture menu plays its gestures and custom poses. Other players are drawn with the
models stored in their skins (see [Other players](#other-players)); CPM servers and Horizon worlds are to come.

## Install

The Horizon launcher compiles native mods into the game: the first build downloads Rust and can take 20 to 30
minutes, later ones are faster. Native mods are not sandboxed, so the launcher makes you confirm that you trust the
code before it builds. See Horizon's
[native mods](https://github.com/horizon-rust/horizon/blob/main/docs/native-mods.md). The paths below are for the
default **Horizon** instance, whose game folder is `%APPDATA%\horizon`. For another instance, use its own folder.

`native/tools/native.py` needs **Python 3.11 or newer** (it reads TOML with `tomllib`).

On Windows the launcher's setup does not put `horizon-launcher` on `PATH`. The setup installs it to
`%LOCALAPPDATA%\Programs\Horizon Launcher\horizon-launcher.exe` unless you picked another folder, so call it by
that path (in PowerShell: `& "$env:LOCALAPPDATA\Programs\Horizon Launcher\horizon-launcher.exe" native-build ...`),
or use the launcher window's **Build** button described below.

**As an installed mod (kept with the instance's other native mods):**

1. Make the file: `python native/tools/native.py package cpm` writes `native/target/cpm-0.2.0.hnmod`.
2. Copy it to `%APPDATA%\horizon\horizon-native-mods\cpm.hnmod`. Create the folder if it is missing.
3. Turn it on: add the line `mod.cpm:true` to `%APPDATA%\horizon\horizon-mods.txt`. Create the file if it is
   missing. The launcher builds only the native mods that are turned on there.
4. Build. Either run `horizon-launcher native-build --trust-native-code`, which builds the selected instance's
   installed and turned-on native mods (add `--instance <name>` for another instance), or use the launcher window:
   on the Mods page, the "Native build" row says **Not built** or **Needs rebuild**; press **Build**.

**Straight from this source folder (only CPM):**

```sh
horizon-launcher native-build native/cpm/mixins/cpm --trust-native-code
```

The folder must be the one holding `horizon-mixin.toml` (`native/cpm/mixins/cpm`), or its parent
(`native/cpm/mixins`). `native/cpm` itself is not found. The selected instance then plays a build that holds only
the mods in that folder.

Either way, once it is built, play that instance. You can switch the mod off and on in the game under
Options → Mods.

## Use

1. Put your models in `%APPDATA%\horizon\player_models` (the `player_models` folder in the game folder; the
   **Open models folder** button on the settings page makes and opens it).
2. Pick one in Options → Mods → CPM → Settings: the **Model** button goes through None, then each
   `.cpmmodel` file in the folder by name, then None again. The choice is kept (the `model` setting) and comes
   back at the next start; if its file is gone, you are drawn vanilla.
3. Press F5 to see yourself.

If a model cannot be drawn, the settings page shows an `Error:` line (also in the log, `cpm: <file>: ...`). A
model file needs its own embedded texture (no skin fallback yet), and the texture must be at most 1020x1024 (with
the model's elytra and cape textures, if it has them, beside it: their widths plus 4 at most 1024).

### Model stored in your skin

CPM can store a model in the free pixels of a skin (its editor's export to skin), so it travels with your Mojang
skin. The mod reads it from your downloaded skin file (the game's skin cache, `assets/skins/<2 hex>/<digest>`, the
PNG as downloaded: the game's processed copy loses the data), as CPM's `SkinDataInputStream` does with its free
space template: the slim or wide channel per your skin's model, turning to the other when the stored model says
it is of the other type.

- **When nothing is chosen** (Model: None), the skin's model is drawn, as in CPM. The Model button then shows
  **Your skin's model**, and the settings page says `Using the model stored in your skin`.
- **Choosing it**: while your skin holds a model, **Your skin's model** comes first in the Model button's cycle and
  in the picker (a name tile); choosing it keeps the `model` setting as `<skin>`. A chosen file is drawn instead.
- **Links** in it (CPM's usual export: the model in a gist) download and are cached as a link model's.
- **Texture**: a stored model without a texture of its own draws with your skin, as the game draws it.
- **No model**: a skin without one (or a default or offline skin) draws you vanilla, with no error, even with
  **Your skin's model** chosen. Data that does not read (a bad checksum, cut short) is an `Error:` line,
  `cpm: skin model: ...`.
- A skin change (a new download) reads it again. Other players' skins are read too, for their own models
  ([Other players](#other-players)).

A model's animated textures (regions of its skin, cape or elytra texture that step through frames, or blend from
one frame into the next) play as in CPM, in the world and in the gesture menu's and picker's previews. The texture
is sent to the game again only when a frame changes its pixels (a blending one every 50 ms, so
it blends in 20 steps a second rather than every frame).

### Gesture menu

The key **Gesture menu** (`G` by default, rebindable in Options > Mods) opens the model's gesture menu while you
are playing (not in menus or chat). The settings page's **Gestures…** button asks for the same menu: it opens once
you close the menus and are back in the world (it never opens over another screen).

The menu is laid out as CPM's own: a top row with **Reset Gesture** (stops the playing gesture) and **Reset Pose**
(clears the custom pose), then the model's buttons in the model's order, four to a row (fewer on a narrow window).
Buttons the model hides are left out. When they don't all fit, **<** and **>** page through them. A model with no
buttons (or no model) shows `This model has no gestures`.

- **Gesture**: a click plays it (a click while it plays stops it). It shows green while it plays.
- **Custom pose**: a click selects it, shown green; a second click or **Reset Pose** clears it.
- **Toggle**: a click flips it; the bar at its left is green when on, red when off.
- **Slider**: click or drag along it; the marker and the number show the value.
- **Dropdown**: its name is above it and the current choice in it (`None` for no choice). A click opens the list
  of options; a click on one chooses it, a click anywhere else closes the list.

The right of the menu (about 40% of its width) shows the model live in 3D: posed as the world draws you (the same
movement, gestures, poses, sliders, toggles, dropdowns and worn armour and elytra; standing when the world is not
drawing you, as in first person), shaded as the game's inventory player is. It starts turned 30°; drag on it to turn
it (left and right turn it round, up and down tilt it, at most 60° either way) and turn the mouse wheel over it to
zoom (0.5× to 3×). A drag that starts on a slider still moves the slider.

**Models** (bottom left) opens the model picker in the same screen: the `.cpmmodel` files of `player_models` (read
again each time it opens) as tiles, each with the model's own picture (32×32; a file without one shows its name), page
by page (**<** / **>**, or the mouse wheel over the tiles). Hover a tile for its full name. Click a tile to preview
that model in 3D on the right, standing at its default settings (drag and wheel as above). **Use this model** switches
to it, as the settings page's Model button does (kept in the `model` setting, its saved parameters restored), and
returns to the gestures; **Back** returns without switching. Files are read in the background, so the game never waits
for them. A file that does not read shows `Can't read this model` (its name in red); a link model whose gist was never
downloaded shows `Select it once to download it` (using it downloads it); a model whose texture has no room left in the
mod image shows `Too large to preview`. A model made by an older converter shows the settings page's note under it.

Escape closes the menu; other keys do nothing while it is open. The menu and the gist popup share CPM's one mod
screen, so one waits for the other to close.

The model's parameters (toggles, sliders, dropdowns) are kept per model file in the mod setting
`params.<model file name>` as lowercase hex, and restored when the model loads. A saved value of the wrong length or
bad hex (the model changed) is ignored. Gestures and custom poses are not kept.

### Armor and elytra

The game leaves out its own armor and elytra for a player a mod draws, so the model draws them, as CPM does:

- A model without armor parts of its own wears normal armor: the vanilla helmet, chestplate, leggings and boots
  pieces on its head, body, arms and legs, with the armor's texture (leather in its dye, with its overlay; trims;
  an OptiFine CIT armor texture when a resource pack has one).
- A model with custom armor parts (made in the CPM editor) shows each piece's parts only while you wear that
  piece, textured with that armor's texture (leggings with the leggings layer). Where the model hides the vanilla
  piece, only its custom parts show. A model's **Armor** toggle (or any button that hides its armor parts) hides
  them as it does in CPM.
- An elytra in the chest slot shows the model's own elytra parts, or the vanilla wings, with the model's elytra
  texture if it has one, else the elytra's (a CIT elytra texture, or your cape's elytra as the game draws it).
- Only vanilla armor (`minecraft:<material>_helmet` and so on) is drawn as armor: a modded armor item shows
  nothing on the model, and does not count as worn. A pumpkin, head or other item in the head slot is a head
  item (below). Armor in first person is not drawn.
- Enchanted armor glints as the game draws it: on the piece's first texture and its trim (not leather's
  overlay), on the model's textured armor parts and the vanilla pieces alike (colour and glowing cubes do not
  glint, as in CPM). An enchanted elytra's textured wings glint whatever texture they take (the model's elytra
  texture, a CIT one, your cape's).
- Custom armor means resource packs that retexture vanilla armor and OptiFine CIT armor: the equipment asset of a
  non-vanilla item (a server's `equippable` `asset_id`) is not read.
- A texture with an OptiFine emissive overlay (`_e`, or the pack's own suffix) is drawn again with it at full
  brightness, without the glint: each armor texture's, and the elytra's (not over the model's elytra texture or
  your cape).

### Capes

The game leaves out its cape for a player a mod draws, so the model draws it, as CPM does:

- A model with a cape part (CPM's cape root) draws it where the vanilla cape hangs. That is on the vanilla body,
  as in CPM, so moving or animating the model's body does not move it; only the cape part's own position and
  animations do.
  - It swings with your movement as the vanilla cape does.
  - It uses the model's own cape texture if it has one, else your cape. With neither, no cape is drawn (CPM
    draws a template cape that this mod does not ship).
  - A hidden cape part with nothing on it hides the cape.
  - It still shows while you are invisible, as in CPM.
  - The model's render scale applies to it.
- A model without a cape part wears your normal cape (if you have one) as the game draws it: on the vanilla
  body, without the model's render scale, and not while you are invisible.
- No cape is drawn under an elytra (or any chest item with wings). Over a chestplate the cape moves out as the
  game moves it.
- With the game's Cape skin option off, a cape part without its own texture shows nothing (CPM would still show
  your cape there).
- In the gesture menu's preview the cape hangs as it last did in the world, or still in first person. In the
  picker's preview it hangs still.

### Held and head items

The game leaves out its held items and head item for a player a mod draws too, so the model draws them, as CPM
does:

- The stacks in your hands (the selected hotbar slot and the off hand) are drawn at the model's hand item parts
  (CPM's item renderer, `Right Hand Item` / `Left Hand Item` in the editor), following their animations; a
  model without one, or whose part is not drawn this frame (inside an armor part while that armor is not worn),
  holds the item at its arm as vanilla does. The main hand is the right unless the game's **Main Hand** option
  is left. A hand item part that is hidden (or under a hidden part) holds nothing.
- Held items are drawn as the game draws your own: in use they show it (a bow's pull, a crossbow's charge, a
  raised shield, ...), and OptiFine CIT rules for the hand apply. The spyglass held to the eye and the spear's
  stab and use motions are not drawn as the game does: the item stays in the hand.
- A pumpkin, banner or other non-armor item in the head slot is drawn at the model's head item part, or its
  head; a skull or head is drawn with the skull renderer as on a vanilla player. While a skull or head is worn,
  the model's head is hidden (CPM's **Hide head if skull**, on unless the model turns it off).
- At the arm or head (no item part), the item follows the model's scale (`MODEL_SCALE`): CPM places it at the
  unscaled vanilla arm instead.
- The gestures menu's preview does not show items.
- Drawing the items reads the draw's pose back through a probe quad and so depends on Horizon's internal quad
  code (`item_probe.rs`): re-check it whenever Horizon is updated.

The game leaves out the parrots on your shoulders as well, so the model draws them, as CPM does: each parrot
(the game's own parrot model, colour and on-shoulder animation) sits at the model's `Left Shoulder` / `Right
Shoulder` item part, following its animations (lifted a little more while sneaking, as in CPM), or, without one,
where vanilla puts it. A shoulder part that is hidden (or under a hidden part) holds no parrot. Item parts for
CPM's tool and any slots draw nothing, as in CPM. The parrots are drawn the same way as the items
(`parrot_probe.rs`, through `item_probe.rs`'s probe quad).

### Action states

The model's pose animations for what you do play as in CPM: punching (with the swing's progress), holding an
item in either hand, eating and drinking, blocking, drawing a bow and charging a crossbow (with their pull),
holding a charged crossbow, the spyglass, trident, goat horn, brush and spear in use, riptide, being on a
ladder or climbing one, crawling, swimming in water without the swim pose, freezing, being hurt and dying,
plus health, hunger, air and light, what is worn (a skull or helmet, an elytra or chest piece, leggings, boots)
and a parrot on either shoulder. The poses are picked in CPM's order. Speaking, VR, the in-GUI and in-menu poses
do not play.

### Render scale

A model's render scale (its position, rotation and per-axis scale, set in the CPM editor's scaling settings)
is applied to the player drawn in the world, with its armor, elytra and cape part, and in the gesture menu's and the
picker's previews. A held or head item placed by one of the model's item parts follows it; one held at the
plain hand or head (the model has no item part for it) does not, as in CPM. As in CPM, it is not applied to the
first-person arm. A scale below 0.1 or above 10 counts as 1. Gameplay scaling (entity size, eye height, hitbox
and other attributes) needs CPM on the server and is not applied. This includes an old model's single size
setting (the legacy scale), which is gameplay scaling: such a model is drawn at its normal size.

### First person

In first person the bare hand (and the arms under a held map) is the model's arm: the arm bone's cubes, with
the vanilla arm and sleeve if the model keeps them. As CPM draws it, the arm is static: it moves only with the
game's own swing and equip motion, not with the model's animations or gestures. A model's first-person hand
setting (its position, rotation and scale for each hand) moves that arm as in CPM; the item held in first person
is the game's own and does not move with it. This hooks the game's own
`first_person_arm` function, not a named hook point, so it holds for the pinned Horizon commit only: a Horizon
that renamed or changed that function fails to build the mod.

### Player heads

A player head whose profile is yours shows your model's head instead of the vanilla skull, as CPM's skull
renderers do (`AnimationMode.SKULL`): the head and its cubes only (the vanilla head and its hat too if the model
keeps them), posed by the model's **Skull render** and **Global** animations, with the head where the skull's
head is. The head part's own position, rotation and animation are not applied there, as in CPM. Yours means its
UUID is yours, or, for a head with no UUID, its name: on a server, the account or name you joined with; in a
world of your own, its player and your signed-in account. Other players' heads stay vanilla until multiplayer.

- **Shown:** a player head held, in a slot, dropped or in an item frame; worn by you (with **Hide head if
  skull** still hiding the body model's head), by a mob or by an armor stand. All of them are drawn through the
  game's player-head item renderer (`item_render::special::submit`, `PlayerHeadSpecialRenderer`), which this
  hooks: the game's own function, not a named hook point, so it holds for the pinned Horizon commit only. That
  renderer is in the game's client library, which cannot reach the mod's code, so the frame hook hands it the
  drawing (`item_render::cpm_skull_hook`). The vanilla skull is left out only when the model's head was drawn.
- **Not shown (vanilla):** a player head placed as a block. Horizon draws placed skulls in its block-entity
  renderer (`horizon-client-render`'s `render_one`), another crate again, with only the head's skin there and not
  its profile, and with geometry it may keep between frames, so it is not hooked.
- **Drawing:** the head is drawn with the model's texture in the head renderer's light. Glowing faces are drawn
  at full block light; the separate additive glow pass of the player model is not available to the item
  renderer.

### Converting projects

The CPM editor saves its work as a `.cpmproject`. To play one without exporting it in the editor first, press
**Convert project…** on the settings page. A file picker opens in the models folder (on Windows a PowerShell
dialog; on Linux `zenity` or `kdialog`; on macOS `osascript`). Pick the project. The conversion runs in the
background (the button says **Converting…**) and its outcome shows on the settings page by itself once it
ends. (If the page still says **Converting…**, press any of its buttons or reopen it: the first press on
**Converting…** after the conversion ended only shows the outcome, the next one starts a new conversion.)

- `<name>.cpmmodel` is written to `player_models`, named after the project. If a model of that name is already
  there and was not made by the converter, it is kept and the new one is `<name>_2.cpmmodel` (then `_3`, ...).
  An earlier conversion of the same project is overwritten. The converter marks its models by ending their
  description with ` (converted by CPM for Horizon, v4)` (the converter's version). If that model is the one
  you are drawn with, it is loaded again. A model made by an older converter (it lacked the armour parts,
  the render scale and first-person hand, or animated textures) still draws, but the page says `Old conversion: convert <name>.cpmproject again` and the log has a line for it.
- Converted models use CPM's current format (`CUBES`, `ROOT_INFO`, `ANIMATION_NEW`), so other players need a
  recent CPM to see them.
- The render scale, the first-person hand, cape parts, the cape texture and animated textures (of the skin, cape and
  elytra textures) are carried over.
- What the converter cannot carry over (extra textures, templates, gameplay scaling, tags, ...) is skipped: the page says
  `(N skipped, see log)` and the log has one `cpm: convert <file>: <what>` line for each.
- On an error nothing is written, and the page shows `Convert error: ...`. The converted model is loaded back
  before it is written: one that would not load is an error too (`exported model does not load: ...`).

**Gist limit** (the `gist_limit_kib` setting, 8 KiB by default; the button goes 4, 8, 16, 32, 64): CPM sends a
model to other players inline only when it is small. A converted model bigger than the limit still works for
you, but the converter also writes `<name>.gist.txt` next to it and opens the gist popup, so you can make a small
*link model* that points at a GitHub Gist instead. The page then says `Converted: <name>.cpmmodel — too big: the
gist popup opens when you return to the game`: the popup cannot open over the menus, so it opens once you close
them and are in a world (converted from the title screen: when you join a world).

### The gist flow

1. Convert a project bigger than the gist limit, then close the menus. The **Upload to a GitHub Gist** popup
   opens over the game.
2. Press **Copy text**: the gist text goes onto the clipboard. (If the clipboard does not take it, open
   `<name>.gist.txt` in the models folder and copy it from there.)
3. Press **Open gist.github.com**, sign in, paste the text into a new gist and create it as a **public** gist.
4. Copy the gist's page link (`https://gist.github.com/<user>/<id>`), paste it into the popup's field (Ctrl+V)
   and press **Save link** (or Enter).
5. The popup writes `<name>.link.cpmmodel` next to the model: the same name, description and skin type, with
   only the link inside. Escape or **Close** closes the popup.

### Link models

A link model (such as a `.link.cpmmodel`, or one made in the CPM editor with a gist link) holds no model of its
own, only the link. Select it like any other model: the first time, the mod downloads the gist in the background
(this needs an internet connection) and you are drawn vanilla until it arrives. The gist text is then cached in
`%APPDATA%\horizon\horizon-mods\.horizon-mod-data\cpm\gist-cache\<user>_<id>`, so later starts load it without
the network; delete that file to download it again (for example after you edited the gist).

If the download fails, or the gist does not hold CPM gist text, the settings page shows
`Error: cpm: <file>: gist git:<user>/<id>: <why>` and you are drawn vanilla. Select the model again (go around
the **Model** button) to try again. Downloads time out after 15 seconds and take at most 16 MiB.

### Other link types

A model may also link (as a link model, or with a definition or skin link part) to CPM's other loaders: a GitHub
repository file (`gh:`, `https://raw.githubusercontent.com/<path>`), CPM's paste site (`p:`,
`https://paste.tom5454.com/raw/<path>`, falling back to `https://cf-paste.tom5454.com/raw/<path>` when the first
does not connect), Pastebin (`pb:`, `https://pastebin.com/raw/<path>`) and CPM's models CDN (`ms:`,
`https://cdn-cpmmodels.tom5454.com/content/<path>`). They download, cache and fail as gists do: the cache is
`.horizon-mod-data\cpm\link-cache\<loader>\<path>` (every character of the path but letters, digits, `-` and `_`
written `~XX`), and a failure reads `Error: cpm: <file>: link <loader>:<path>: <why>`. A model with several links
downloads them one after the other. A link inside a linked package is refused, as in CPM; a `local:` link (CPM's
in-game test model) and an unknown loader are errors (`unsupported link`).

### Other players

Other players in the world are drawn with the model stored in their Mojang skin, on any server, as CPM draws a
skin's model (CPM's server plugin and Horizon worlds, which send models another way, are to come).

- **Loading**: each player's downloaded skin file is read, and its model loaded (its links downloaded and cached
  as yours are), off the game's thread. A player draws vanilla until their model is ready, and stays vanilla when
  their skin holds none. A skin change loads again; a player who leaves is dropped.
- **Drawing**: each model is posed from what the game knows of that player: the walk, head turn, sneaking,
  sprinting, swimming and crawling, elytra flight, sleeping, riding, on fire, freezing, riptide, ladders,
  the swing, the item in use (bow, crossbow, eating, blocking, ...), hurt and dying, health, and the light
  where they stand. Their armour, trims, elytra, cape, held and head items and shoulder parrots are drawn as
  yours are. What the game is not told of another player plays as off: hunger, air (Horizon does not keep
  another player's), creative flight, falling, and gestures and custom poses (those need CPM's server or
  Horizon worlds). As for you, the riptide swirl, stuck arrows and bee stingers, and other mods' layers are
  not drawn on a player the model draws.
- **Settings**: `Other players' models: On/Off` (on) and `Max other models: N` (8; 1, 2, 4, 8, 16 or 32), with
  how many are loaded. The nearest players get the places: with every place taken, a player at least 4
  blocks nearer than the farthest one drawn (drawn for 3 s or more) is loaded beside them, and takes that
  place only once their model is ready, so a player whose skin holds none never costs anyone theirs.
- **Limits** (CPM's for players who are not friends): textures at most 256 px a side (each image's own size),
  at most 256 cubes (extruded ones counted as CPM counts them), links at most 100 KiB each and 1 MiB of model
  data in all. A model past them draws vanilla, with one log line for that player. A link that failed to
  download is tried again after a minute, three tries in all.
- **Texture room**: Horizon gives a mod one 1024x1024 image. Each other player's model takes a region of it
  beside yours; when it is full, the rest draw vanilla until room frees up.
- Animated textures do not play on other players (CPM's default for players who are not friends), and their
  player heads stay vanilla.

## Not supported (yet)

Other players' player heads, gestures and custom poses, models from CPM servers and Horizon worlds, placed player heads, modded armor, the enchantment glint in the preview, and items and parrots in the
preview. Duplicated root parts (a second head, arm and so on) are drawn as
CPM draws them: they follow their body part's pose with their own offset, draw that part's skin box and second
layer unless hidden, take their animations and draw their cubes. Extruded parts are drawn as CPM draws them (the texture
sliced per texel, so its transparent texels leave holes).

## Layout

| File | What it is |
|---|---|
| `mixins/cpm/horizon-mixin.toml` | The bundle: seventeen modules, the HEAD hooks (and the `first_person_arm` and `player_head` injections), the `cpm-core` crate and the `[api]` package |
| `mixins/cpm/api/` | A trait-mod package that overrides nothing. It puts CPM in the game's mod list, which the game needs before it reserves the `horizon/modimage` sprite the model's texture goes in |
| `mixins/cpm/runtime.rs` | State and logic, added to the game as `game::cpm_mod`; also this crate's library, for its tests |
| `mixins/cpm/*.rs` | One small helper per hook (frame, listened, player_submit, first_person_arm, player_head, mods list, settings, click, key, and CPM's mod screen's screen_render, screen_click, screen_release, screen_drag, screen_scroll, screen_key, screen_char); `draw.rs` (`game::cpm_draw`) draws quads for player_submit and first_person_arm, and a player head's quads; `player_head.rs`, at the head of the game's player-head item renderer, places the skull's head part and asks `skull_hook.rs` (`item_render::cpm_skull_hook`, in the client library) for that drawing; `head_probe.rs` (`game::cpm_head_probe`) reads your identities and a head's profile; `pose_probe.rs` (`game::cpm_pose_probe`), `action_probe.rs` (`game::cpm_action_probe`), `equipment_probe.rs` (`game::cpm_equipment_probe`), `cape_probe.rs` (`game::cpm_cape_probe`) `item_probe.rs` (`game::cpm_item_probe`) and `parrot_probe.rs` (`game::cpm_parrot_probe`) read the local player's state, its actions, what it wears, its cape, what it holds and its shoulder parrots for player_submit (`item_probe.rs` also draws the held and head items, `parrot_probe.rs` the parrots); `others_probe.rs` (`game::cpm_others_probe`) reads another player's state, stacks, cape and parrots and draws them with their model; `skin_probe.rs` (`game::cpm_skin_probe`) reads the downloaded skin files of the local player and the other players (nearest first) for the models stored in them; `convert.rs` (`game::cpm_convert`) runs the file picker and the `.cpmproject` conversion on its own thread; `gist_fetch.rs` (`game::cpm_gist_fetch`) downloads a model's links (gists and CPM's other loaders) on its own thread; `screen.rs` (`game::cpm_screen`) routes those screen hooks to the CPM screen shown: `gist_screen.rs` (`game::cpm_gist`), the gist popup for a model too big to send directly, or `gesture_screen.rs` (`game::cpm_menu`), the gesture menu |
| `mixins/cpm/cpm-core/` | The `.cpmmodel` reader, model, animation engine and posing, free of the game |

```sh
(cd native/cpm && cargo test)                  # the runtime
(cd native/cpm/mixins/cpm/cpm-core && cargo test)   # the reader and engine
python native/tools/native.py check cpm        # apply on the tested Horizon and cargo check the game
                                               # (with the generated game executable the [api] package needs)
python native/tools/native.py package cpm      # native/target/cpm-0.2.0.hnmod
```

`cargo test` inside `cpm-core` leaves a `Cargo.lock` and a `target/` there. Delete both before `check`:
horizon-mixins refuses a bundle crate that ships its own `Cargo.lock`.

## Credits

Customizable Player Models is by tom5454, under the MIT License: this mod ports its file format and animation
behaviour to Horizon. `mixins/cpm/cpm-core/src/free_space_template.png` is CPM's free space template
(`assets/cpm/textures/template/free_space_template.png`), bundled unchanged under the same license. This mod is under the MIT License too (`LICENSE`).
