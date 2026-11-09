# CustomPlayerModels for Horizon

> [!WARNING]
> **Not every feature has been tested in game.** I don't have models that use every CPM feature, so some
> features are checked only by automated tests. Animated textures, duplicated body parts, non-gist links and
> models stored in a skin are among them. If a model looks or behaves differently than it does in real CPM, or
> doesn't load, please [open an issue](https://github.com/Scrapper1824/CustomPlayerModels-Horizon/issues/new).
> Include a screenshot and, if you can, the model file or a link to it. Your reports are how these features
> get fixed.

An unofficial port of [Customizable Player Models](https://github.com/tom5454/CustomPlayerModels) (CPM) by tom5454
to [Horizon](https://github.com/horizon-rust/horizon), the Rust Minecraft client, as a Horizon **native mod**.

It draws you with any CPM model (`.cpmmodel`), plays the model's animations, and gives you CPM's gesture menu.
It also converts `.cpmproject` files from the CPM editor into `.cpmmodel` files.

> This port is not affiliated with tom5454 or the CPM project. Models made with CPM's editor load as they are.

## Feature comparison

🟢 = supported  ·  🔴 = not supported (yet)

### Models

| Feature | Original CPM | This port |
|---|:-:|:-:|
| Load `.cpmmodel` files | 🟢 | 🟢 |
| Cubes, per-face UVs, colours, glow (emissive) parts | 🟢 | 🟢 |
| Hiding and moving vanilla body parts | 🟢 | 🟢 |
| Copy-transform effects | 🟢 | 🟢 |
| Custom armour and elytra parts (armour toggles) | 🟢 | 🟢 |
| Custom cape part, and the vanilla cape | 🟢 | 🟢 |
| Held items on item parts (main hand, off hand, head) | 🟢 | 🟢 |
| "Hide head if skull" | 🟢 | 🟢 |
| Render scale (model size / offset / rotation) | 🟢 | 🟢 |
| First-person hand position | 🟢 | 🟢 |
| Model arm in first person | 🟢 | 🟢 |
| Link models (GitHub Gist `git:` links) | 🟢 | 🟢 |
| Other link types | 🟢 | 🟢 |
| Extruded parts | 🟢 | 🟢 |
| Duplicated root parts | 🟢 | 🟢 |
| Animated textures | 🟢 | 🟢 |
| Enchantment glint, emissive and custom (resource-pack / CIT) armour | 🟢 | 🟢 |
| Parrots on shoulder parts (tool and any-slot parts draw nothing, as in CPM) | 🟢 | 🟢 |
| Models on player heads (skulls) (your own head until multiplayer) | 🟢 | 🟢 |
| Model stored in the skin file | 🟢 | 🟢 |

### Animations

| Feature | Original CPM | This port |
|---|:-:|:-:|
| Legacy and new animation formats | 🟢 | 🟢 |
| Movement poses (walk, run, sneak, swim, fly, elytra, ride, sleep, jump, fall) | 🟢 | 🟢 |
| Staged animations (setup / finish stages) | 🟢 | 🟢 |
| Action poses (punch, bow, crossbow, eat, block, spyglass, trident, horn, brush) | 🟢 | 🟢 |
| Holding, ladder, crawling, freezing, on fire | 🟢 | 🟢 |
| Health, hunger, air and light values | 🟢 | 🟢 |
| Hurt pose | 🟢 | 🟢 |
| Dying pose | 🟢 | 🔴 (Horizon hides the body while dying) |
| Gestures, custom poses, toggles, sliders, dropdowns | 🟢 | 🟢 |
| Gesture parameters saved per model | 🟢 | 🟢 |
| Speaking pose (voice chat) | 🟢 | 🔴 |
| Parrot poses (parrot on the left / right shoulder) | 🟢 | 🟢 |
| Spear and spyglass item motions | 🟢 | 🔴 |
| VR and OSC support | 🟢 | 🔴 |

### Tools and menus

| Feature | Original CPM | This port |
|---|:-:|:-:|
| Gesture menu with a 3D model preview | 🟢 | 🟢 |
| Model picker with icons and a 3D preview | 🟢 | 🟢 |
| `.cpmproject` → `.cpmmodel` converter (with the gist upload flow) | 🟢 | 🟢 |
| In-game model editor | 🟢 | 🔴 |
| Blockbench plugin | 🟢 | 🔴 |

### Multiplayer and gameplay

| Feature | Original CPM | This port |
|---|:-:|:-:|
| Your own model | 🟢 | 🟢 |
| Other players' models | 🟢 | 🔴 |
| Server plugin and model sync | 🟢 | 🔴 |
| Gameplay scaling (size, eye height, hitbox, reach, attributes) | 🟢 | 🔴 (needs CPM's server plugin) |

## Install

You need [Horizon](https://github.com/horizon-rust/horizon) and its launcher. This mod was built and tested against
Horizon commit `02c8607` (`horizon_revision` in `native/cpm/mixins/cpm/horizon-mixin.toml`). Other Horizon
versions may not build it.

1. Download `cpm.hnmod` from the [latest release](../../releases/latest).
2. Copy it to `%APPDATA%\horizon\horizon-native-mods\cpm.hnmod`. Create the folder if it is missing.
3. Turn it on: add the line `mod.cpm:true` to `%APPDATA%\horizon\horizon-mods.txt`. Create the file if it is
   missing.
4. Build it. Native mods are compiled into the game, and the first build downloads Rust, so it can take 20 to 30
   minutes. Either open the launcher's **Mods** page and press **Build** on the "Native build" row, or run
   `horizon-launcher native-build --trust-native-code`. Native mods are not sandboxed: only build code you trust.
5. Play the instance.

The paths are for the default **Horizon** instance. For another instance, use its own game folder.

## Use

1. Put your models in `%APPDATA%\horizon\player_models`. The **Open models folder** button in
   Options → Mods → CPM → Settings opens it.
2. Pick a model: press `G` for the gesture menu, then **Models** in its bottom left (a picker with icons and a 3D
   preview). You can also cycle through models with the **Model** button on the settings page.
3. Press F5 to see yourself. The gesture menu (`G`, or **Gestures…** on the settings page) plays the model's
   gestures and poses.

To convert a `.cpmproject`, press **Convert project…** on the settings page and pick the file. Models over 8 KiB
get a popup that walks you through putting them in a GitHub Gist, the same way CPM does.

The full guide, with every setting, the gist flow and the known differences from CPM, is in
[native/cpm/README.md](native/cpm/README.md).

## Building from source

```sh
python native/tools/native.py package cpm
```

This writes `native/target/cpm-0.1.5.hnmod`. It needs Python 3.11 or newer. You can also build the source folder
straight into the game:

```sh
horizon-launcher native-build native/cpm/mixins/cpm --trust-native-code
```

The model format, animation engine and converter live in the `cpm-core` crate (`native/cpm/mixins/cpm/cpm-core`),
which has no game types and its own tests (`cargo test`). Some tests use sample models that are not included here;
they skip when the files are missing (see `native/cpm/mixins/cpm/cpm-core/tests/samples/README.md`).

## Credits

- [Customizable Player Models](https://github.com/tom5454/CustomPlayerModels) by tom5454 (MIT). This port follows
  its file format and behaviour. `free_space_template.png` (in `native/cpm/mixins/cpm/cpm-core/src/`, which
  marks the skin pixels a model is stored in) is CPM's own `assets/cpm/textures/template/free_space_template.png`,
  bundled unchanged (MIT).
- [Horizon](https://github.com/horizon-rust/horizon) by Ludovic Marie and the Horizon contributors (MIT).
  `native/tools/native.py` comes from Horizon.

## License

MIT. See [LICENSE](LICENSE).
