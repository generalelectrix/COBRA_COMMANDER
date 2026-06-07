# RGBW White-Diode Brightness Notes

Working notes on how bright the W diode is relative to the chromatic R, G, B diodes
on common entertainment LED hardware. These numbers drive `W_DIODE_BRIGHTNESS` in
`src/color.rs` and frame why the HSLuv→RGBW and HSI→RGBW conversions choose
different strategies.

## The `k_w` concept

Let `k_w` be the W diode's luminous output, at full drive, expressed in
"chromatic-channel-units" — where 1 chromatic-channel-unit is the lumens emitted
by a single R, G, or B die at the same drive current. Three values bracket the
useful range:

| `k_w` | Means | Implied algorithm |
|------:|:------|:------------------|
| 1.0 | W ≈ one chromatic LED | saikoled HSI→RGBW ([blog](https://blog.saikoled.com/post/44677718712/how-to-convert-from-hsi-to-rgb-white)) |
| 2.0 | W ≈ two chromatic LEDs | brightness-aware white subtraction |
| 3.0 | W ≈ R+G+B equal-mix | naive white subtraction |

Real fixtures sit somewhere on this axis. Picking the wrong value leaves either
desaturated colors too bright (over-estimate W) or too dim (under-estimate W).

## Findings across hardware regimes

| Hardware | `k_w` (W ÷ R+G+B summed photometric) |
|:---------|:-------------------------------------|
| Analog 5050 RGBW LED strip (24 V) | ≈ 0.8 |
| SK6812 RGBW addressable pixel | ≈ 0.85 |
| SaikoLED MyKi (algorithm design target, 2013) | ≈ 1.0 |
| **4-die quad LED PAR (Cree XM-L Color, Luxeon C Color)** | **≈ 2.0** |
| **6-die hex PAR (Osram OSTAR Stage / Chinese hex)** | **≈ 1.7–2.0** |

The factor of ~2× between pixel strings/strips and entertainment-grade pars is the
dominant story. Cobra runs on PARs and hex pars, so `k_w = 2.0` is the right
default. Pixel strings would want `k_w ≈ 1.0` if Cobra ever supports them
natively.

## Detail per regime

### 4-die quad RGBW pars (k_w ≈ 2.0)

The canonical part is **Cree XM-L Color RGBW** ([LEDsupply](https://www.ledsupply.com/leds/cree-xml-rgbw-star-led)),
behind many "quad" or "quad-color" wash pars. Minimum-bin lumens at 350 mA per
channel:

| Channel | Lumens |
|---------|-------:|
| White (cool) | 80–100 |
| Green | 87.4 |
| Red | 45.7 |
| Royal-Blue | 13.9 (radiometric; ~25–40 lm photometric blue) |
| **R+G+B summed photometric** | **~145–160 lm** |

→ W ≈ 55–65% of R+G+B sum ≈ **2.0× a single chromatic channel** ≈ 1.0× green alone.

**Lumileds Luxeon C Color** ([product page](https://lumileds.com/products/color-leds/luxeon-c-colors/),
[datasheet](https://www.ledsupply.com/content/pdf/Luxeon-c-color-line-datasheet.pdf))
shows the same pattern: R+G+B ≈ 233 lm at 350 mA, W ≈ 100–130 lm cool-white,
again ~50% of R+G+B sum.

**Why this `k_w`:** the W die is phosphor-converted blue (InGaN pump + YAG:Ce or
similar), which has roughly **2× the luminous efficacy** of an RGB-mixed white at
the system level (the "green gap" and red Stokes-shift inefficiency penalize RGB
mixing). The chromatic dies and the W die share the same drive current with no
calibration — manufacturers wire them at identical I_F and let the lumens fall
where they fall.

### 6-die hex RGBWAU pars (k_w ≈ 1.7–2.0)

Hex pars (ADJ Mega Hex Par, Chauvet COLORdash Hex, Elation Sixpar, Eurolite
PAR-64 HCL RGBAWUV) use either the **Osram OSTAR Stage LE RTDUW** family or
generic Chinese 6-in-1 dies. Osram publishes per-channel data:

- **LE RTDUW S2W** at 1 A typical: R ≈ 140 lm, G ≈ 280 lm, B ≈ 50 lm (binned),
  W ≈ 355 lm ([Farnell datasheet](https://www.farnell.com/datasheets/2034550.pdf))
- **LE RTDUW S2WN** at 1 A typical: R 90–140 lm, G 180–355 lm, W 224–450 lm
  ([Mouser datasheet](https://www.mouser.com/pdfDocs/LERTDUWS2WN_ENDatasheet.pdf))
- **LE RTDUW S2WP** at 1.4 A: R 112–280 lm, G 280–710 lm, W up to ~710 lm
  ([ams-osram datasheet](https://look.ams-osram.com/m/d9e4a1902c08390/original/LE-RTDUW-S2WP.pdf))

→ W is approximately equal to R+G+B summed at equal I_F, giving k_w ≈ 1.7–2.0,
same as the 4-die quad case.

**Why the same `k_w` despite the die-count change:** going quad → hex shrinks
every die by ~33% (six dies share the same dome instead of four), but the W:RGB
brightness *ratio* is preserved because phosphor white's lm/mm² is class-bound,
not size-bound. Absolute lumens fall together; the ratio survives.

**Amber** (590 nm) at equal drive gives ~0.5–0.8× a red's lumens
([Moon LEDs technical brief](https://www.moon-leds.com/news-what-is-amber-led-pc-amber-1800k-2200k-monochromatic-amber.html)).
**UV** (365/395 nm) is < 0.5 lm/W photopic
([Waveform Lighting](https://www.waveformlighting.com/tech/top-4-things-to-consider-before-buying-uv-blacklights))
— treat as zero photometric contribution.

Fixture vendors (ADJ Mega Hex, Elation [SixPar 200](https://www.elationlighting.com/products/sixpar-200) /
[SixPar 300](https://www.elationlighting.com/products/sixpar-300), Chauvet
[COLORdash Par H7X](https://chauvetprofessional.com/product/colordash-par-h7x-ip/),
[Eurolite PAR-64 HCL RGBAWUV](https://www.prolighting.de/en/lighting-effects/spotlights/led-spots/eurolite-led-par-64-hcl-12x10w-floor-schwarz-rgbawuv.html))
publish only total fixture lumens or peak lux, never per-channel — the
component-level data from Osram / Lumileds is the only triangulation route.

### Addressable pixel strings (k_w ≈ 0.85)

**SK6812 RGBW** (the canonical "NeoPixel RGBW" part, sold by Adafruit and
BTF-Lighting) at 13 mA per channel
([Normand LED datasheet](https://www.normandled.com/upload/201805/SK6812RGBX-XX%20Datasheet.pdf),
[Adafruit-hosted Rev01](https://cdn-shop.adafruit.com/product-files/2757/p2757_SK6812RGBW_REV01.pdf)):

| Channel | Lumens @ 13 mA |
|---------|---------------:|
| Red | 1.0–2.0 |
| Green | 3.0–5.0 |
| Blue | 1.0–2.0 |
| White (CW 6000–7000 K) | 5.0–7.0 |

→ W ≈ 6 lm vs R+G+B ≈ 7 lm summed → **k_w ≈ 0.85**.

The package puts four discrete dies under a single 5050 dome
([BTF-Lighting product page](https://www.btf-lighting.com/products/sk6812similar-ws2812b-rgbw-rgbnature-warm-white-5050-smd-individually-addressable-digital-led-chip-pixels-dc-5v),
[LEDYi technical comparison](https://www.ledyilighting.com/sk6812-vs-ws2812b-which-led-strip-light-is-best-why/)).
The W die is a separately-fabricated phosphor-white chip of *comparable area* to
each chromatic die — confirmed by the fact that driving blue alone faintly
excites the W phosphor by package proximity. Unlike entertainment pars, the W
die here doesn't dominate by area, so its lumen advantage from phosphor
efficacy is muted.

### Analog 5050 RGBW strips (k_w ≈ 0.8)

Same 5050 4-in-1 die as SK6812 RGBW, just non-addressable (24 V or 12 V common
anode). Representative per-channel spec for a 60 LED/m strip:
R 128 / G 326 / B 82 / W 432 lm/m
([ledmyplace product spec](https://www.ledmyplace.com/products/rgbw-led-strip-lights-12v-led-tape-light-w-white-366-lumens-ft),
[Super Bright LEDs](https://www.superbrightleds.com/5m-rgbw-led-strip-light-4-in-1-chip-5050-color-changing-led-tape-light-12v-24v-ip20)).

→ W / (R+G+B) ≈ 432 / 536 ≈ **0.81** — same ballpark as SK6812.

### SaikoLED MyKi (k_w ≈ 1.0)

The HSI→RGBW saikoled algorithm was developed for the
**[SaikoLED MyKi](https://www.crowdsupply.com/saiko-led/myki-led-light)**, a
12 W RGBW spotlight architected with **3 W per chromatic channel + 3 W for
white** — equal electrical power per channel. On the mid-power phosphor whites
available in 2013 that yields W ≈ 1× single chromatic channel photometric, i.e.
k_w ≈ 1.0.

The saikoled HSI→RGBW math reads `W = (1-S)·I` with no scaling
([blog post](https://blog.saikoled.com/post/44677718712/how-to-convert-from-hsi-to-rgb-white)).
That's only correct when k_w ≈ 1.0. Porting the same algorithm to entertainment
PARs (k_w ≈ 2.0) over-drives W by ~2×, causing desaturated mixes to come out
chalky and washed-out.

## Algorithm implications

Two candidate RGBW conversion strategies, both implicitly assuming some `k_w`:

- **Naive white subtraction** (`W = min(R,G,B); R,G,B -= W`) assumes
  **k_w = 3** (W ≈ R+G+B equal-mix). On a real par (k_w ≈ 2) this
  *over*-estimates W, so pastels emit ~33% less light than HSLuv intended.
- **SaikoLED HSI→RGBW** (`W = (1-S)·I`, chromatic scaled by `S·I/3`) assumes
  **k_w = 1**. On a real par this *under*-estimates W, so desaturated colors
  come out ~2× brighter than HSI intensity intended.

The fix used by `Hsv::rgbw()` and `Hsluv::rgbw()` in `src/color.rs` is
**brightness-aware white subtraction**, generalized over `k_w`. Both methods
convert their source space to linear RGB and then call a shared
`linear_rgb_to_rgbw` helper:

```rust
let m = r.min(g).min(b);
let w = (3.0 * m / W_DIODE_BRIGHTNESS).min(1.0);
let c = w * W_DIODE_BRIGHTNESS / 3.0;
// emit (r - c, g - c, b - c, w)
```

At `k_w = 3` this collapses to naive white subtraction; at `k_w = 1` it
approaches the saikoled W allocation; at `k_w = 2` it matches typical
entertainment pars. The "one chromatic channel always zero" property holds
except on near-white inputs (`min(r,g,b) > k_w/3`) where W saturates and the
chromatic channels carry the residual achromatic load — physically correct
behavior because W can't get any brighter.

`Hsi::rgbw` deliberately does NOT use this helper — HSI's intensity invariant
(constant total LED drive) is incompatible with white subtraction's lightness
invariant, so HSI keeps the saikoled sector scheme.

**HSI's sector scheme isn't redundant — it solves a different problem.** HSI's
contract is "intensity = total LED drive across all diodes, constant for any
(h, s)"; the sector algorithm is constructed to maintain that invariant.
HSLuv's contract is *perceptual lightness*, which brightness-aware white
subtraction preserves on any fixture with a correctly-tuned `k_w`. Each
algorithm matches its color space's semantics.

## Second-order knobs

Two factors move `k_w` within a hardware class:

1. **W color temperature.** Cool white (~6000 K) has higher photopic efficacy
   than warm white (~3000 K). Warm-white parts skew toward k_w ≈ 1.0 even in
   par form factors; cool-white pixels skew toward k_w ≈ 1.0+ where strips
   would otherwise be 0.8. If per-fixture calibration ever becomes a thing,
   parameterize on W CCT, not die count.
2. **Drive-current calibration.** Some higher-end fixtures (Robe, Martin) do
   calibrate W drive current to balance perceived brightness; cheap fixtures
   never do. Assume "no calibration" for the bulk of the market.

## Sources

- [SaikoLED: How to convert from HSI to RGB+White](https://blog.saikoled.com/post/44677718712/how-to-convert-from-hsi-to-rgb-white)
- [SaikoLED: Why every LED light should be using HSI](https://blog.saikoled.com/post/43693602826/why-every-led-light-should-be-using-hsi)
- [SaikoLED MyKi — Crowd Supply](https://www.crowdsupply.com/saiko-led/myki-led-light)
- [Cree XM-L Color RGBW (LEDSupply)](https://www.ledsupply.com/leds/cree-xml-rgbw-star-led)
- [Lumileds Luxeon C Color Line](https://lumileds.com/products/color-leds/luxeon-c-colors/)
- [Luxeon C Color Line datasheet](https://www.ledsupply.com/content/pdf/Luxeon-c-color-line-datasheet.pdf)
- [SK6812RGBX-XX datasheet (Normand LED)](https://www.normandled.com/upload/201805/SK6812RGBX-XX%20Datasheet.pdf)
- [SK6812RGBW Rev01 (Adafruit-hosted)](https://cdn-shop.adafruit.com/product-files/2757/p2757_SK6812RGBW_REV01.pdf)
- [Adafruit NeoPixel RGBW Strip Product 2824](https://www.adafruit.com/product/2824)
- [BTF-Lighting SK6812 RGBW product page](https://www.btf-lighting.com/products/sk6812similar-ws2812b-rgbw-rgbnature-warm-white-5050-smd-individually-addressable-digital-led-chip-pixels-dc-5v)
- [LEDYi: SK6812 vs WS2812B technical comparison](https://www.ledyilighting.com/sk6812-vs-ws2812b-which-led-strip-light-is-best-why/)
- [Osram LE RTDUW S2W datasheet](https://www.farnell.com/datasheets/2034550.pdf)
- [Osram LE RTDUW S2WN datasheet](https://www.mouser.com/pdfDocs/LERTDUWS2WN_ENDatasheet.pdf)
- [Osram LE RTDUW S2WP datasheet](https://look.ams-osram.com/m/d9e4a1902c08390/original/LE-RTDUW-S2WP.pdf)
- [Cree app note: Optimizing 4-Channel Color Mixing](https://www.cree-led.com/news/ap-note-optimizing-4-channel-color-mixing-systems-for-color-rendering/)
- [ledmyplace RGBW 5050 60 LED/m spec](https://www.ledmyplace.com/products/rgbw-led-strip-lights-12v-led-tape-light-w-white-366-lumens-ft)
- [Super Bright LEDs 5050 RGBW strip](https://www.superbrightleds.com/5m-rgbw-led-strip-light-4-in-1-chip-5050-color-changing-led-tape-light-12v-24v-ip20)
- [Chauvet COLORdash Par H7X IP](https://chauvetprofessional.com/product/colordash-par-h7x-ip/)
- [Elation SixPar 200](https://www.elationlighting.com/products/sixpar-200)
- [Elation SixPar 300](https://www.elationlighting.com/products/sixpar-300)
- [ADJ Mega Hex Par](https://www.adj.com/mega-hex-par)
- [Eurolite PAR-64 HCL RGBAWUV](https://www.prolighting.de/en/lighting-effects/spotlights/led-spots/eurolite-led-par-64-hcl-12x10w-floor-schwarz-rgbawuv.html)
- [Moon LEDs: PC amber 590 nm efficacy](https://www.moon-leds.com/news-what-is-amber-led-pc-amber-1800k-2200k-monochromatic-amber.html)
- [Waveform Lighting: UV blacklight lm/W](https://www.waveformlighting.com/tech/top-4-things-to-consider-before-buying-uv-blacklights)
- [NCBI: LED efficacy review](https://pmc.ncbi.nlm.nih.gov/articles/PMC7105460/)
