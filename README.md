# Nebula Cluster

**Nebula Cluster** is a free open-source dirt box plugin made by **Nebula Audio**.

It is built for one job: take clean, polite audio and give it weight, bite, heat, movement, and attitude. Put it on drums, bass, synths, vocals, guitars, loops, rooms, buses, or anything that needs to stop behaving.

Nebula Cluster is released under the MIT license. It is not proprietary software.

<img width="1183" height="792" alt="Image" src="https://github.com/user-attachments/assets/fda701db-caec-4143-b677-3e70041636d4" />

## The Sound

Nebula Cluster is not a transparent utility plugin pretending to be exciting. It is a character box.

At low settings it adds thickness, density, and subtle harmonic lift. Push it harder and it turns into a crunchy saturation engine with enough harmonic control to go from warm and rounded to sharp, torn, and aggressive. The filter and compressor sections make it easy to shape the chaos into something that still works in a mix.

Use it when a track needs:

- More grit.
- More forward motion.
- More body.
- More edge.
- More impact.
- More strange little sparks around the sound.

It can be gentle, but it is happiest when you ask it to misbehave.

## Why Use It

Nebula Cluster gives you the classic dirt-box workflow: drive, shape, squeeze, blend, and compare. The controls are immediate, the UI is built around fast decisions, and the post-effects analyzer shows what the plugin is doing to the signal in real time.

It is especially good for:

- Smashing drum loops.
- Making kicks and snares hit harder.
- Turning plain bass into a mix anchor.
- Adding edge to soft synths.
- Giving vocals a gritty parallel layer.
- Warming up sterile digital sources.
- Creating aggressive transitions and sound-design layers.
- Making boring buses feel alive.

## Plugin Formats

- CLAP
- VST3

## Platforms

- macOS Universal: Apple Silicon and Intel
- Linux x86_64

Nebula Cluster is 64-bit only.

## Interface

The editor uses a deep black neon sci-fi look with resizable controls. The main analyzer stays visible globally while the effect sections live in tabs, so you can shape the sound without losing sight of the output.

The UI includes:

- Global toolbar.
- Post-effects spectrum analyzer.
- Waveform display.
- Output peak meter.
- Gain reduction meter.
- Tabbed Global, Distortion, Filter, and Compressor controls.
- Preset save/load.
- Undo and redo.
- A/B comparison.
- Chaos randomization.
- MIDI learn.

## The Dirt Engine

The distortion section is the heart of Nebula Cluster.

Instead of giving you one fixed distortion flavor, it lets you build the character from multiple harmonic layers:

- Saturation intensity.
- 2nd order harmonics for warmth and thickness.
- 3rd order harmonics for bite and aggression.
- 4th through 7th order harmonics for extra edge, texture, and fuzz-like complexity.
- Dedicated distortion mix.
- Distortion phase flip.
- Pre-shaping HPF and LPF controls.
- Continuously variable filter slopes.

Turn up Saturation for more grind. Blend the harmonic controls to decide whether the sound gets round, sharp, buzzy, raspy, or broken.

## Filter Section

After the distortion stage, the filter section lets you carve the result into place.

Use it to:

- Remove low-end mud after heavy saturation.
- Tame harsh high end.
- Add resonant bite around cutoff points.
- Create focused midrange grit.
- Shape parallel dirt layers so they sit under the dry sound.

Controls:

- HPF
- HPF Slope
- HPF Resonance
- LPF
- LPF Slope
- LPF Resonance

## Compressor Section

The compressor is there to make the dirt move.

Modes:

- **Down**: classic downward compression.
- **Up**: brings lower-level detail forward.
- **Boost**: pushes the signal with controlled upward-style energy.

Use it after distortion to clamp, pump, thicken, or pull hidden texture forward.

Controls:

- Mode
- Ratio
- Knee
- Makeup
- Boost
- Attack Threshold
- Attack Time
- Release Threshold
- Release Time
- Hold

## Global Controls

Nebula Cluster also includes the practical controls needed to keep extreme tones usable:

- Input Level
- Input Pan
- Output Level
- Output Pan
- Global Mix
- Oversampling: Off, 2x, 4x, 6x, 8x
- Global Phase
- FX Bypass

The pan knobs are bipolar: center is 12 o'clock, left turns left, and right turns right.

## Analyzer

The analyzer is placed after the effects, so it shows the signal coming out of Nebula Cluster.

It includes:

- Real-time spectrum view.
- Real-time waveform view.
- Output peak readout.
- Gain reduction readout.
- Frequency grid.
- dB grid.

This makes it easy to see when the low end is getting too wild, when the high end is getting fizzy, or when compression is pulling the sound into shape.

## Presets, A/B, and Chaos

Nebula Cluster is built for fast exploration.

### Presets

Save your current setup and recall it later from the preset menu.

### A/B

Set up one tone on A, switch to B, create another tone, then flip back and forth until one wins.

### Chaos

Chaos randomizes the plugin controls and section states. Sometimes it creates something subtle. Sometimes it creates a problem. Sometimes the problem is the sound.

Use it when you want the plugin to surprise you.

## MIDI Learn

Nebula Cluster includes MIDI learn for mapping hardware controls.

Basic workflow:

1. Click MIDI Learn.
2. Click a knob, button, or menu control.
3. Move a MIDI controller knob or slider.
4. The mapping is assigned.

Right-click MIDI Learn to manage mappings:

- MIDI On/Off
- Clean Up
- Roll Back
- Save

## Safety

Nebula Cluster can get loud if you push it. That is part of the point.

To keep the output host-safe, the plugin includes a final stereo safety limiter at approximately `-0.3 dBFS`. This helps prevent extreme settings from overloading the output while still letting the plugin hit hard.

## Quick Start

Try these starting points:

### Drum Crush

1. Turn on Distortion.
2. Push Saturation.
3. Add 3rd and 5th order harmonics.
4. Turn on Compressor in Down mode.
5. Blend with Global Mix.

### Bass Weight

1. Add 2nd order harmonics.
2. Keep Saturation moderate.
3. Use the LPF to shave off fizz.
4. Add a little compressor Boost.
5. Match output level by ear.

### Parallel Vocal Dirt

1. Drive Saturation harder than feels reasonable.
2. Use HPF to remove low-end rumble.
3. Use LPF to darken the grit.
4. Lower Global Mix until the dirt sits behind the dry vocal.

### Synthetic Edge

1. Add 3rd, 5th, and 7th harmonics.
2. Use Oversampling if the top end gets too sharp.
3. Shape with the post filter.
4. Use A/B to compare subtle and extreme versions.

## Building From Source

Nebula Cluster can be built locally with the included scripts.

### macOS Universal

```sh
./scripts/build_macos_universal.sh
```

### Linux x86_64

```sh
./scripts/build_linux_x86_64.sh
```

Build output is written to:

```text
target/bundled/
```

## Installing

After building, copy the generated bundles from `target/bundled/` into your plugin folders.

Common macOS locations:

```text
~/Library/Audio/Plug-Ins/CLAP/
~/Library/Audio/Plug-Ins/VST3/
```

Common Linux user locations:

```text
~/.clap/
~/.vst3/
```

Then rescan plugins in your DAW or plugin host.

## Testing

Run the release test suite:

```sh
cargo test --release -- --test-threads=1 --nocapture
```

Run the audio evaluation helper:

```sh
./scripts/run_audio_evaluation.sh
```

The project includes automated checks for audio behavior, stability, state reset, sample-rate switching, randomized stress cases, and extreme-gain output safety.

---
Pre-built CLAP and VST3 binaries can be bought from Gumroad:
https://subhankar42.gumroad.com/l/mdhqe
---

## License

Nebula Cluster is free open-source software under the MIT license. See `LICENSE` for the full license text.
